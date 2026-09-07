//! Cooperative identity on the desktop (GAP-010, GAP-018): the AIS feeds the baseline
//! names, their reports associated with tracks, and the identification engine fed from
//! them.
//!
//! **A cooperative report is a claim.** A vessel says who it is on a channel anybody can
//! transmit on, so the evidence submitted for it argues for `Neutral` at a fixed, modest
//! confidence and never declares on its own: `gungnir-identification`'s engine fuses it
//! under the baseline's thresholds (DN-08 §5), PN-04 shows every piece, and DN-15's
//! cooperative detectors watch for the report and the track disagreeing or the report
//! going quiet. The picture's own classification is **not** rewritten here: the engine's
//! conclusion is shown beside the evidence, and carrying it onto the track is the
//! pipeline's act (GAP-011).
//!
//! Association is by proximity: the report is paired with the nearest track inside
//! [`ASSOCIATION_GATE_M`]. The separation is recorded for the detector rather than used
//! to refuse the pairing, because a report that lands far from its track is exactly what
//! the detector exists to see.

use std::collections::HashMap;
use std::time::Duration;

use gungnir_config::{AisSource, ConfigBaseline};
use gungnir_identification::{
    IdentificationDecision, IdentificationEngine, IdentificationEvidence,
};
use gungnir_ingest::adapters::ais::{
    AisReceiverAdapter, AisStatsSink, CooperativeReport, CooperativeSink, NmeaSource,
    RecordedNmeaSource, TcpNmeaSource,
};
use gungnir_ingest::IngestGateway;
use gungnir_model::{Classification, Geodetic, LocalFrame, MissionTime, SensorId, TrackId};

use crate::state::AppState;

/// How far a report may land from a track and still be taken as that track's.
pub const ASSOCIATION_GATE_M: f64 = 5_000.0;
/// What a self-declared identity is worth on its own.
pub const COOPERATIVE_CONFIDENCE: f32 = 0.5;

/// What the desktop keeps of its bound AIS feeds.
#[derive(Debug, Default)]
pub struct BoundAisFeeds {
    pub reports: Vec<CooperativeSink>,
    pub stats: Vec<(String, AisStatsSink)>,
}

/// The last cooperative report associated with a track, for PN-04 and the detectors.
#[derive(Debug, Clone, PartialEq)]
pub struct LastCooperative {
    pub mmsi: u32,
    pub at: MissionTime,
    pub separation_m: f64,
    /// The report's declared identity (a civil vessel) against the picture's class.
    pub disagrees: bool,
    /// The platform class the report declares (GAP-027), from the AIS ship type, when
    /// the static report carried one.
    pub platform_class: Option<String>,
}

/// The platform class an AIS ship type declares (ITU-R M.1371-6 Table 53), as a
/// `surface.*` class id the baseline's lethality table can name. Codes the table does
/// not assign, and the reserved ones, yield `None` rather than a guess.
#[must_use]
pub fn platform_class_of_ship_type(code: u8) -> Option<&'static str> {
    Some(match code {
        20..=29 => "surface.wig",
        30 => "surface.fishing",
        31 | 32 => "surface.towing",
        33 => "surface.dredging",
        34 => "surface.diving",
        35 => "surface.military",
        36 => "surface.sailing",
        37 => "surface.pleasure",
        40..=49 => "surface.high-speed-craft",
        50 => "surface.pilot",
        51 => "surface.search-and-rescue",
        52 => "surface.tug",
        53 => "surface.port-tender",
        54 => "surface.anti-pollution",
        55 => "surface.law-enforcement",
        58 => "surface.medical",
        60..=69 => "surface.passenger",
        70..=79 => "surface.cargo",
        80..=89 => "surface.tanker",
        90..=99 => "surface.other",
        _ => return None,
    })
}

/// The class lethality per track for the assessor (GAP-027): the declared class looked
/// up in the baseline's table. A track with no declared class, or a class the table does
/// not list, is absent and weighs 1.0 there.
#[must_use]
pub fn class_weights(state: &AppState) -> std::collections::HashMap<TrackId, f64> {
    state
        .cooperative
        .by_track
        .iter()
        .filter_map(|(track, last)| {
            let class = last.platform_class.as_deref()?;
            let weight = state.config.assessment.lethality_by_class.get(class)?;
            Some((*track, *weight))
        })
        .collect()
}

/// The association memory between frames.
#[derive(Debug, Default)]
pub struct CooperativeState {
    pub by_track: HashMap<TrackId, LastCooperative>,
    /// Pairs already submitted as evidence, so a report every ten seconds does not
    /// accumulate into certainty.
    pub submitted: std::collections::HashSet<(TrackId, u32)>,
    pub matched: u64,
    /// Reports with a position that no track was near.
    pub unmatched: u64,
}

fn open_source(source: &AisSource) -> Result<Box<dyn NmeaSource>, String> {
    match source {
        AisSource::Tcp { addr } => {
            let addr = addr
                .parse()
                .map_err(|e| format!("{addr}: not a socket address ({e})"))?;
            TcpNmeaSource::connect(addr, Duration::from_secs(3))
                .map(|s| Box::new(s) as Box<dyn NmeaSource>)
                .map_err(|e| e.to_string())
        }
        AisSource::File { path } => RecordedNmeaSource::open(std::path::Path::new(path))
            .map(|s| Box::new(s.with_lines_per_poll(64)) as Box<dyn NmeaSource>)
            .map_err(|e| e.to_string()),
    }
}

/// Bind every configured AIS feed into the gateway. A feed that cannot be opened is an
/// alert, not a silent absence; a deployment with no local frame binds nothing and says
/// why.
pub fn bind_feeds(
    config: &ConfigBaseline,
    gateway: &mut IngestGateway,
    alerts: &mut Vec<String>,
) -> BoundAisFeeds {
    let mut bound = BoundAisFeeds::default();
    if config.ais_feeds.is_empty() {
        return bound;
    }
    let Some(frame) = config.origin.map(|[lat_rad, lon_rad, alt_m]| {
        LocalFrame::new(Geodetic {
            lat_rad,
            lon_rad,
            alt_m,
        })
    }) else {
        alerts.push(format!(
            "{} AIS feed(s) configured and no local frame origin declared: no receiver is \
             bound, because a report cannot be placed without one",
            config.ais_feeds.len()
        ));
        return bound;
    };
    for feed in &config.ais_feeds {
        match open_source(&feed.source) {
            Ok(source) => {
                let reports = CooperativeSink::default();
                let stats = AisStatsSink::default();
                let adapter = AisReceiverAdapter::new(
                    feed.name.clone(),
                    SensorId(feed.sensor_id),
                    frame,
                    source,
                )
                .with_report_sink(reports.clone())
                .with_stats_sink(stats.clone());
                gateway.add_adapter(Box::new(adapter));
                bound.reports.push(reports);
                bound.stats.push((feed.name.clone(), stats));
            }
            Err(err) => alerts.push(format!("AIS feed {} not bound: {err}", feed.name)),
        }
    }
    bound
}

/// The tick step: associate every report with a track and feed the engine.
pub fn tick(state: &mut AppState) {
    if state.ais_sinks.is_empty() {
        return;
    }
    let mut drained = Vec::new();
    for sink in &state.ais_sinks {
        if let Ok(mut q) = sink.lock() {
            drained.extend(q.drain(..));
        }
    }
    if drained.is_empty() {
        return;
    }
    let tracks: Vec<(TrackId, [f64; 3], Classification)> = state
        .tracking
        .tracks()
        .iter()
        .map(|t| (t.id, t.position_enu(), t.classification))
        .collect();
    for report in drained {
        associate(state, &tracks, &report);
    }
}

fn associate(
    state: &mut AppState,
    tracks: &[(TrackId, [f64; 3], Classification)],
    report: &CooperativeReport,
) {
    let Some(pos) = report.position_enu else {
        return;
    };
    let nearest = tracks
        .iter()
        .map(|(id, p, class)| {
            let d = ((p[0] - pos[0]).powi(2) + (p[1] - pos[1]).powi(2)).sqrt();
            (*id, d, *class)
        })
        .filter(|(_, d, _)| *d <= ASSOCIATION_GATE_M)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let Some((track, separation_m, class)) = nearest else {
        state.cooperative.unmatched += 1;
        return;
    };
    state.cooperative.matched += 1;
    // A vessel declaring itself is a civil claim; a picture that already calls the
    // track hostile or friendly disagrees with it, and the detector says so.
    let disagrees = matches!(class, Classification::Hostile | Classification::Friendly);
    state.cooperative.by_track.insert(
        track,
        LastCooperative {
            mmsi: report.mmsi,
            at: report.receipt_time,
            separation_m,
            disagrees,
            platform_class: report
                .ship_type
                .and_then(platform_class_of_ship_type)
                .map(ToOwned::to_owned),
        },
    );
    if state.cooperative.submitted.insert((track, report.mmsi)) {
        let label = match &report.name {
            Some(name) => format!("AIS MMSI {} ({name})", report.mmsi),
            None => format!("AIS MMSI {}", report.mmsi),
        };
        state
            .identification
            .submit_evidence(IdentificationEvidence {
                track_id: track,
                source: label,
                suggested: Classification::Neutral,
                confidence: COOPERATIVE_CONFIDENCE,
            });
    }
}

/// PN-04's evidence lines for a track, owned so the view can borrow them.
#[must_use]
pub fn evidence_lines(state: &AppState, track: TrackId) -> Vec<OwnedEvidence> {
    state
        .identification
        .evidence_for(track)
        .iter()
        .map(|e| OwnedEvidence {
            kind: if e.source.starts_with("AIS") {
                "cooperative identity (AIS)"
            } else {
                "identity evidence"
            },
            source: e.source.clone(),
            weight: e.confidence,
            supports: e.suggested,
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct OwnedEvidence {
    pub kind: &'static str,
    pub source: String,
    pub weight: f32,
    pub supports: Classification,
}

/// What the engine concludes for a track, in a sentence for PN-04.
#[must_use]
pub fn decision_sentence(state: &AppState, track: TrackId) -> String {
    match state.identification.decide(track) {
        IdentificationDecision::Declared(c) => format!("identification declares {c:?}"),
        IdentificationDecision::NeedsOperator { class, confidence } => format!(
            "identification leans {class:?} at {confidence:.2} and needs an operator to confirm"
        ),
        IdentificationDecision::Unknown { reason } => format!("no identification: {reason}"),
    }
}

/// PN-09's line per AIS feed.
#[must_use]
pub fn feed_lines(
    state: &AppState,
) -> Vec<gungnir_ui::panels::sensor_health::CooperativeFeedLine<'_>> {
    state
        .ais_stats
        .iter()
        .map(|(name, sink)| {
            let s = sink.lock().map(|s| *s).unwrap_or_default();
            gungnir_ui::panels::sensor_health::CooperativeFeedLine {
                name,
                sentences: s.lines,
                positions: s.positions,
                static_reports: s.static_reports,
                not_decoded: s.undecodable + s.unsupported,
            }
        })
        .collect()
}

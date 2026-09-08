// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Cooperative identity from ADS-B on the desktop (GAP-010), on the same shape
//! [`crate::cooperative`] already established for AIS: the feeds the baseline names,
//! their reports associated with tracks, and the identification engine fed from them.
//!
//! **A cooperative report is a claim.** A transponder says who it is on a channel
//! anybody can transmit on -- more so than AIS, since ADS-B carries no authentication at
//! all (`gungnir_interop::adsb`'s `SchemaKind::Adsb1090Es` names this in its own
//! `normative_source_pinned: false`) -- so the evidence submitted for it argues for
//! `Neutral` at a fixed, modest confidence and never declares on its own, exactly as
//! `cooperative.rs` does for AIS.
//!
//! Association is by proximity, same gate and same reasoning as AIS
//! ([`crate::cooperative::ASSOCIATION_GATE_M`]). This module does not duplicate a
//! platform-class lethality mapping the way AIS's ship-type table does (GAP-027): no
//! mission capability names one for aircraft categories, and inventing a mapping table
//! nobody asked for is not this change's decision to make.

use std::time::Duration;

use gungnir_config::{AdsbSource, ConfigBaseline};
use gungnir_identification::{IdentificationEngine, IdentificationEvidence};
use gungnir_ingest::adapters::adsb::{
    AdsbAdapter, AdsbStatsSink, AvrSource, CooperativeReport, CooperativeSink, RecordedAvrSource,
    TcpAvrSource,
};
use gungnir_ingest::IngestGateway;
use gungnir_model::{Classification, Geodetic, LocalFrame, SensorId, TrackId};

use crate::state::AppState;

/// What the desktop keeps of its bound ADS-B feeds.
#[derive(Debug, Default)]
pub struct BoundAdsbFeeds {
    pub reports: Vec<CooperativeSink>,
    pub stats: Vec<(String, AdsbStatsSink)>,
}

fn open_source(source: &AdsbSource) -> Result<Box<dyn AvrSource>, String> {
    match source {
        AdsbSource::Tcp { addr } => {
            let addr = addr
                .parse()
                .map_err(|e| format!("{addr}: not a socket address ({e})"))?;
            TcpAvrSource::connect(addr, Duration::from_secs(3))
                .map(|s| Box::new(s) as Box<dyn AvrSource>)
                .map_err(|e| e.to_string())
        }
        AdsbSource::File { path } => RecordedAvrSource::open(std::path::Path::new(path))
            .map(|s| Box::new(s.with_lines_per_poll(256)) as Box<dyn AvrSource>)
            .map_err(|e| e.to_string()),
    }
}

/// Bind every configured ADS-B feed into the gateway. A feed that cannot be opened is
/// an alert, not a silent absence; a deployment with no local frame binds nothing and
/// says why. Same shape as [`crate::cooperative::bind_feeds`].
pub fn bind_feeds(
    config: &ConfigBaseline,
    gateway: &mut IngestGateway,
    alerts: &mut Vec<String>,
) -> BoundAdsbFeeds {
    let mut bound = BoundAdsbFeeds::default();
    if config.adsb_feeds.is_empty() {
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
            "{} ADS-B feed(s) configured and no local frame origin declared: no receiver \
             is bound, because a position cannot be placed without one",
            config.adsb_feeds.len()
        ));
        return bound;
    };
    for feed in &config.adsb_feeds {
        match open_source(&feed.source) {
            Ok(source) => {
                let reports = CooperativeSink::default();
                let stats = AdsbStatsSink::default();
                let adapter =
                    AdsbAdapter::new(feed.name.clone(), SensorId(feed.sensor_id), frame, source)
                        .with_report_sink(reports.clone())
                        .with_stats_sink(stats.clone());
                gateway.add_adapter(Box::new(adapter));
                bound.reports.push(reports);
                bound.stats.push((feed.name.clone(), stats));
            }
            Err(err) => alerts.push(format!("ADS-B feed {} not bound: {err}", feed.name)),
        }
    }
    bound
}

/// The tick step: associate every report with a track and feed the engine. Same shape
/// as [`crate::cooperative::tick`].
pub fn tick(state: &mut AppState) {
    if state.adsb_sinks.is_empty() {
        return;
    }
    let mut drained = Vec::new();
    for sink in &state.adsb_sinks {
        if let Ok(mut q) = sink.lock() {
            drained.extend(q.drain(..));
        }
    }
    if drained.is_empty() {
        return;
    }
    let tracks: Vec<(TrackId, [f64; 3])> = state
        .tracking
        .tracks()
        .iter()
        .map(|t| (t.id, t.position_enu()))
        .collect();
    for report in drained {
        associate(state, &tracks, &report);
    }
}

fn associate(state: &mut AppState, tracks: &[(TrackId, [f64; 3])], report: &CooperativeReport) {
    let Some(pos) = report.position_enu else {
        return;
    };
    let nearest = tracks
        .iter()
        .map(|(id, p)| {
            let d = ((p[0] - pos[0]).powi(2) + (p[1] - pos[1]).powi(2)).sqrt();
            (*id, d)
        })
        .filter(|(_, d)| *d <= crate::cooperative::ASSOCIATION_GATE_M)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let Some((track, _separation_m)) = nearest else {
        return;
    };
    if state.adsb_submitted.insert((track, report.address.0)) {
        let label = match &report.callsign {
            Some(callsign) => format!("ADS-B {:06X} ({})", report.address.0, callsign.trim()),
            None => format!("ADS-B {:06X}", report.address.0),
        };
        state
            .identification
            .submit_evidence(IdentificationEvidence {
                track_id: track,
                source: label,
                suggested: Classification::Neutral,
                confidence: crate::cooperative::COOPERATIVE_CONFIDENCE,
            });
    }
}

/// PN-09's line per ADS-B feed. Same shape as [`crate::cooperative::feed_lines`].
#[must_use]
pub fn feed_lines(
    state: &AppState,
) -> Vec<gungnir_ui::panels::sensor_health::CooperativeFeedLine<'_>> {
    state
        .adsb_stats
        .iter()
        .map(|(name, sink)| {
            let s = sink.lock().map(|s| *s).unwrap_or_default();
            gungnir_ui::panels::sensor_health::CooperativeFeedLine {
                name,
                sentences: s.lines,
                positions: s.positions,
                static_reports: s.identifications,
                not_decoded: s.pair_failed,
            }
        })
        .collect()
}

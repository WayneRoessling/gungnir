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
//! ([`crate::cooperative::ASSOCIATION_GATE_M`]).
//!
//! **A platform-class mapping, narrower than AIS's (GAP-027).** This module used to
//! decline one outright: "no mission capability names one for aircraft categories."
//! That overstated it -- `docs/mission/air-defense-and-counter-uas.md` §2 names
//! "Crewed aircraft and helicopters" as its own threat class and says, in the same
//! row, "Identification is critical: friendly aircraft share the space." A
//! cooperatively squawking ADS-B contact answers exactly that question. What the
//! mission does *not* name is a way to tell a large transport from a fighter from a
//! rotorcraft by lethality -- so unlike AIS's `surface.*` table, which spans the full
//! ITU-R M.1371-6 ship-type range, [`platform_class_of_category`] collapses every
//! crewed-aircraft subcategory (DO-260B category set A, codes 1-7) into the one class
//! the mission actually distinguishes: `air.crewed`. Set A code 0 ("no category
//! information") and every other category set stay unmapped, `None`, rather than a
//! guess dressed as a class.

use std::collections::HashMap;
use std::time::Duration;

use gungnir_config::{AdsbSource, ConfigBaseline};
use gungnir_identification::{IdentificationEngine, IdentificationEvidence};
use gungnir_ingest::adapters::adsb::{
    AdsbAdapter, AdsbStatsSink, AvrSource, CooperativeReport, CooperativeSink, RecordedAvrSource,
    TcpAvrSource,
};
use gungnir_ingest::IngestGateway;
use gungnir_model::{Classification, Geodetic, LocalFrame, MissionTime, SensorId, TrackId};

use crate::state::AppState;

/// What the desktop keeps of its bound ADS-B feeds.
#[derive(Debug, Default)]
pub struct BoundAdsbFeeds {
    pub reports: Vec<CooperativeSink>,
    pub stats: Vec<(String, AdsbStatsSink)>,
}

/// The last cooperative report associated with a track, for the platform class it
/// declares. Narrower than AIS's `LastCooperative`: no disagreement check is built
/// here, because nothing asked for one and inventing it would be its own decision.
#[derive(Debug, Clone, PartialEq)]
pub struct LastAdsbCooperative {
    pub address: u32,
    pub at: MissionTime,
    /// The platform class the report declares (GAP-027), from the ADS-B category, when
    /// an identification message carried one.
    pub platform_class: Option<String>,
}

/// The platform class an ADS-B category declares, as an `air.*` class id the
/// baseline's lethality table can name. Only category set A's crewed-aircraft codes
/// (DO-260B, 1-7) are mapped, collapsed to one class rather than split by size or
/// performance, because that is the one distinction
/// `docs/mission/air-defense-and-counter-uas.md` §2 draws for cooperative aircraft
/// ("Crewed aircraft and helicopters"). Code 0 ("no category information") and every
/// other category set yield `None` rather than a guess.
#[must_use]
pub fn platform_class_of_category(category_set: char, category_code: u8) -> Option<&'static str> {
    match (category_set, category_code) {
        ('A', 1..=7) => Some("air.crewed"),
        _ => None,
    }
}

/// The class lethality per track for the assessor (GAP-027), the ADS-B half of
/// [`crate::cooperative::class_weights`]: the declared class looked up in the
/// baseline's table. A track with no declared class, or a class the table does not
/// list, is absent and weighs 1.0 there.
#[must_use]
pub fn class_weights(state: &AppState) -> HashMap<TrackId, f64> {
    state
        .adsb_cooperative
        .iter()
        .filter_map(|(track, last)| {
            let class = last.platform_class.as_deref()?;
            let weight = state.config.assessment.lethality_by_class.get(class)?;
            Some((*track, *weight))
        })
        .collect()
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
    state.adsb_cooperative.insert(
        track,
        LastAdsbCooperative {
            address: report.address.0,
            at: report.receipt_time,
            platform_class: report
                .category_set
                .zip(report.category_code)
                .and_then(|(set, code)| platform_class_of_category(set, code))
                .map(ToOwned::to_owned),
        },
    );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_crewed_aircraft_subcategory_under_set_a_maps_to_one_class() {
        for code in 1..=7u8 {
            assert_eq!(
                platform_class_of_category('A', code),
                Some("air.crewed"),
                "category A{code}"
            );
        }
    }

    #[test]
    fn no_category_information_and_every_other_set_are_unmapped() {
        assert_eq!(
            platform_class_of_category('A', 0),
            None,
            "0 is \"no category information\", not a class"
        );
        for set in ['B', 'C', 'D'] {
            for code in 0..=7u8 {
                assert_eq!(
                    platform_class_of_category(set, code),
                    None,
                    "the mission names no class for set {set}"
                );
            }
        }
    }
}

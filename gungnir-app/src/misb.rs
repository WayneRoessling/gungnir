// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! MISB ST 0601 UAS metadata feeds on the desktop (GAP-099): the `misb_feeds` the
//! baseline names, bound into the gateway at start exactly as every other live feed
//! type is -- the platform's own position becomes a `DetectionView` and is tracked,
//! the same as AIS's, ADS-B's, and every other feed's position.
//!
//! **Narrower than [`crate::cooperative`] and [`crate::adsb`], and deliberately so.**
//! AIS and ADS-B each attach a report sink, drain it every tick
//! (`crate::cooperative::tick`, `crate::adsb::tick`), associate each report with the
//! nearest track, and submit identification evidence -- all of which GAP-010's own
//! closing action named. GAP-099's own closing action names exactly two remaining
//! pieces (`docs/mission/gap-analysis/tools/gen_gaps.py`, `gap(id="GAP-099", ...)`):
//! wiring, and confirming this decoder's tag table against MISB's own primary text.
//! Evidence fusion over `UasPlatformReport` is not one of them, so it is not built
//! here. Concretely: this module attaches
//! `UasMetadataAdapter::with_stats_sink` (a small `Copy` struct overwritten in place
//! every poll, not a queue -- safe to leave unread) so a future PN-09 row has
//! something to draw, but it never calls `with_report_sink`. `PlatformReportSink` is
//! an unbounded queue; attaching one with nothing to drain it would grow for as long
//! as a live feed kept producing frames, which is exactly the kind of silent defect
//! this workspace's health flags exist to refuse rather than the honest "not built
//! yet" this module doc comment states instead. A future change that consumes
//! `UasPlatformReport` attaches `with_report_sink` and drains it every tick, the same
//! shape [`crate::cooperative::tick`] already establishes for AIS.
//!
//! `gungnir-node` binds the same adapter the same way, attaching neither sink at all
//! (`gungnir-node/src/main.rs::bind_misb_feeds`), for the same reason its own AIS and
//! ADS-B bindings attach neither: that binary has no edge to `gungnir-identification`
//! in the first place.

use std::time::Duration;

use gungnir_config::{ConfigBaseline, MisbSource};
use gungnir_ingest::adapters::misb::{
    KlvSource, MisbStatsSink, RecordedKlvSource, TcpKlvSource, UasMetadataAdapter,
};
use gungnir_ingest::IngestGateway;
use gungnir_model::{Geodetic, LocalFrame, SensorId};

/// What the desktop keeps of its bound MISB feeds: each one's counters, by name, for
/// a future PN-09 row. See the module doc comment for why there is no report sink
/// here, unlike [`crate::cooperative::BoundAisFeeds`] and
/// [`crate::adsb::BoundAdsbFeeds`].
#[derive(Debug, Default)]
pub struct BoundMisbFeeds {
    pub stats: Vec<(String, MisbStatsSink)>,
}

fn open_source(source: &MisbSource) -> Result<Box<dyn KlvSource>, String> {
    match source {
        MisbSource::Tcp { addr } => {
            let addr = addr
                .parse()
                .map_err(|e| format!("{addr}: not a socket address ({e})"))?;
            TcpKlvSource::connect(addr, Duration::from_secs(3))
                .map(|s| Box::new(s) as Box<dyn KlvSource>)
                .map_err(|e| e.to_string())
        }
        MisbSource::File { path } => RecordedKlvSource::open(std::path::Path::new(path))
            .map(|s| Box::new(s) as Box<dyn KlvSource>)
            .map_err(|e| e.to_string()),
    }
}

/// Bind every configured MISB feed into the gateway. A feed that cannot be opened is
/// an alert, not a silent absence; a deployment with no local frame binds nothing and
/// says why. Same shape as [`crate::adsb::bind_feeds`]; see the module doc comment for
/// why no report sink is attached.
pub fn bind_feeds(
    config: &ConfigBaseline,
    gateway: &mut IngestGateway,
    alerts: &mut Vec<String>,
) -> BoundMisbFeeds {
    let mut bound = BoundMisbFeeds::default();
    if config.misb_feeds.is_empty() {
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
            "{} MISB feed(s) configured and no local frame origin declared: no receiver \
             is bound, because a position cannot be placed without one",
            config.misb_feeds.len()
        ));
        return bound;
    };
    for feed in &config.misb_feeds {
        match open_source(&feed.source) {
            Ok(source) => {
                let stats = MisbStatsSink::default();
                let adapter = UasMetadataAdapter::new(
                    feed.name.clone(),
                    SensorId(feed.sensor_id),
                    frame,
                    source,
                )
                .with_stats_sink(stats.clone());
                gateway.add_adapter(Box::new(adapter));
                bound.stats.push((feed.name.clone(), stats));
            }
            Err(err) => alerts.push(format!("MISB feed {} not bound: {err}", feed.name)),
        }
    }
    bound
}

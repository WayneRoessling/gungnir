// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The SAPIENT feeds on the desktop (GAP-001): bound from the baseline at start, each
//! reporting detections of what it sees, not a cooperative claim about itself.
//!
//! **Not cooperative identity.** AIS and ADS-B transponders report their own position
//! (`crate::cooperative`, `crate::adsb`); a SAPIENT node is a spotter, an acoustic
//! array, or a passive-RF direction finder reporting a *detection of something else*, so
//! its output is a [`gungnir_ingest::DetectionView`] into the same gateway a radar feeds,
//! not an identity claim to associate with a track. This module is therefore shaped like
//! [`crate::radar`] (bind and drain a stats sink) rather than like
//! [`crate::cooperative`] (bind, associate, and submit identification evidence): there
//! is no per-frame tick here, because the gateway already polls every bound adapter.

use std::time::Duration;

use gungnir_config::{ConfigBaseline, SapientNodeType, SapientSource};
use gungnir_ingest::adapters::sapient::{
    RecordedSapientSource, SapientDetectionAdapter, SapientFeedStatsSink,
    SapientSource as AdapterSource, TcpSapientSource, ACOUSTIC_NODE_TYPE, PASSIVE_RF_NODE_TYPE,
    SPOTTER_NODE_TYPE,
};
use gungnir_ingest::IngestGateway;
use gungnir_model::{Geodetic, LocalFrame, SensorId};

fn accepted_node_type(kind: SapientNodeType) -> &'static str {
    match kind {
        SapientNodeType::Spotter => SPOTTER_NODE_TYPE,
        SapientNodeType::Acoustic => ACOUSTIC_NODE_TYPE,
        SapientNodeType::PassiveRf => PASSIVE_RF_NODE_TYPE,
    }
}

fn open_source(source: &SapientSource) -> Result<Box<dyn AdapterSource>, String> {
    match source {
        SapientSource::Tcp { addr } => {
            let addr = addr
                .parse()
                .map_err(|e| format!("{addr}: not a socket address ({e})"))?;
            TcpSapientSource::connect(addr, Duration::from_secs(3))
                .map(|s| Box::new(s) as Box<dyn AdapterSource>)
                .map_err(|e| e.to_string())
        }
        SapientSource::File { path } => RecordedSapientSource::open(std::path::Path::new(path))
            .map(|s| Box::new(s) as Box<dyn AdapterSource>)
            .map_err(|e| e.to_string()),
    }
}

/// What the desktop keeps of its bound SAPIENT feeds, for PN-09. Same shape as
/// [`crate::radar::BoundFeeds`].
#[derive(Debug, Default)]
pub struct BoundSapientFeeds {
    pub stats: Vec<(String, SapientFeedStatsSink)>,
}

/// Bind every configured SAPIENT feed into the gateway. A feed that cannot be opened is
/// an alert, not a silent absence; a deployment with no local frame binds nothing and
/// says why, exactly as [`crate::cooperative::bind_feeds`] and
/// [`crate::adsb::bind_feeds`] do.
pub fn bind_feeds(
    config: &ConfigBaseline,
    gateway: &mut IngestGateway,
    alerts: &mut Vec<String>,
) -> BoundSapientFeeds {
    let mut bound = BoundSapientFeeds::default();
    if config.sapient_feeds.is_empty() {
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
            "{} SAPIENT feed(s) configured and no local frame origin declared: no node is \
             bound, because a bearing's origin cannot be placed without one",
            config.sapient_feeds.len()
        ));
        return bound;
    };
    for feed in &config.sapient_feeds {
        // The node's own siting (SensorConfig::position), the same lookup radar.rs
        // makes for a radar's position.
        let Some(sensor) = config.sensors.iter().find(|s| s.id == feed.sensor_id) else {
            // Unreachable once `validate` has run (a SAPIENT feed's sensor is checked
            // there), kept as a named alert rather than a panic for a caller that binds
            // an unvalidated baseline.
            alerts.push(format!(
                "SAPIENT feed {} names sensor {}, which is not in the sensor list",
                feed.name, feed.sensor_id
            ));
            continue;
        };
        let observer_enu = frame.to_enu(Geodetic {
            lat_rad: sensor.position[0],
            lon_rad: sensor.position[1],
            alt_m: sensor.position[2],
        });
        match open_source(&feed.source) {
            Ok(source) => {
                let stats = SapientFeedStatsSink::default();
                let adapter = SapientDetectionAdapter::new(
                    feed.name.clone(),
                    SensorId(feed.sensor_id),
                    frame,
                    observer_enu,
                    source,
                    accepted_node_type(feed.node_type),
                )
                .with_stats_sink(stats.clone());
                gateway.add_adapter(Box::new(adapter));
                bound.stats.push((feed.name.clone(), stats));
            }
            Err(err) => alerts.push(format!("SAPIENT feed {} not bound: {err}", feed.name)),
        }
    }
    bound
}

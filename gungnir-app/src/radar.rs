// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The radar feeds on the desktop (GAP-001, GAP-064): bound from the baseline at start,
//! their service messages drained into the sensor registry every frame.
//!
//! A radar speaking for itself is the confirmation DN-11 §5 rule 1 asks for before the
//! registry may report a mode, so a north marker from a sensor at Standby moves it to
//! Search and the change goes on the record like an operator's would.

use gungnir_config::ConfigBaseline;
use gungnir_ingest::adapters::asterix::{
    bind_feed, FeedSinks, FeedSpec, FeedStatsSink, RadarBinding, ServiceObservationKind,
    ServiceObservationSink,
};
use gungnir_ingest::IngestGateway;
use gungnir_model::{Geodetic, LocalFrame, SensorId};
use gungnir_sensor_management::ServiceObservation;

use crate::state::AppState;

/// The feeds as the ingest crate builds them, from a validated baseline.
#[must_use]
pub fn feed_specs(config: &ConfigBaseline) -> Vec<FeedSpec> {
    config
        .radar_feeds
        .iter()
        .filter_map(|f| {
            let bind_addr = f.bind_addr.parse().ok()?;
            let multicast = match &f.multicast {
                Some(m) => Some((m.group.parse().ok()?, m.interface.parse().ok()?)),
                None => None,
            };
            let radars = f
                .radars
                .iter()
                .filter_map(|r| {
                    let sensor = config.sensors.iter().find(|s| s.id == r.sensor_id)?;
                    Some(RadarBinding {
                        sac: r.sac,
                        sic: r.sic,
                        sensor: SensorId(r.sensor_id),
                        position: Geodetic {
                            lat_rad: sensor.position[0],
                            lon_rad: sensor.position[1],
                            alt_m: sensor.position[2],
                        },
                    })
                })
                .collect();
            Some(FeedSpec {
                name: f.name.clone(),
                bind_addr,
                multicast,
                radars,
            })
        })
        .collect()
}

/// What the desktop keeps of its bound feeds (GAP-001, GAP-064).
#[derive(Debug, Default)]
pub struct BoundFeeds {
    pub observations: Vec<ServiceObservationSink>,
    /// Each feed by name with its counters, for PN-09.
    pub stats: Vec<(String, FeedStatsSink)>,
}

/// Bind every configured feed into the gateway. A feed that cannot be bound is an alert,
/// not a silent absence; a deployment with no local frame binds nothing and says why.
pub fn bind_feeds(
    config: &ConfigBaseline,
    gateway: &mut IngestGateway,
    alerts: &mut Vec<String>,
) -> BoundFeeds {
    let mut sinks = BoundFeeds::default();
    if config.radar_feeds.is_empty() {
        return sinks;
    }
    let Some(frame) = config.origin.map(|[lat_rad, lon_rad, alt_m]| {
        LocalFrame::new(Geodetic {
            lat_rad,
            lon_rad,
            alt_m,
        })
    }) else {
        alerts.push(format!(
            "{} radar feed(s) configured and no local frame origin declared: no adapter \
             is bound, because a plot cannot be placed without one",
            config.radar_feeds.len()
        ));
        return sinks;
    };
    for spec in feed_specs(config) {
        let feed_sinks = FeedSinks::default();
        match bind_feed(&spec, &frame, &feed_sinks) {
            Ok(adapter) => {
                gateway.add_adapter(Box::new(adapter));
                gateway.set_expected_adapters(config.sensors.len() + sinks.observations.len() + 1);
                sinks.observations.push(feed_sinks.observations);
                sinks.stats.push((spec.name.clone(), feed_sinks.stats));
            }
            Err(err) => alerts.push(format!("radar feed {} not bound: {err}", spec.name)),
        }
    }
    sinks
}

/// PN-09's line per feed (GAP-001): what arrived and what did not decode, so a feed
/// that is bound and silent is told apart from one that is talking and unreadable.
#[must_use]
pub fn feed_lines(state: &AppState) -> Vec<gungnir_ui::panels::sensor_health::FeedLine<'_>> {
    state
        .feed_stats
        .iter()
        .map(|(name, sink)| {
            let s = sink.lock().map(|s| *s).unwrap_or_default();
            gungnir_ui::panels::sensor_health::FeedLine {
                name,
                datagrams: s.datagrams,
                detections: s.detections,
                service_reports: s.service_reports,
                not_decoded: s.malformed_datagrams
                    + s.malformed_blocks
                    + s.unsupported_category_blocks,
                unknown_radar: s.unknown_radar,
            }
        })
        .collect()
}

/// The tick step: what the radars said about themselves, into the registry and, where a
/// mode was confirmed, onto the record.
pub fn observe_services(state: &mut AppState) {
    if state.service_sinks.is_empty() {
        return;
    }
    let now = state.clock.now();
    let mut drained = Vec::new();
    for sink in &state.service_sinks {
        if let Ok(mut q) = sink.lock() {
            drained.extend(q.drain(..));
        }
    }
    for o in drained {
        let observations = [
            match o.kind {
                ServiceObservationKind::NorthMarker => Some(ServiceObservation::NorthMarker {
                    rotation_period_s: o.rotation_period_s,
                }),
                ServiceObservationKind::SectorCrossing => Some(ServiceObservation::SectorCrossing),
                ServiceObservationKind::Other => None,
            },
            match (
                o.released_for_operational_use,
                o.overloaded,
                o.time_source_invalid,
            ) {
                (Some(released), Some(overloaded), Some(time_source_invalid)) => {
                    Some(ServiceObservation::Status {
                        released_for_operational_use: released,
                        overloaded,
                        time_source_invalid,
                    })
                }
                _ => None,
            },
        ];
        for observation in observations.into_iter().flatten() {
            match state.sensors.observe_service(o.sensor, observation, now) {
                Ok(Some(from)) => {
                    let event = gungnir_model::events::SensorEvent::ModeChanged {
                        sensor: o.sensor,
                        from,
                        to: gungnir_model::SensorMode::Search,
                        at: now,
                    };
                    crate::update::publish(state, now, gungnir_eventing::Event::Sensor(event));
                }
                Ok(None) => {}
                Err(err) => {
                    tracing::warn!(sensor = o.sensor.0, %err, "service message from an unknown sensor");
                }
            }
        }
    }
}

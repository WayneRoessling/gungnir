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
    bind_feed, DfBinding, FeedSinks, FeedSpec, FeedStatsSink, RadarBinding, ServiceObservationKind,
    ServiceObservationSink,
};
use gungnir_ingest::IngestGateway;
use gungnir_model::{Geodetic, LocalFrame, SensorId};
use gungnir_sensor_management::ServiceObservation;

use crate::state::AppState;

/// The feeds as the ingest crate builds them, from a validated baseline. `df_sites`
/// (GAP-100) is built the same way `radars` is: a sensor named by id in the baseline's
/// own sensor list supplies the position, so a direction finder no sensor names is
/// silently absent here the same way a radar with the same problem already is --
/// `gungnir_config::validate` refuses that baseline before it would ever reach this.
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
            let df_sites = f
                .df_sites
                .iter()
                .filter_map(|d| {
                    let sensor = config.sensors.iter().find(|s| s.id == d.sensor_id)?;
                    Some(DfBinding {
                        sac: d.sac,
                        sic: d.sic,
                        sensor: SensorId(d.sensor_id),
                        position: Geodetic {
                            lat_rad: sensor.position[0],
                            lon_rad: sensor.position[1],
                            alt_m: sensor.position[2],
                        },
                        azimuth_sigma_rad: d.azimuth_sigma_rad,
                    })
                })
                .collect();
            Some(FeedSpec {
                name: f.name.clone(),
                bind_addr,
                multicast,
                radars,
                df_sites,
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

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_config::{DfSiteConfig, RadarFeedConfig, SensorConfig};
    use gungnir_ingest::AllowListAuthenticator;

    fn df_sensor(id: u32) -> SensorConfig {
        SensorConfig {
            id,
            modality: "df".into(),
            position: [0.9, 0.2, 30.0],
            max_range_m: 5_000.0,
            control_endpoint: None,
            maintenance: Vec::new(),
        }
    }

    fn df_site(sensor_id: u32) -> DfSiteConfig {
        DfSiteConfig {
            sensor_id,
            sac: 50,
            sic: 6,
            azimuth_sigma_rad: 1.5_f64.to_radians(),
        }
    }

    /// GAP-100: a direction finder named in `RadarFeedConfig::df_sites` reaches
    /// `FeedSpec::df_sites` as the exact `DfBinding` `bind_feed` will hand to
    /// `AsterixFeedAdapter::with_df_sites` -- SAC/SIC and accuracy carried straight
    /// through, position resolved from the sensor list the same way a radar's already
    /// is. A feed naming only a direction finder and no radar still produces one spec.
    #[test]
    fn feed_specs_carries_a_configured_direction_finder_into_the_binding() {
        let config = ConfigBaseline {
            sensors: vec![df_sensor(21)],
            radar_feeds: vec![RadarFeedConfig {
                name: "north".into(),
                bind_addr: "0.0.0.0:8600".into(),
                multicast: None,
                radars: Vec::new(),
                df_sites: vec![df_site(21)],
            }],
            ..ConfigBaseline::default()
        };
        let specs = feed_specs(&config);
        assert_eq!(specs.len(), 1);
        assert!(specs[0].radars.is_empty());
        assert_eq!(
            specs[0].df_sites,
            vec![DfBinding {
                sac: 50,
                sic: 6,
                sensor: SensorId(21),
                position: Geodetic {
                    lat_rad: 0.9,
                    lon_rad: 0.2,
                    alt_m: 30.0,
                },
                azimuth_sigma_rad: 1.5_f64.to_radians(),
            }]
        );
    }

    /// GAP-100, "reachable at start-up": the desktop's real `bind_feeds` -- the
    /// function `AppState` calls when it stands the ingest gateway up -- binds a feed
    /// that names only a direction finder without an alert and registers it, exactly
    /// as it already does for a radar-only feed. `127.0.0.1:0` is a real loopback bind
    /// (OS-assigned port), not a stub, so this proves the construction path itself
    /// runs; `bind_feed_wires_a_configured_direction_finder_into_the_live_adapter` in
    /// `gungnir-ingest` proves what that path builds actually attributes a bearing.
    #[test]
    fn bind_feeds_reaches_a_configured_direction_finder_at_start_up() {
        let config = ConfigBaseline {
            sensors: vec![df_sensor(21)],
            origin: Some([0.9, 0.2, 0.0]),
            radar_feeds: vec![RadarFeedConfig {
                name: "north".into(),
                bind_addr: "127.0.0.1:0".into(),
                multicast: None,
                radars: Vec::new(),
                df_sites: vec![df_site(21)],
            }],
            ..ConfigBaseline::default()
        };
        let mut gateway = IngestGateway::new(Box::new(AllowListAuthenticator {
            allowed: vec![SensorId(21)],
        }));
        let mut alerts = Vec::new();
        let bound = bind_feeds(&config, &mut gateway, &mut alerts);
        assert!(alerts.is_empty(), "unexpected alerts: {alerts:?}");
        assert_eq!(
            bound.stats.len(),
            1,
            "the one feed, bound for its direction finder alone"
        );
    }
}

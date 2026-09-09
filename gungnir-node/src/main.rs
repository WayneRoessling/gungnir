// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! gungnir-node: the headless service node (ARCHITECTURE.md §8). Runs the same
//! services layer the desktop embeds, journals every event as the authoritative
//! record for the mission, and exposes `gungnir-api` to desktops and peer systems.
//!
//! Usage: `gungnir-node [config.json]`. Without a config path the default baseline
//! is used (no sensors, no resources), which is enough to prove the loop runs.
//!
//! **Status, 2026-09-06:** the tracking pipeline, the allocator and the v2 transport are
//! all real. This node tracks, plans, and serves the v2 read paths plus four write paths
//! -- submitting a detection, tasking a sensor, reporting on a handoff, acknowledging a
//! warning -- each authorising its caller. This paragraph said the opposite until today,
//! and understating what a deployment does is read as carelessly as overstating it.
//!
//! What it still does not do: run an approval queue, which is a desktop's job and which
//! `POST /v2/plans/{id}/decision` refuses architecturally rather than for want of a
//! feature; fuse cooperative evidence or correlate identity across sessions, for which it
//! has no dependency edge (GAP-010, GAP-019); and produce any exchange product, so the
//! three `/v2/exchange` routes answer `NotHeld` with a reason (GAP-065).

use std::net::SocketAddr;
use std::sync::Arc;

mod account;
mod auth;
mod entities;

use gungnir_api::transport::NodeApi;
use gungnir_api::v2::{CoverageResponse, SnapshotResponse};
use gungnir_api::API_VERSION;
use gungnir_config::{validate, ConfigBaseline, ConfigStore, FileConfigStore, NodeConfig};
use gungnir_eventing::{Event, EventBus, InProcessBus};
use gungnir_ingest::adapters::peer::{LaunchWarningOutcome, LaunchWarningSink};
use gungnir_ingest::{AllowListAuthenticator, IngestGateway, MachineIdentityAuthenticator};
use gungnir_intercept_service::{DpInterceptService, InterceptService};
use gungnir_mission::{JournalMissionManager, MissionManager, MissionState};
use gungnir_model::events::InterceptEvent;
use gungnir_model::{PlanView, SensorId, SystemHealth};
use gungnir_observability::WatchdogConfig;
use gungnir_policy::{ControlStatusPolicy, GeofencePolicy, PolicyChain, PolicyEngine};
use gungnir_sensor_management::{InMemorySensorRegistry, SensorRegistry};
use gungnir_store::{EventJournal, FileEventJournal};
use gungnir_time::{TimeAuthority, WallClockAuthority};
use gungnir_tracking_service::{
    project_pipeline_stats, LiveTrackingService, PipelineStats, TrackingService,
};
use std::time::{Duration, Instant};

/// Service tick period.
const TICK: Duration = Duration::from_millis(50);
/// How often health is logged while running.
const HEALTH_LOG_INTERVAL: Duration = Duration::from_secs(10);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let args: Vec<String> = std::env::args().skip(1).collect();

    // `gungnir-node account ...` provisions the accounts this node authenticates
    // against (GAP-057). It is a separate path rather than a flag on the running node
    // because it must not start a server, open a journal, or bind a port: it reads one
    // file, writes it back, and exits.
    if args.first().map(String::as_str) == Some("account") {
        match account::run(&args[1..]) {
            Ok(said) => {
                println!("{said}");
                return Ok(());
            }
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(2);
            }
        }
    }

    let config = load_config(args.into_iter().next())?;
    let node_cfg = config.node.clone().unwrap_or_default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let handle = runtime.handle().clone();
    runtime.block_on(run(config, node_cfg, handle))
}

/// The sensor positions the tracker needs to place an angular report (GAP-001, DN-27 §4).
///
/// Built from the baseline's own sensor list, which is where a deployment states where
/// each sensor is. Without it every bearing and every range-azimuth-elevation report is
/// refused, which is what happened until 2026-09-07.
fn sensor_positions(config: &ConfigBaseline) -> gungnir_tracking_service::SensorPositions {
    gungnir_tracking_service::SensorPositions::from_sensors(
        config.sensors.iter().map(|s| (s.id, s.position)),
    )
}

fn load_config(path: Option<String>) -> Result<ConfigBaseline, gungnir_config::ConfigError> {
    let Some(p) = path else {
        tracing::info!("no config path given; using the default baseline");
        return Ok(ConfigBaseline::default());
    };
    // GAP-052 / DN-08 §6 rule 3: the canonical action and role names come from
    // `gungnir-security`, which `gungnir-config` may not depend on, so this binary hands
    // them over. Without it a rule naming a misspelled action passes every other check,
    // matches no request, and reads in the file exactly like a grant.
    let known = gungnir_config::KnownVocabulary::new(
        gungnir_security::actions::ALL.iter().copied(),
        gungnir_security::Role::ALL.iter().map(|r| format!("{r:?}")),
    );
    let store = FileConfigStore::new(&p, known.clone());
    let baseline = store.load()?;
    validate(&baseline)?;
    gungnir_config::validate_authority_names(&baseline, &known)?;
    tracing::info!(path = %p, "loaded config baseline");
    Ok(baseline)
}

/// The ingest gateway, allowing exactly the sensors the baseline names, with an ASTERIX
/// adapter bound per configured radar feed (GAP-001). Returns the observation sinks the
/// loop drains into the registry (GAP-064).
/// A partner's tracks and launch warnings through a machine link (GAP-009): the
/// adapter's stream over the link `gungnir-remote` keeps up.
///
/// Two calls rather than one (DN-16 §5): a launch warning is a distinct message and
/// cannot be handed over where a track is expected.
struct LinkPeerStream {
    link: gungnir_remote::peer::PeerLink,
}

impl gungnir_ingest::adapters::peer::PeerStream for LinkPeerStream {
    fn take_tracks(&mut self) -> Vec<gungnir_model::TrackView> {
        self.link.take_tracks()
    }

    fn take_launch_warnings(&mut self) -> Vec<gungnir_model::LaunchWarningReport> {
        self.link.take_launch_warnings()
    }

    fn describe(&self) -> String {
        format!("link:{}", self.link.endpoint())
    }
}

/// One bound peer: the link, and where its launch warnings land (GAP-009, DN-16 §5).
///
/// The sink is held here rather than reached through the adapter because the adapter is
/// moved into the gateway; it is the same arrangement the ASTERIX feeds use for their
/// service observations.
struct BoundPeer {
    name: String,
    link: gungnir_remote::peer::PeerLink,
    launch_warnings: LaunchWarningSink,
}

/// Put every peer's launch warnings on this node's record (GAP-009, DN-16 §5).
///
/// A node has no operator and no alert list, so DN-16 §5's "raises an alert with the
/// peer named" is served the way every other node-side alert is: the log line for a
/// person watching, and an envelope on the bus for the journal that is the system of
/// record for every desktop reading this node. **No track is created**: a
/// `LaunchWarningOutcome` never becomes a `DetectionView`, so there is nothing for the
/// pipeline to take.
fn record_launch_warnings(
    peers: &[BoundPeer],
    bus: &InProcessBus,
    now: gungnir_model::MissionTime,
) -> Result<(), gungnir_eventing::EventingError> {
    for peer in peers {
        // A poisoned sink means the adapter panicked holding it. Say so and carry on
        // with the other peers rather than stopping the node.
        let Ok(mut queue) = peer.launch_warnings.lock() else {
            tracing::error!(peer = %peer.name, "the peer's launch-warning queue was poisoned");
            continue;
        };
        let drained: Vec<LaunchWarningOutcome> = queue.drain(..).collect();
        drop(queue);
        for outcome in drained {
            let event = match outcome {
                LaunchWarningOutcome::Admitted(warning) => {
                    tracing::warn!(peer = %warning.peer, warning = %warning.report.id, age_s = warning.age_s(), what = %warning.report.what, "launch warning from a peer");
                    gungnir_model::events::LaunchWarningEvent::Received(warning)
                }
                LaunchWarningOutcome::Quarantined { peer, reason, at } => {
                    tracing::warn!(%peer, %reason, "a peer's launch warning was refused");
                    gungnir_model::events::LaunchWarningEvent::Refused { peer, reason, at }
                }
            };
            bus.publish(now, Event::LaunchWarning(event))?;
        }
    }
    Ok(())
}

/// What this host presents to a partner (D-02): an identity issued from its own key
/// provider, preferred, and the environment-supplied certificate as a fallback for
/// whichever half is missing (GAP-060).
///
/// **The provider is ephemeral, and deliberately still is (2026-09-08).** The
/// OS-keystore path GAP-057 admitted for this node's account store has since been
/// generalised to key material (GAP-060's remaining slice: `PersistentKeyProvider::
/// open_or_create_via_os_keystore` now takes `service` as a parameter), and
/// `spawn_tls_from_provider` uses that generalisation for the node's *serving*
/// identity. This function -- the node's own outbound, peer-link identity -- was left
/// untouched on purpose: whether it should persist too, and whether it should then be
/// the *same* identity as the serving one or a third, separately named entry, is real
/// design surface the register leaves open (ARCHITECTURE.md item 105), and closing it
/// silently by picking one here is not this change's to do. So this remains a new
/// identity every start, exactly as before.
fn host_tls(config: &ConfigBaseline) -> gungnir_remote::LinkTls {
    let identity_pem = match (
        std::env::var("GUNGNIR_TLS_CERT").ok(),
        std::env::var("GUNGNIR_TLS_KEY").ok(),
    ) {
        (Some(cert), Some(key)) => {
            match (
                std::fs::read_to_string(&cert),
                std::fs::read_to_string(&key),
            ) {
                (Ok(cert), Ok(key)) => Some(format!("{cert}\n{key}")),
                _ => None,
            }
        }
        _ => None,
    };
    // Issued wins over the PEM fallback (`LinkTls`'s own rule); reading both and letting
    // `client_config` choose is the same shape `gungnir-app`'s `link_tls_for` uses.
    let issued = gungnir_remote::identity::issue_for_client("gungnir-node")
        .map_err(|err| {
            tracing::warn!(%err, "this node could not issue its own peer-link identity");
        })
        .ok();
    gungnir_remote::LinkTls {
        trust_roots_pem: config.security.tls.trust_roots_pem.clone(),
        issued,
        identity_pem,
    }
}

/// Bind a peer link per configured peer whose endpoint is a node (GAP-009, GAP-065
/// inbound). A peer whose endpoint is not one is said and skipped.
fn bind_peers(
    config: &ConfigBaseline,
    gateway: &mut IngestGateway,
    handle: &tokio::runtime::Handle,
) -> Vec<BoundPeer> {
    let tls = host_tls(config);
    let mut bound = Vec::new();
    for peer in &config.peers {
        let Some(endpoint) = config.endpoints.iter().find(|e| e.name == peer.endpoint) else {
            continue;
        };
        if endpoint.kind != "peer" || !endpoint.address.starts_with("http") {
            tracing::warn!(peer = %peer.name, endpoint = %endpoint.name, kind = %endpoint.kind, "peer endpoint is not a node url; no link bound");
            continue;
        }
        let remote = gungnir_remote::RemoteEndpoint {
            url: endpoint.address.clone(),
            tls: tls.clone(),
        };
        match gungnir_remote::peer::PeerLink::connect(&remote, handle) {
            Ok(link) => {
                let launch_warnings = LaunchWarningSink::default();
                let adapter = gungnir_ingest::adapters::peer::PeerSourceAdapter::new(
                    peer.name.clone(),
                    SensorId(peer.source_id),
                    peer.assigned_quality,
                    peer.max_age_s,
                    LinkPeerStream { link: link.clone() },
                )
                .with_launch_warning_sink(launch_warnings.clone());
                gateway.add_adapter(Box::new(adapter));
                tracing::info!(peer = %peer.name, url = %endpoint.address, "peer link bound as a machine (DN-16); connected when the partner answers");
                bound.push(BoundPeer {
                    name: peer.name.clone(),
                    link,
                    launch_warnings,
                });
            }
            Err(err) => {
                tracing::error!(peer = %peer.name, %err, "peer link not bound");
            }
        }
    }
    bound
}

/// What the node reports about its feeds on the health line (GAP-001, step 5): each
/// radar feed's counters, and each peer link's state. A node has no panel; the log is
/// where an operator reads it.
struct FeedReports {
    radar: Vec<(String, gungnir_ingest::adapters::asterix::FeedStatsSink)>,
    peers: Vec<BoundPeer>,
}

impl FeedReports {
    fn summary(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .radar
            .iter()
            .map(|(name, sink)| {
                let s = sink.lock().map(|s| *s).unwrap_or_default();
                format!(
                    "{name}: {} datagrams, {} detections, {} service reports, {} not decoded",
                    s.datagrams,
                    s.detections,
                    s.service_reports,
                    s.malformed_datagrams + s.not_detections
                )
            })
            .collect();
        lines.extend(self.peers.iter().map(|peer| {
            let (name, link) = (&peer.name, &peer.link);
            if link.connected() {
                format!("peer {name}: linked to {}", link.endpoint())
            } else {
                format!(
                    "peer {name}: not linked to {} ({})",
                    link.endpoint(),
                    link.last_error().unwrap_or_else(|| "no answer yet".into())
                )
            }
        }));
        lines
    }
}

fn build_gateway(
    config: &ConfigBaseline,
    handle: &tokio::runtime::Handle,
) -> (
    IngestGateway,
    Vec<gungnir_ingest::adapters::asterix::ServiceObservationSink>,
    FeedReports,
    BoundSapientFeeds,
) {
    let mut gateway = IngestGateway::new(Box::new(AllowListAuthenticator {
        // DN-16 §5: a peer is a source and is admitted like one, under its own id.
        allowed: config
            .sensors
            .iter()
            .map(|s| SensorId(s.id))
            .chain(config.peers.iter().map(|p| SensorId(p.source_id)))
            .collect(),
    }));
    gateway.set_expected_adapters(config.sensors.len());
    let mut reports = FeedReports {
        radar: Vec::new(),
        peers: Vec::new(),
    };
    if !config.peers.is_empty() {
        reports.peers = bind_peers(config, &mut gateway, handle);
        tracing::info!(
            peers = config.peers.len(),
            bound = reports.peers.len(),
            "peer links"
        );
    }
    let mut sinks = Vec::new();
    if !config.radar_feeds.is_empty() {
        match local_frame(config) {
            None => tracing::warn!(
                feeds = config.radar_feeds.len(),
                "radar feeds are configured and no local frame origin is declared: no \
                 adapter is bound, because a plot cannot be placed without one"
            ),
            Some(frame) => {
                for spec in feed_specs(config) {
                    let feed_sinks = gungnir_ingest::adapters::asterix::FeedSinks::default();
                    match gungnir_ingest::adapters::asterix::bind_feed(&spec, &frame, &feed_sinks) {
                        Ok(adapter) => {
                            tracing::info!(feed = %spec.name, addr = %spec.bind_addr, radars = spec.radars.len(), df_sites = spec.df_sites.len(), uas_sites = spec.uas_sites.len(), "radar feed bound");
                            gateway.add_adapter(Box::new(adapter));
                            gateway.set_expected_adapters(config.sensors.len() + sinks.len() + 1);
                            // The node has no panel; its counters reach the log on the
                            // health line, and the sink keeps them readable there.
                            sinks.push(feed_sinks.observations);
                            reports.radar.push((spec.name.clone(), feed_sinks.stats));
                        }
                        Err(err) => {
                            tracing::error!(feed = %spec.name, %err, "radar feed not bound");
                        }
                    }
                }
            }
        }
    }
    // GAP-010: AIS receivers. The reports go to a sink nobody on the node fuses.
    //
    // **The reason changed on 2026-09-06 and the conclusion did not**, which is worth
    // saying because the old reason was that the node had no tracks to attach evidence
    // to: GAP-011 closed and it has tracks now. What stops it is that this binary has no
    // edge to `gungnir-identification`, the crate that fuses evidence -- edge (n) chose
    // the desktop, on an argument whose first half has since expired
    // (`docs/design/dependency-edges.md`). So it is a graph decision rather than a
    // missing capability. The detections still enter the gateway like any source, and the
    // sink is drained on the loop so it cannot grow without bound.
    if !config.ais_feeds.is_empty() {
        match local_frame(config) {
            None => tracing::warn!(
                feeds = config.ais_feeds.len(),
                "AIS feeds are configured and no local frame origin is declared: no receiver \
                 is bound"
            ),
            Some(frame) => {
                for feed in &config.ais_feeds {
                    let source: Result<Box<dyn gungnir_ingest::adapters::ais::NmeaSource>, String> =
                        match &feed.source {
                            gungnir_config::AisSource::Tcp { addr } => addr
                                .parse()
                                .map_err(|e| format!("{addr}: {e}"))
                                .and_then(|addr| {
                                    gungnir_ingest::adapters::ais::TcpNmeaSource::connect(
                                        addr,
                                        std::time::Duration::from_secs(3),
                                    )
                                    .map(|s| Box::new(s) as _)
                                    .map_err(|e| e.to_string())
                                }),
                            gungnir_config::AisSource::File { path } => {
                                gungnir_ingest::adapters::ais::RecordedNmeaSource::open(
                                    std::path::Path::new(path),
                                )
                                .map(|s| Box::new(s.with_lines_per_poll(64)) as _)
                                .map_err(|e| e.to_string())
                            }
                        };
                    match source {
                        Ok(source) => {
                            let adapter = gungnir_ingest::adapters::ais::AisReceiverAdapter::new(
                                feed.name.clone(),
                                SensorId(feed.sensor_id),
                                frame,
                                source,
                            );
                            tracing::info!(feed = %feed.name, sensor = feed.sensor_id, "AIS feed bound; detections are tracked, and the cooperative reports are not fused here because this binary has no edge to the crate that fuses evidence (GAP-010)");
                            gateway.add_adapter(Box::new(adapter));
                            gateway.set_expected_adapters(config.sensors.len() + sinks.len() + 1);
                        }
                        Err(err) => tracing::error!(feed = %feed.name, %err, "AIS feed not bound"),
                    }
                }
            }
        }
    }
    bind_adsb_feeds(config, &mut gateway, &sinks);
    bind_misb_feeds(config, &mut gateway, &sinks);
    let sapient = bind_sapient_feeds(config, &mut gateway, &sinks);
    (gateway, sinks, reports, sapient)
}

/// GAP-010: ADS-B receivers. Same reasoning and the same gap as AIS above: the codec
/// is decoded and gated, detections are tracked, and the cooperative reports go to a
/// sink nobody on the node fuses, because this binary has no edge to
/// `gungnir-identification`. Split out of [`build_gateway`] so that function stays
/// readable as a sequence of "bind this feed type" steps rather than growing a fourth
/// inline block the same size as this one.
fn bind_adsb_feeds(
    config: &ConfigBaseline,
    gateway: &mut IngestGateway,
    sinks: &[gungnir_ingest::adapters::asterix::ServiceObservationSink],
) {
    if config.adsb_feeds.is_empty() {
        return;
    }
    let Some(frame) = local_frame(config) else {
        tracing::warn!(
            feeds = config.adsb_feeds.len(),
            "ADS-B feeds are configured and no local frame origin is declared: no \
             receiver is bound"
        );
        return;
    };
    for feed in &config.adsb_feeds {
        let source: Result<Box<dyn gungnir_ingest::adapters::adsb::AvrSource>, String> = match &feed
            .source
        {
            gungnir_config::AdsbSource::Tcp { addr } => addr
                .parse()
                .map_err(|e| format!("{addr}: {e}"))
                .and_then(|addr| {
                    gungnir_ingest::adapters::adsb::TcpAvrSource::connect(
                        addr,
                        std::time::Duration::from_secs(3),
                    )
                    .map(|s| Box::new(s) as _)
                    .map_err(|e| e.to_string())
                }),
            gungnir_config::AdsbSource::File { path } => {
                gungnir_ingest::adapters::adsb::RecordedAvrSource::open(std::path::Path::new(path))
                    .map(|s| Box::new(s.with_lines_per_poll(256)) as _)
                    .map_err(|e| e.to_string())
            }
        };
        match source {
            Ok(source) => {
                let adapter = gungnir_ingest::adapters::adsb::AdsbAdapter::new(
                    feed.name.clone(),
                    SensorId(feed.sensor_id),
                    frame,
                    source,
                );
                tracing::info!(feed = %feed.name, sensor = feed.sensor_id, "ADS-B feed bound; detections are tracked, and the cooperative reports are not fused here because this binary has no edge to the crate that fuses evidence (GAP-010)");
                gateway.add_adapter(Box::new(adapter));
                gateway.set_expected_adapters(config.sensors.len() + sinks.len() + 1);
            }
            Err(err) => {
                tracing::error!(feed = %feed.name, %err, "ADS-B feed not bound");
            }
        }
    }
}

/// GAP-099: MISB ST 0601 UAS metadata receivers. Same reasoning and the same shape as
/// AIS/ADS-B above: the codec and adapter are decoded and gated
/// (`gungnir-interop/src/misb0601/mod.rs`, `gungnir-ingest/src/adapters/misb.rs`), the
/// platform's own position is tracked, and the orientation/sensor-pointing report goes
/// nowhere on this binary -- no sink of either kind is attached, exactly as AIS/ADS-B
/// attach none here -- because this binary has no edge to `gungnir-identification`.
/// Split out of [`build_gateway`] for the same reason as [`bind_adsb_feeds`].
fn bind_misb_feeds(
    config: &ConfigBaseline,
    gateway: &mut IngestGateway,
    sinks: &[gungnir_ingest::adapters::asterix::ServiceObservationSink],
) {
    if config.misb_feeds.is_empty() {
        return;
    }
    let Some(frame) = local_frame(config) else {
        tracing::warn!(
            feeds = config.misb_feeds.len(),
            "MISB feeds are configured and no local frame origin is declared: no \
             receiver is bound"
        );
        return;
    };
    for feed in &config.misb_feeds {
        let source: Result<Box<dyn gungnir_ingest::adapters::misb::KlvSource>, String> = match &feed
            .source
        {
            gungnir_config::MisbSource::Tcp { addr } => addr
                .parse()
                .map_err(|e| format!("{addr}: {e}"))
                .and_then(|addr| {
                    gungnir_ingest::adapters::misb::TcpKlvSource::connect(
                        addr,
                        std::time::Duration::from_secs(3),
                    )
                    .map(|s| Box::new(s) as _)
                    .map_err(|e| e.to_string())
                }),
            gungnir_config::MisbSource::File { path } => {
                gungnir_ingest::adapters::misb::RecordedKlvSource::open(std::path::Path::new(path))
                    .map(|s| Box::new(s) as _)
                    .map_err(|e| e.to_string())
            }
        };
        match source {
            Ok(source) => {
                let adapter = gungnir_ingest::adapters::misb::UasMetadataAdapter::new(
                    feed.name.clone(),
                    SensorId(feed.sensor_id),
                    frame,
                    source,
                );
                tracing::info!(feed = %feed.name, sensor = feed.sensor_id, "MISB feed bound; the platform's own position is tracked, and the orientation/sensor-pointing report is not fused here because this binary has no edge to the crate that fuses evidence (GAP-099, matching GAP-010's AIS/ADS-B reasoning)");
                gateway.add_adapter(Box::new(adapter));
                gateway.set_expected_adapters(config.sensors.len() + sinks.len() + 1);
            }
            Err(err) => {
                tracing::error!(feed = %feed.name, %err, "MISB feed not bound");
            }
        }
    }
}

/// What binding the SAPIENT feeds produced beyond the gateway adapters themselves
/// (GAP-004): every feed's `TaskAck` sink, to drain each tick, and a `sensor ->
/// adapter` pair for every feed a `destination_id` names, to attach to the registry.
#[derive(Default)]
struct BoundSapientFeeds {
    task_ack_sinks: Vec<gungnir_ingest::adapters::sapient::TaskAckSink>,
    task_adapters: Vec<(
        u32,
        Arc<dyn gungnir_sensor_management::tasking::SensorControlAdapter>,
    )>,
}

/// Routes a task to the adapter bound to that sensor's own SAPIENT connection
/// (GAP-004). Built because `InMemorySensorRegistry::attach_adapter` holds one
/// adapter for the whole registry, and a node's SAPIENT feeds are one TCP connection
/// per sensor -- the same shape the inbound side already binds -- not one shared
/// middleware for all of them.
struct SapientTaskRouter {
    by_sensor: std::collections::HashMap<
        u32,
        Arc<dyn gungnir_sensor_management::tasking::SensorControlAdapter>,
    >,
}

impl gungnir_sensor_management::tasking::SensorControlAdapter for SapientTaskRouter {
    fn issue(
        &self,
        task: &gungnir_sensor_management::tasking::SensorTask,
    ) -> Result<(), gungnir_sensor_management::SensorManagementError> {
        match self.by_sensor.get(&task.sensor.0) {
            Some(adapter) => adapter.issue(task),
            None => Err(gungnir_sensor_management::SensorManagementError::Refused {
                sensor: task.sensor,
                reason: format!(
                    "sensor {} has no SAPIENT task adapter attached",
                    task.sensor.0
                ),
            }),
        }
    }
}

/// GAP-001: SAPIENT edge nodes. Unlike AIS/ADS-B these are detection sources, not
/// cooperative-identity ones (`gungnir-app/src/sapient.rs`'s module documentation), so
/// there is no evidence-fusion edge to be missing here: a bound feed's detections are
/// tracked exactly like a radar's. Split out of [`build_gateway`] for the same reason
/// as [`bind_adsb_feeds`].
///
/// **GAP-004: the node's half of outbound tasking.** Every feed's `TaskAck`s are read
/// (`with_task_ack_sink`), the desktop's own wiring since the reader was built. A feed
/// whose source is `Tcp` and whose `destination_id` is configured also gets a
/// `SapientTaskAdapter`, its sink an independent handle to that same connection
/// (`TcpSapientSource::sink`) -- extracted here, before the source is erased to `Box<dyn
/// SapientSource>` for the gateway, because there is nowhere left to reach the concrete
/// type from after that. Validation already refuses a `destination_id` without
/// `sapient_node_id` or over a `File` source, so neither case is re-checked here.
///
/// Long on purpose, the same reason `run`'s own allow states: the sequence -- resolve
/// the sensor, connect the source, extract a task sink from it if one is wanted,
/// build the detection adapter over it -- is one feed's worth of setup, and splitting
/// it would put a single feed's binding in two places for no reader's benefit.
#[allow(clippy::too_many_lines)]
fn bind_sapient_feeds(
    config: &ConfigBaseline,
    gateway: &mut IngestGateway,
    sinks: &[gungnir_ingest::adapters::asterix::ServiceObservationSink],
) -> BoundSapientFeeds {
    let mut bound = BoundSapientFeeds::default();
    if config.sapient_feeds.is_empty() {
        return bound;
    }
    let Some(frame) = local_frame(config) else {
        tracing::warn!(
            feeds = config.sapient_feeds.len(),
            "SAPIENT feeds are configured and no local frame origin is declared: no \
             node is bound, because a bearing's origin cannot be placed without one"
        );
        return bound;
    };
    for feed in &config.sapient_feeds {
        let Some(sensor) = config.sensors.iter().find(|s| s.id == feed.sensor_id) else {
            tracing::error!(
                feed = %feed.name,
                sensor = feed.sensor_id,
                "SAPIENT feed names a sensor not in the sensor list"
            );
            continue;
        };
        let observer_enu = frame.to_enu(gungnir_model::Geodetic {
            lat_rad: sensor.position[0],
            lon_rad: sensor.position[1],
            alt_m: sensor.position[2],
        });
        let node_type = match feed.node_type {
            gungnir_config::SapientNodeType::Spotter => {
                gungnir_ingest::adapters::sapient::SPOTTER_NODE_TYPE
            }
            gungnir_config::SapientNodeType::Acoustic => {
                gungnir_ingest::adapters::sapient::ACOUSTIC_NODE_TYPE
            }
            gungnir_config::SapientNodeType::PassiveRf => {
                gungnir_ingest::adapters::sapient::PASSIVE_RF_NODE_TYPE
            }
        };
        let source: Result<Box<dyn gungnir_ingest::adapters::sapient::SapientSource>, String> =
            match &feed.source {
                gungnir_config::SapientSource::Tcp { addr } => addr
                    .parse()
                    .map_err(|e| format!("{addr}: {e}"))
                    .and_then(|addr| {
                        gungnir_ingest::adapters::sapient::TcpSapientSource::connect(
                            addr,
                            std::time::Duration::from_secs(3),
                        )
                        .map_err(|e| e.to_string())
                        .and_then(|tcp| {
                            if let Some(destination_id) = &feed.destination_id {
                                let Some(node_id) = &config.sapient_node_id else {
                                    // Unreachable once validated; refused rather than
                                    // silently untasked if it somehow is not.
                                    return Err(
                                        "a destination_id is set with no sapient_node_id"
                                            .to_string(),
                                    );
                                };
                                let sink = tcp.sink().map_err(|e| e.to_string())?;
                                let adapter = gungnir_sensor_management::sapient_task::SapientTaskAdapter::new(
                                    node_id.clone(),
                                    gungnir_sensor_management::sapient_task::SapientDestinations::from_sensors([(
                                        feed.sensor_id,
                                        destination_id.clone(),
                                    )]),
                                    move |json: String| sink.send(json),
                                );
                                bound.task_adapters.push((feed.sensor_id, Arc::new(adapter)));
                            }
                            Ok(Box::new(tcp) as _)
                        })
                    }),
                gungnir_config::SapientSource::File { path } => {
                    gungnir_ingest::adapters::sapient::RecordedSapientSource::open(
                        std::path::Path::new(path),
                    )
                    .map(|s| Box::new(s) as _)
                    .map_err(|e| e.to_string())
                }
            };
        match source {
            Ok(source) => {
                let task_ack_sink = gungnir_ingest::adapters::sapient::TaskAckSink::default();
                let adapter = gungnir_ingest::adapters::sapient::SapientDetectionAdapter::new(
                    feed.name.clone(),
                    SensorId(feed.sensor_id),
                    frame,
                    observer_enu,
                    source,
                    node_type,
                )
                .with_task_ack_sink(task_ack_sink.clone());
                bound.task_ack_sinks.push(task_ack_sink);
                tracing::info!(feed = %feed.name, sensor = feed.sensor_id, node_type, taskable = feed.destination_id.is_some(), "SAPIENT feed bound");
                gateway.add_adapter(Box::new(adapter));
                gateway.set_expected_adapters(config.sensors.len() + sinks.len() + 1);
            }
            Err(err) => {
                tracing::error!(feed = %feed.name, %err, "SAPIENT feed not bound");
            }
        }
    }
    bound
}

/// The deployment's local frame, when it has declared one.
fn local_frame(config: &ConfigBaseline) -> Option<gungnir_model::LocalFrame> {
    config.origin.map(|[lat_rad, lon_rad, alt_m]| {
        gungnir_model::LocalFrame::new(gungnir_model::Geodetic {
            lat_rad,
            lon_rad,
            alt_m,
        })
    })
}

/// The feeds as the ingest crate builds them: validated addresses parsed, positions
/// taken from the sensor list. `df_sites` (GAP-100) is built the same way `radars` is.
/// `uas_sites` (GAP-101) needs no position at all -- see `gungnir_ingest`'s own
/// documentation on `UasBinding` -- so the sensor list is consulted only to confirm the
/// named sensor exists, which is what the gateway's allow list admits a detection under.
fn feed_specs(config: &ConfigBaseline) -> Vec<gungnir_ingest::adapters::asterix::FeedSpec> {
    use gungnir_ingest::adapters::asterix::{DfBinding, FeedSpec, RadarBinding, UasBinding};
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
                        position: gungnir_model::Geodetic {
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
                        position: gungnir_model::Geodetic {
                            lat_rad: sensor.position[0],
                            lon_rad: sensor.position[1],
                            alt_m: sensor.position[2],
                        },
                        azimuth_sigma_rad: d.azimuth_sigma_rad,
                    })
                })
                .collect();
            let uas_sites = f
                .uas_sites
                .iter()
                .filter(|u| config.sensors.iter().any(|s| s.id == u.sensor_id))
                .map(|u| UasBinding {
                    sac: u.sac,
                    sic: u.sic,
                    sensor: SensorId(u.sensor_id),
                })
                .collect();
            Some(FeedSpec {
                name: f.name.clone(),
                bind_addr,
                multicast,
                radars,
                df_sites,
                uas_sites,
            })
        })
        .collect()
}

/// Drain the feeds' service observations into the registry (GAP-064): the radar
/// speaking for itself confirms what the registry may report.
fn observe_services(
    sinks: &[gungnir_ingest::adapters::asterix::ServiceObservationSink],
    sensors: &mut InMemorySensorRegistry,
    now: gungnir_model::MissionTime,
) {
    use gungnir_ingest::adapters::asterix::ServiceObservationKind;
    use gungnir_sensor_management::ServiceObservation;
    for sink in sinks {
        let drained: Vec<_> = match sink.lock() {
            Ok(mut q) => q.drain(..).collect(),
            Err(_) => continue,
        };
        for o in drained {
            let observations = [
                match o.kind {
                    ServiceObservationKind::NorthMarker => Some(ServiceObservation::NorthMarker {
                        rotation_period_s: o.rotation_period_s,
                    }),
                    ServiceObservationKind::SectorCrossing => {
                        Some(ServiceObservation::SectorCrossing)
                    }
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
                match sensors.observe_service(o.sensor, observation, now) {
                    Ok(Some(from)) => tracing::info!(
                        sensor = o.sensor.0,
                        ?from,
                        "radar confirmed searching by its own service message"
                    ),
                    Ok(None) => {}
                    Err(err) => {
                        tracing::warn!(sensor = o.sensor.0, %err, "service message from a sensor the registry does not hold");
                    }
                }
            }
        }
    }
}

/// The sensor registry this node holds (GAP-003).
///
/// Every sensor starts at Standby, so a node that has just come up reports covering
/// nothing. That is true rather than pessimistic, and it is what makes the coverage
/// view worth reading during MT-07: a registry that came up claiming everything was
/// searching would report coverage nobody had switched on.
fn build_registry(config: &ConfigBaseline) -> InMemorySensorRegistry {
    let sensors = InMemorySensorRegistry::from_config(
        &config.sensors,
        &format!("baseline-v{}", config.version),
    );
    tracing::info!(
        sensors = sensors.sensors().len(),
        contributing = sensors.coverage().len(),
        "sensor registry built from the baseline"
    );
    sensors
}

/// Carries detections submitted over the API into the ingest gateway.
///
/// **A `ProtocolAdapter` rather than a direct call into the gateway**, so a submission is
/// authenticated against the sensor allow-list and validated by exactly the code a
/// sensor's own feed goes through. `gungnir-ingest` is the trust boundary for external
/// data; a route that reached past it would be a second way in with no checks on it.
struct ApiSubmissionAdapter {
    api: Arc<NodeApi>,
}

impl gungnir_ingest::ProtocolAdapter for ApiSubmissionAdapter {
    // The trait ties the returned lifetime to `&self`, so a `&'static str` here would
    // not match its signature. The adapters in `gungnir-ingest` are written the same way.
    #[allow(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "api-v2-submission"
    }

    fn poll(
        &mut self,
        _now: gungnir_model::MissionTime,
    ) -> Result<Vec<gungnir_model::DetectionView>, gungnir_ingest::IngestError> {
        Ok(self.api.take_submissions())
    }
}

/// Carries the machine-submitted detections (GAP-002) into the gateway, apart from the
/// operators' queue so the gateway can admit them under the machine-identity
/// authenticator.
struct MachineSubmissionAdapter {
    api: Arc<NodeApi>,
}

impl gungnir_ingest::ProtocolAdapter for MachineSubmissionAdapter {
    #[allow(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "api-v2-machine-submission"
    }

    fn poll(
        &mut self,
        _now: gungnir_model::MissionTime,
    ) -> Result<Vec<gungnir_model::DetectionView>, gungnir_ingest::IngestError> {
        Ok(self.api.take_machine_submissions())
    }
}

/// Issue the sensor tasks the transport accepted (GAP-004) through the registry, on the
/// record, and answer each route. A refusal is the registry's own words and records
/// nothing (DN-11 §5); an adapter's refusal is recorded as `Failed`, which the desktop
/// sees on the stream.
fn issue_api_tasks(
    api: &NodeApi,
    sensors: &mut InMemorySensorRegistry,
    bus: &InProcessBus,
    now: gungnir_model::MissionTime,
) -> Result<(), Box<dyn std::error::Error>> {
    use gungnir_model::events::SensorTaskEvent;
    use gungnir_sensor_management::SensorControl;
    for pending in api.take_tasks() {
        let sensor = pending.sensor;
        let answer = sensors
            .issue(sensor, pending.command, pending.requirement, now)
            .map_err(|e| e.to_string());
        if let Ok(task) = answer {
            bus.publish(
                now,
                Event::SensorTask(SensorTaskEvent::Issued {
                    task,
                    sensor,
                    at: now,
                }),
            )?;
            if let Some(reason) =
                sensors
                    .tasks()
                    .iter()
                    .find(|t| t.id == task)
                    .and_then(|t| match &t.state {
                        gungnir_sensor_management::tasking::TaskState::Failed { reason } => {
                            Some(reason.clone())
                        }
                        _ => None,
                    })
            {
                bus.publish(
                    now,
                    Event::SensorTask(SensorTaskEvent::Failed {
                        task,
                        sensor,
                        reason,
                        at: now,
                    }),
                )?;
            }
        }
        // A dropped receiver means the route timed out; nothing to answer.
        let _ = pending.reply.send(answer);
    }
    Ok(())
}

/// Read every bound SAPIENT feed's `TaskAck`s and apply them to the registry
/// (GAP-004): the node's own reader, the same shape `gungnir-app/src/sapient.rs`'s
/// `apply_task_ack` applies on the desktop. **Unlike the desktop's reader, this one
/// publishes `SensorTaskEvent`**: the desktop's record has no further audience, but
/// this node is the system of record for every desktop connected to it (the same
/// reason `issue_api_tasks` above publishes `Issued`/`Failed`), so an acknowledgement
/// or a rejection has to reach a connected desktop's own stream, not just this
/// process's registry.
fn apply_sapient_task_acks(
    sinks: &[gungnir_ingest::adapters::sapient::TaskAckSink],
    sensors: &mut InMemorySensorRegistry,
    bus: &InProcessBus,
    now: gungnir_model::MissionTime,
) -> Result<(), Box<dyn std::error::Error>> {
    use gungnir_ingest::adapters::sapient::TaskAckStatus;
    use gungnir_model::events::SensorTaskEvent;
    use gungnir_sensor_management::SensorControl;

    let mut drained = Vec::new();
    for sink in sinks {
        if let Ok(mut queue) = sink.lock() {
            drained.extend(queue.drain(..));
        }
    }
    for report in drained {
        let Some(task) = gungnir_sensor_management::sapient_task::decode_task_id(&report.task_id)
        else {
            tracing::warn!(
                task_id = %report.task_id,
                "a SAPIENT TaskAck named a task id this node did not mint; ignored"
            );
            continue;
        };
        let Some(sensor) = sensors
            .tasks()
            .iter()
            .find(|t| t.id == task)
            .map(|t| t.sensor)
        else {
            tracing::warn!(
                task = task.0,
                "a SAPIENT TaskAck named a task this node has no record of; ignored"
            );
            continue;
        };
        match report.status {
            TaskAckStatus::Accepted => match sensors.acknowledge(task, now) {
                Ok(()) => {
                    bus.publish(
                        now,
                        Event::SensorTask(SensorTaskEvent::Acknowledged {
                            task,
                            sensor,
                            at: now,
                        }),
                    )?;
                }
                Err(err) => {
                    tracing::warn!(task = task.0, sensor = sensor.0, %err, "a SAPIENT TaskAck could not be applied");
                }
            },
            TaskAckStatus::Rejected => {
                let reason = if report.reasons.is_empty() {
                    "rejected".to_string()
                } else {
                    report.reasons.join("; ")
                };
                match sensors.fail(task, reason.clone()) {
                    Ok(()) => {
                        bus.publish(
                            now,
                            Event::SensorTask(SensorTaskEvent::Failed {
                                task,
                                sensor,
                                reason,
                                at: now,
                            }),
                        )?;
                    }
                    Err(err) => {
                        tracing::warn!(task = task.0, sensor = sensor.0, %err, "a SAPIENT TaskAck could not be applied");
                    }
                }
            }
            // Same rule the desktop's own reader states: the registry's task lifecycle
            // has no state past Acknowledged/Failed for a task the sensor already
            // accepted, so calling acknowledge/fail again for the ordinary
            // accept-then-finish sequence a bounded task produces would return
            // TaskClosed for no defect at all. Logged, not applied.
            TaskAckStatus::Completed | TaskAckStatus::Failed => {
                tracing::info!(
                    task = task.0,
                    sensor = sensor.0,
                    status = ?report.status,
                    "a SAPIENT TaskAck reported a post-acceptance outcome"
                );
            }
        }
    }
    Ok(())
}

/// Put the effector reports the transport accepted (GAP-040) on the record.
fn record_effector_reports(
    api: &NodeApi,
    bus: &InProcessBus,
    now: gungnir_model::MissionTime,
) -> Result<(), Box<dyn std::error::Error>> {
    for record in api.take_effector_reports() {
        tracing::info!(decision = record.decision.0, endpoint = %record.endpoint, "effector report on the record");
        bus.publish(
            now,
            Event::Handoff(gungnir_model::events::HandoffEvent::Reported {
                decision: record.decision,
                endpoint: record.endpoint,
                report: record.report,
                at: now,
            }),
        )?;
    }
    Ok(())
}

/// Put the warning acknowledgements the transport accepted (GAP-042) on the record.
///
/// **The node owns no warnings**, so it applies none: a desktop raises them against its
/// own defended assets and holds the ledger. This is the effector-report path exactly --
/// record the fact for every desktop, and let the one that raised the warning apply it or
/// reject it as naming a pair it never raised (DN-03 §5 rule 2).
///
/// The envelope's mission time is this node's `now`; the party's own claimed time travels
/// inside the event, so the record holds both.
fn record_warning_acknowledgements(
    api: &NodeApi,
    bus: &InProcessBus,
    now: gungnir_model::MissionTime,
) -> Result<(), Box<dyn std::error::Error>> {
    for record in api.take_warning_acknowledgements() {
        tracing::info!(
            asset = record.asset.0,
            track = record.track.0,
            party = %record.party,
            "warning acknowledgement on the record"
        );
        bus.publish(
            now,
            Event::Warning(gungnir_model::events::WarningEvent::Acknowledged {
                asset: record.asset,
                track: record.track,
                party: record.party,
                at: record.at,
            }),
        )?;
    }
    Ok(())
}

/// The coverage answer this node serves, or why there is none (GAP-006, DN-12).
///
/// **A node with no declared local frame origin cannot produce one at all**: sensor
/// positions and approach points are geodetic, and placing them in a common ENU picture
/// needs the origin. Guessing one from the first sensor would put every ring somewhere
/// plausible and wrong, so the absence is reported. Likewise a deployment that has
/// declared no approaches has nothing to measure coverage *along*, and an empty gap list
/// would read as a clean sector.
fn coverage_answer(config: &ConfigBaseline, sensors: &InMemorySensorRegistry) -> CoverageResponse {
    let Some([lat_rad, lon_rad, alt_m]) = config.origin else {
        return CoverageResponse::NotComputed {
            reason: "this deployment has declared no local frame origin, so geodetic \
                     positions cannot be placed in a common picture"
                .into(),
        };
    };
    if config.approaches.is_empty() {
        return CoverageResponse::NotComputed {
            reason: "this deployment has declared no approaches, so there is nothing to \
                     measure coverage along"
                .into(),
        };
    }

    let frame = gungnir_model::LocalFrame::new(gungnir_model::Geodetic {
        lat_rad,
        lon_rad,
        alt_m,
    });
    let volumes = gungnir_analytics::coverage_from_registry(
        sensors,
        config.analytics.coverage_min_elevation_rad,
        |record| frame.to_enu(record.position),
    );
    let routes: Vec<Vec<[f64; 3]>> = config
        .approaches
        .iter()
        .map(|a| {
            a.points
                .iter()
                .map(|[lat_rad, lon_rad, alt_m]| {
                    frame.to_enu(gungnir_model::Geodetic {
                        lat_rad: *lat_rad,
                        lon_rad: *lon_rad,
                        alt_m: *alt_m,
                    })
                })
                .collect()
        })
        .collect();
    let approaches: Vec<&[[f64; 3]]> = routes.iter().map(Vec::as_slice).collect();

    CoverageResponse::Computed(gungnir_analytics::combined_coverage(
        &volumes,
        // No terrain is loaded (GAP-023), so line of sight is flat. Reported on the
        // parameters rather than assumed away, which is why the response carries them.
        &gungnir_analytics::FlatTerrainLineOfSight,
        &approaches,
        gungnir_analytics::CoverageParameters {
            sample_spacing_m: config.analytics.coverage_sample_spacing_m,
            terrain_masking_applied: false,
        },
    ))
}

/// Republish the picture for anyone connected.
///
/// Every tick: a desktop takes this once on connecting and follows the event stream
/// after, so it is cheap and always current. Collection requirements are empty because
/// a node states none of its own -- PN-15 is a desktop panel, and publishing an empty
/// list is different from the field being absent (GAP-005).
///
/// **`bearing_rays`/`pipeline_stats` are the same values `tracking.bearing_rays()`/
/// `tracking.pipeline_stats()` already give an embedded desktop** (GAP-096's wire
/// contract): attached here so a connected one reads the same picture rather than the
/// `TrackingService` trait's defaulted empty answer `gungnir-remote` gave before this
/// entry. Refreshed every tick like `tracks`, so a snapshot taken right after this call
/// is as current as the pipeline's last poll -- there is no separate live update for
/// either between snapshots (see `gungnir_remote::RemoteTrackingService`'s own doc
/// comment for what that means for a connected desktop).
fn publish_picture(
    api: &Arc<NodeApi>,
    tracks: &[gungnir_model::TrackView],
    bearing_rays: &[gungnir_model::BearingRayView],
    pipeline_stats: PipelineStats,
    plan: &PlanView,
    health: SystemHealth,
) {
    let snapshot = SnapshotResponse::new(tracks.to_vec(), Some(plan.clone()), health, Vec::new())
        .with_bearing_data(
            bearing_rays.to_vec(),
            project_pipeline_stats(pipeline_stats),
        );
    if let Err(err) = api.publish_snapshot(snapshot) {
        tracing::error!(%err, "could not publish the snapshot");
    }
}

/// Start the v2 transport, or say why it is not started.
///
/// Mutual TLS when the deployment configures it, and plaintext on loopback when it does
/// not. **The address restriction is on plaintext, not on the address**: a node with TLS
/// may serve a routable address, and a node without it may not.
///
/// Not starting is not a fatal error: the node's job is to run the pipeline and journal
/// it, and it does that with nobody connected. Refusing loudly is the point -- a node
/// that fell back to a plaintext listener would be worse than one that serves nobody.
async fn spawn_transport(
    bind_addr: &str,
    api: &Arc<NodeApi>,
    handle: &tokio::runtime::Handle,
    identity_dir: &std::path::Path,
) {
    let Ok(addr) = bind_addr.parse::<SocketAddr>() else {
        tracing::error!(bind = %bind_addr, "bind address is not a socket address; not serving");
        return;
    };

    // TLS first: a deployment that configured it and got it wrong must not quietly fall
    // through to the plaintext path, which is the silent downgrade this design refuses.
    match gungnir_api::tls::TlsPaths::from_env() {
        Some(Ok(paths)) => {
            spawn_tls(addr, &paths, api, handle).await;
            return;
        }
        Some(Err(err)) => {
            // D-29: the client authority alone means the node's own identity comes from
            // its key provider; the other partial combinations stay misconfigurations.
            if let Some(client_ca) = gungnir_api::tls::client_ca_from_env() {
                if std::env::var("GUNGNIR_TLS_CERT").is_err()
                    && std::env::var("GUNGNIR_TLS_KEY").is_err()
                {
                    spawn_tls_from_provider(addr, &client_ca, api, handle, identity_dir).await;
                    return;
                }
            }
            tracing::error!(%err, "TLS is misconfigured; not serving");
            return;
        }
        None => {}
    }

    match gungnir_api::transport::bind(addr).await {
        Ok(listener) => {
            let served = listener
                .local_addr()
                .map_or_else(|_| addr.to_string(), |a| a.to_string());
            tracing::info!(
                bind = %served,
                // Accurate in both configurations rather than always the pessimistic
                // one. This line previously said writes always refuse, which stopped
                // being true when the write paths gained authorisation and was still
                // printed on every start; the two warnings above already say whether this
                // deployment has a caller authority at all. A log that overstates what a
                // deployment cannot do is read as carelessly as one that overstates what
                // it can.
                "serving the v2 transport; a write path is served where its caller can be authenticated and refuses otherwise"
            );
            let serving = Arc::clone(api);
            handle.spawn(async move {
                if let Err(err) = gungnir_api::transport::serve_on(listener, serving).await {
                    tracing::error!(%err, "the v2 transport stopped");
                }
            });
        }
        Err(err) => tracing::error!(%err, "not serving the v2 transport"),
    }
}

/// Serve with mutual TLS under an identity the node's key provider issues (D-29,
/// GAP-060): a self-signed certificate over the provider's transport key, signed through
/// custody, written beside the journal as `node-identity.pem` for operators to pin.
///
/// **Persistent since 2026-09-08 (GAP-060's remaining slice), when the operating
/// system's keystore is reachable.** `issue_node_serving_identity` opens a
/// `PersistentKeyProvider` under `identity_dir` first, so a restart keeps the same
/// certificate operators already pinned; an unreachable keystore falls back to a fresh
/// ephemeral identity, logged as a fallback by that function rather than here, exactly
/// the honest-or-nothing rule DN-22 §5 already applies to journal encryption.
async fn spawn_tls_from_provider(
    addr: SocketAddr,
    client_ca: &str,
    api: &Arc<NodeApi>,
    handle: &tokio::runtime::Handle,
    identity_dir: &std::path::Path,
) {
    let names = vec![addr.ip().to_string(), "localhost".to_string()];
    // Moved to `gungnir-remote` on 2026-09-06 (GAP-060) so the desktop can issue one the
    // same way. The node used to own this; two binaries cannot depend on each other, so
    // leaving it here meant a second copy on the desktop that would drift.
    let identity = match gungnir_remote::identity::issue_node_serving_identity(
        identity_dir,
        names,
        "gungnir-node",
    ) {
        Ok(identity) => identity,
        Err(err) => {
            tracing::error!(%err, "the node's identity could not be issued; not serving");
            return;
        }
    };
    let pem_path = identity_dir.join("node-identity.pem");
    match std::fs::write(&pem_path, &identity.certificate_pem) {
        Ok(()) => {
            tracing::info!(path = %pem_path.display(), "node identity written for operators to pin (public certificate only)");
        }
        Err(err) => {
            tracing::warn!(%err, "the node identity could not be written; clients must pin it another way");
        }
    }
    let acceptor = match gungnir_api::tls::acceptor_with_key(
        identity.certificate_der,
        identity.key,
        client_ca,
    ) {
        Ok(acceptor) => acceptor,
        Err(err) => {
            tracing::error!(%err, "the client authority could not be loaded; not serving");
            return;
        }
    };
    serve_acceptor(addr, acceptor, api, handle).await;
}

/// Bind and serve behind an acceptor, whichever way it was built.
async fn serve_acceptor(
    addr: SocketAddr,
    acceptor: gungnir_api::tls::TlsAcceptor,
    api: &Arc<NodeApi>,
    handle: &tokio::runtime::Handle,
) {
    let tcp = match tokio::net::TcpListener::bind(addr).await {
        Ok(tcp) => tcp,
        Err(err) => {
            tracing::error!(%err, bind = %addr, "could not bind; not serving");
            return;
        }
    };
    let served = tcp
        .local_addr()
        .map_or_else(|_| addr.to_string(), |a| a.to_string());
    tracing::info!(
        bind = %served,
        "serving the v2 contract over mutual TLS; a client certificate is required"
    );
    let listener = gungnir_api::tls::TlsListener::new(tcp, acceptor);
    let serving = Arc::clone(api);
    handle.spawn(async move {
        if let Err(err) = gungnir_api::transport::serve_on_listener(listener, serving).await {
            tracing::error!(%err, "the v2 transport stopped");
        }
    });
}

/// Serve with mutual TLS.
async fn spawn_tls(
    addr: SocketAddr,
    paths: &gungnir_api::tls::TlsPaths,
    api: &Arc<NodeApi>,
    handle: &tokio::runtime::Handle,
) {
    let acceptor = match gungnir_api::tls::acceptor(paths) {
        Ok(acceptor) => acceptor,
        // Fatal to serving, deliberately: a node that fell back to plaintext because a
        // certificate was unreadable would be the downgrade nobody would notice.
        Err(err) => {
            tracing::error!(%err, "the TLS material could not be loaded; not serving");
            return;
        }
    };
    serve_acceptor(addr, acceptor, api, handle).await;
}

/// The node's whole life: construct the services, then tick them until stopped.
///
/// Long on purpose. Splitting the loop would put the order of the pipeline -- ingest,
/// tracking, planning, journal, transport, health -- in two places, and that order is the
/// thing a reader comes here to check.
#[allow(clippy::too_many_lines)]
async fn run(
    config: ConfigBaseline,
    node_cfg: NodeConfig,
    handle: tokio::runtime::Handle,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        api = API_VERSION,
        bind = %node_cfg.bind_addr,
        data_dir = %node_cfg.data_dir,
        "gungnir-node starting"
    );

    // GAP-086 and GAP-053: the promoted algorithm baseline decides how this node
    // filters, so it is read here, before the tracker it configures. It reaches the
    // journal further down, once the event bus exists.
    let governance = gungnir_modelops::InMemoryModelRegistry::from_baseline(&config);
    let promoted: Option<gungnir_modelops::ModelBaseline> = match &governance {
        Ok(registry) => config
            .operating_profile()
            .and_then(|p| gungnir_modelops::ModelRegistry::promoted(registry, &p))
            .cloned(),
        Err(err) => {
            tracing::error!(
                %err,
                "the baseline's algorithm configuration was refused; this node governs nothing"
            );
            None
        }
    };

    // GAP-012: the tracker judges staleness by the baseline's policy. GAP-053: and it
    // filters as the promoted algorithm baseline says, stamping that baseline's
    // identifier only because it is applying it (DN-24 §7). A baseline naming a filter
    // this build does not implement is refused by name and the tracker stays ungoverned,
    // rather than running a different filter under the promoted one's identity.
    let mut tracking = match promoted.as_ref().map(|b| {
        // DN-28 §5: the imm-cv-ct fields, built from the baseline's own `TrackingConfig`
        // here rather than in `gungnir-tracking-service`, which may not depend on
        // `gungnir-config`.
        let imm = gungnir_tracking_service::ImmBaselineFields {
            turn_rate_rad_s: b.config.imm_turn_rate_rad_s,
            mode_transition: b.config.imm_mode_transition,
            initial_mode_probabilities: b.config.imm_initial_mode_probabilities,
        };
        (
            b,
            gungnir_tracking_service::PipelineSettings::from_baseline(
                b.config.gate_threshold,
                &b.config.filter_selection,
                &imm,
                b.config.measurement_noise_var,
            ),
        )
    }) {
        Some((baseline, Ok(settings))) => {
            tracing::info!(baseline = %baseline.id, "the promoted algorithm baseline is applied");
            LiveTrackingService::with_pipeline_settings(&handle, settings)
                .with_staleness(config.policy.staleness.clone())
                .with_sensor_positions(sensor_positions(&config))
                .with_algorithm_baseline(&baseline.id)
        }
        Some((baseline, Err(err))) => {
            tracing::error!(baseline = %baseline.id, %err, "the promoted algorithm baseline is not applied; the tracker runs its default filter and stays ungoverned");
            LiveTrackingService::new(&handle)
                .with_staleness(config.policy.staleness.clone())
                .with_sensor_positions(sensor_positions(&config))
        }
        None => LiveTrackingService::new(&handle)
            .with_staleness(config.policy.staleness.clone())
            .with_sensor_positions(sensor_positions(&config)),
    };
    let mut intercept = DpInterceptService::new(config.allocation_horizon);
    let resources = config.resource_views();
    // GAP-088: the fences the baseline declares. None declared is said once, because
    // an engine that can only pass is worth knowing about.
    let geo = geo_service_from(&config);
    if config.geofences.is_empty() {
        tracing::warn!(
            "no geofences are declared: the geofence engine will deny nothing; authority \
             is not evaluated on a node, which has no signed-in caller (DN-23 §4)"
        );
    } else {
        tracing::info!(
            fences = config.geofences.len(),
            "geofences declared; authority is not evaluated on a node (DN-23 §4)"
        );
    }

    let bus = InProcessBus::new();
    let journal_rx = bus.subscribe();
    // The default policy is `SyncEveryEnvelope`, which is the service-node half of
    // D-04 (`ARCHITECTURE.md` §10 item 19): the node is the system of record for every
    // connected desktop, and its budget is "an accepted envelope is on disk within
    // 100 ms". The desktop takes the buffered profile instead.
    let mut journal = FileEventJournal::open(&node_cfg.data_dir)?;
    // GAP-060: the node seals its journal the way the desktop does (DN-22 §5), and says
    // what it is doing. An ephemeral key on a node is a system of record that cannot be
    // read after restart; it is allowed because the baseline said so, and warned about.
    let encryption = seal_journal(&config, &mut journal);
    match &encryption {
        gungnir_security::EncryptionStatus::Active { provider } => {
            tracing::info!(%provider, "journal encryption active");
        }
        gungnir_security::EncryptionStatus::NotConfigured => {
            tracing::warn!("journal encryption is not configured; the journal is plaintext");
        }
        gungnir_security::EncryptionStatus::UnavailableWritingPlaintext { reason } => {
            tracing::warn!(%reason, "journal encryption is off; writing plaintext");
        }
    }

    // GAP-019, edge (s): fold the retained sessions into a resolver before the loop
    // opens a new one, so a track that reappears is correlated to the entity it was
    // rather than minted afresh. Done here, on the node, because this journal is the
    // authoritative account of the mission: a watch during which no desktop was attached
    // is exactly the period whose correlation would otherwise be lost.
    let mut entities =
        entities::EntityIdentity::recover(&journal, config.reporting.retention_sessions);
    if let Some(reason) = entities.unreadable() {
        // Said, not skipped: a resolver folded from some of the retained sessions will
        // mint a new entity for a track the unread ones would have matched, and the
        // journal would show an object appearing where none did.
        tracing::warn!(%reason, "the entity resolver was folded from an incomplete journal");
    }
    tracing::info!(
        sessions = entities.sessions(),
        entities = entities.entities(),
        "entity resolver folded from the retained sessions"
    );

    let clock = WallClockAuthority::default();
    // GAP-051: created through the lifecycle rather than minted from the wall clock, so
    // there is a record on disk saying which baseline this session ran under and how it
    // ended. A node killed without closing leaves that record marked live, and the next
    // start reports it as interrupted rather than quietly reusing the ground.
    // Scoped: the manager reads the journal, and the loop below writes to it. Holding
    // one open across the run would borrow the journal for the whole node.
    let mission = {
        let mut missions = JournalMissionManager::open(journal.root(), &journal)?;
        for earlier in missions.missions()? {
            if missions.load(earlier)?.state == MissionState::Interrupted {
                tracing::warn!(
                    session = earlier.0,
                    "an earlier session was not closed; its record ends where the process stopped"
                );
            }
        }
        let mut mission = missions.create(config.clone())?;
        missions.transition(&mut mission, MissionState::Live)?;
        mission
    };
    let session = mission.session;
    tracing::info!(session = session.0, "opened live session");

    // GAP-086: which algorithm configuration this session opened with, in the journal.
    // **Not a promotion** -- nobody promoted anything, the baseline said so, and there is
    // nobody signed in to attribute an act to.
    // Read before the tracker was built, above; journaled here, where the bus exists.
    let in_force = promoted.as_ref().map(|b| b.id.clone());
    bus.publish(
        clock.now(),
        Event::Governance(match &in_force {
            Some(baseline) => gungnir_model::events::GovernanceEvent::InForceAtStart {
                baseline: baseline.clone(),
                at: clock.now(),
            },
            None => gungnir_model::events::GovernanceEvent::NoneInForce {
                profile: config.operating_profile(),
                at: clock.now(),
            },
        }),
    )?;
    if let Some(b) = &in_force {
        tracing::info!(baseline = %b, "algorithm configuration in force");
    } else {
        tracing::warn!("no algorithm configuration is in force");
    }

    let (mut gateway, service_sinks, feed_reports, sapient) = build_gateway(&config, &handle);
    let mut sensors = build_registry(&config);
    // GAP-004: attach the SAPIENT task adapters bound above, one per taskable feed,
    // routed by sensor id since the registry holds a single adapter slot for all of
    // them (`InMemorySensorRegistry::attach_adapter`) and a node's SAPIENT feeds are
    // one connection per sensor, not one shared middleware.
    if !sapient.task_adapters.is_empty() {
        let count = sapient.task_adapters.len();
        sensors.attach_adapter(Arc::new(SapientTaskRouter {
            by_sensor: sapient.task_adapters.into_iter().collect(),
        }));
        tracing::info!(sensors = count, "SAPIENT task adapters attached");
    }

    // GAP-041: the v2 read paths are served. The write paths are routed and refuse,
    // because nothing can authenticate a caller (GAP-057, GAP-060), and only loopback is
    // bound because there is no TLS (GAP-060). A node asked to bind anything else fails
    // to start rather than listening in plaintext.
    // DN-18 §6 (GAP-065): what each party may receive, from the baseline; empty means
    // an authenticated peer may receive nothing.
    let base = NodeApi::new(SnapshotResponse::new(
        Vec::new(),
        None,
        SystemHealth::default(),
        Vec::new(),
    ))
    .with_exchange(gungnir_model::ExchangeSet {
        agreements: config.exchange.clone(),
    })
    // D-02: what each client certificate speaks for (GAP-002, GAP-040).
    .with_machine_identities(
        config
            .machine_identities
            .iter()
            .map(|m| {
                let role = match &m.speaks_for {
                    gungnir_config::MachineRole::Sensor { sensor_id } => {
                        gungnir_api::transport::MachineRole::Sensor(SensorId(*sensor_id))
                    }
                    gungnir_config::MachineRole::Effector { endpoint } => {
                        gungnir_api::transport::MachineRole::Effector {
                            endpoint: endpoint.clone(),
                        }
                    }
                    gungnir_config::MachineRole::WarnedParty { channel } => {
                        gungnir_api::transport::MachineRole::WarnedParty {
                            channel: channel.clone(),
                        }
                    }
                    gungnir_config::MachineRole::Peer { peer } => {
                        gungnir_api::transport::MachineRole::Peer { name: peer.clone() }
                    }
                };
                (m.common_name.clone(), role)
            })
            .collect(),
    );
    if !config.machine_identities.is_empty() {
        tracing::info!(
            identities = config.machine_identities.len(),
            "machine identities recognised (D-02)"
        );
    }
    if !config.exchange.is_empty() {
        tracing::info!(
            parties = config.exchange.len(),
            "exchange agreements in force"
        );
    }
    let callers = match auth::build_caller_authority(&config) {
        Ok(authority) => {
            tracing::info!("caller authority built from the baseline's account store");
            Some(authority)
        }
        Err(why) => {
            tracing::warn!(%why, "no caller authority: nobody can sign in to this node");
            None
        }
    };
    let api = Arc::new(if let Some(callers) = callers {
        base.with_callers(Arc::new(callers))
    } else {
        tracing::warn!(
            "no caller authority configured: the v2 transport will refuse every route but the session one (GAP-057)"
        );
        base
    });
    // DN-18 §5, GAP-065: the three exchange items this node does not hold. Said once, in
    // words, rather than left to a default -- a partner reading an empty list would take
    // it for "there are none here", and the truth is that warnings, reports and handoffs
    // live on the desktops this node serves. `publish_exchange` is the door for a
    // deployment that does hold them.
    for (item, reason) in [
        (
            gungnir_model::ExchangeItem::Warnings,
            "warnings are raised on a desktop against its own defended assets; this node \
             holds no warning ledger",
        ),
        (
            gungnir_model::ExchangeItem::Reports,
            "reports are produced on a desktop from its journal; this node publishes none \
             for exchange",
        ),
        (
            gungnir_model::ExchangeItem::Handoffs,
            "handoffs are issued on a desktop from a recorded decision; this node holds \
             none",
        ),
    ] {
        api.withhold_exchange(item, reason)?;
    }
    let api_rx = bus.subscribe();
    // Registered after the transport is built, so a submitted detection has somewhere to
    // arrive from. The gateway counts it as an adapter, so a node with the API enabled
    // reports one more than the baseline's sensors -- which is true.
    gateway.add_adapter(Box::new(ApiSubmissionAdapter {
        api: Arc::clone(&api),
    }));
    // GAP-002: detections a sensor submitted under its own certificate, admitted by the
    // one authenticator that may stamp `MachineIdentity`. Registered even when no
    // identity is declared: an empty vouched list admits nothing, which is true.
    gateway.add_adapter_with_authenticator(
        Box::new(MachineSubmissionAdapter {
            api: Arc::clone(&api),
        }),
        Box::new(MachineIdentityAuthenticator {
            vouched: api.vouched_sensors(),
        }),
    );

    spawn_transport(
        &node_cfg.bind_addr,
        &api,
        &handle,
        std::path::Path::new(&node_cfg.data_dir),
    )
    .await;

    let watchdog = WatchdogConfig {
        max_ingest_gap_s: 30.0,
        max_tracking_pipeline_latency_s: 1.0,
    };
    let mut ticker = tokio::time::interval(TICK);
    let mut last_plan = PlanView::default();
    let mut last_health: Option<SystemHealth> = None;
    let mut last_health_log = Instant::now();
    let mut ingest_gap_warned = false;

    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);

    loop {
        tokio::select! {
            _ = &mut ctrl_c => {
                tracing::info!("shutdown requested");
                break;
            }
            _ = ticker.tick() => {}
        }
        let now = clock.now();

        observe_services(&service_sinks, &mut sensors, now);
        for event in gateway.tick(now, &mut tracking) {
            bus.publish(now, Event::Ingest(event))?;
        }
        issue_api_tasks(&api, &mut sensors, &bus, now)?;
        apply_sapient_task_acks(&sapient.task_ack_sinks, &mut sensors, &bus, now)?;
        record_effector_reports(&api, &bus, now)?;
        record_warning_acknowledgements(&api, &bus, now)?;
        // GAP-009, DN-16 §5: what the peers said that was not a track. After the
        // gateway tick, which is what fills the sinks.
        record_launch_warnings(&feed_reports.peers, &bus, now)?;

        // GAP-054: planned downtime is tracked here too, not only on the desktop. This
        // node is the system of record for every desktop connected to it, so a window
        // opening -- or closing with the sensor still off the air -- has to be in *its*
        // journal. Scheduled products are not produced here: they are a watch's paperwork
        // and a headless node has no watch, so the desktop owns them.
        for (sensor, next) in sensors.advance_maintenance(now) {
            let event = match next {
                gungnir_model::MaintenanceState::Active => {
                    gungnir_model::events::RhythmEvent::MaintenanceOpened {
                        sensor,
                        until: now,
                        reason: "see the baseline".into(),
                        at: now,
                    }
                }
                gungnir_model::MaintenanceState::Completed => {
                    gungnir_model::events::RhythmEvent::MaintenanceCompleted { sensor, at: now }
                }
                gungnir_model::MaintenanceState::Overrun => {
                    tracing::warn!(
                        sensor = sensor.0,
                        "sensor did not return from planned maintenance"
                    );
                    gungnir_model::events::RhythmEvent::MaintenanceOverrun {
                        sensor,
                        window_closed: now,
                        reason: "see the baseline".into(),
                        at: now,
                    }
                }
                gungnir_model::MaintenanceState::Planned => continue,
            };
            bus.publish(now, Event::Rhythm(event))?;
        }
        tracking.poll(now);

        // GAP-019, edge (s): resolve cross-session identity and journal it. One event per
        // track rather than one per tick -- an identity is a claim about what a track is,
        // and repeating it ten times a second would bury the tracking events it sits
        // beside. The picture is unchanged: `TrackView` carries no entity identity and
        // none was added, because putting one there changes what every consumer of a
        // track believes it is holding, and the record does not need it.
        for event in entities.observe(tracking.tracks(), now) {
            bus.publish(now, Event::Identity(event))?;
        }

        // GAP-066: only a plan computed for this snapshot is proposed. A stale one
        // published as `PlanProposed` would be a recommendation nobody made now, and this
        // node is the system of record for every desktop reading it.
        let outcome = intercept.plan(now, tracking.tracks(), &resources);
        let plan = outcome.plan().cloned().unwrap_or_default();
        if outcome.is_fresh() && plan != last_plan {
            bus.publish(
                now,
                Event::Intercept(InterceptEvent::PlanProposed(plan.clone())),
            )?;
            // GAP-028: the chain runs here too, and the record says which engines ran.
            // Authority is not among them -- it is a question about who is asking, and
            // nobody signs in to a node -- and there is no queue, for the same reason.
            let verdict = evaluate_on_node(&config, &geo, tracking.tracks(), &plan, &resources);
            bus.publish(
                now,
                Event::Intercept(InterceptEvent::PlanEvaluated {
                    plan: plan.id,
                    verdict: verdict.summary(),
                    engines: NODE_ENGINES.iter().map(|e| (*e).to_string()).collect(),
                }),
            )?;
            last_plan = plan;
        }

        for envelope in journal_rx.try_iter() {
            journal.append(session, &envelope)?;
        }
        // The same envelopes reach every connected desktop. Offered after the journal
        // append, so nothing is published to a client that is not yet on disk here --
        // the node is the system of record, and a desktop must never hold an envelope
        // the node could lose.
        for envelope in api_rx.try_iter() {
            if let Err(err) = api.publish_event(envelope) {
                tracing::error!(%err, "could not offer an envelope to the transport");
            }
        }

        let health = SystemHealth {
            tracking_healthy: tracking.is_healthy(),
            intercept_healthy: intercept.is_healthy(),
            ingest_healthy: gateway.is_healthy(),
        };
        if last_health != Some(health) || last_health_log.elapsed() >= HEALTH_LOG_INTERVAL {
            // The registry is reported rather than merely held: a node that built one
            // and never read it would be construction without wiring, which is the
            // thing this gap was open about.
            tracing::info!(
                ?health,
                tracks = tracking.tracks().len(),
                sensors = sensors.sensors().len(),
                covering = sensors.coverage().len(),
                feeds = ?feed_reports.summary(),
                "node health"
            );
            if last_health != Some(health) {
                // MOE-06: the transition is on the record, the periodic log is not.
                bus.publish(
                    now,
                    Event::Health(gungnir_model::events::HealthEvent::Changed {
                        tracking_healthy: health.tracking_healthy,
                        intercept_healthy: health.intercept_healthy,
                        ingest_healthy: health.ingest_healthy,
                        at: now,
                    }),
                )?;
            }
            last_health = Some(health);
            last_health_log = Instant::now();
        }
        api.set_now(now.0);
        publish_picture(
            &api,
            tracking.tracks(),
            tracking.bearing_rays(),
            tracking.pipeline_stats(),
            &last_plan,
            health,
        );
        // Computed here rather than in the request handler, so a caller's polling rate
        // cannot decide this node's load.
        if let Err(err) = api.publish_coverage(coverage_answer(&config, &sensors)) {
            tracing::error!(%err, "could not publish the coverage answer");
        }
        if let Some(gap) = gateway.seconds_since_last_receipt(now) {
            if gap > f64::from(watchdog.max_ingest_gap_s) && !ingest_gap_warned {
                ingest_gap_warned = true;
                tracing::warn!(
                    gap_s = gap,
                    "no detections accepted for longer than the watchdog limit"
                );
            } else if gap <= f64::from(watchdog.max_ingest_gap_s) {
                ingest_gap_warned = false;
            }
        }
    }

    for envelope in journal_rx.try_iter() {
        journal.append(session, &envelope)?;
    }
    // Drained first, then closed. A record marked closed over a journal still missing its
    // last envelopes would claim a completeness it does not have; the other way round, a
    // failure here leaves the session reported as interrupted, which is true of a node
    // that could not finish shutting down.
    JournalMissionManager::open(journal.root(), &journal)?.close(mission)?;
    tracing::info!(
        session = session.0,
        "gungnir-node stopped; journal flushed and session closed"
    );
    Ok(())
}

/// The engines a node runs, in order. Two of the desktop's three: authority needs an
/// asking role and a node has none.
const NODE_ENGINES: [&str; 2] = ["readiness and geofence", "control status"];

/// The policy chain as a node can honestly run it (GAP-028).
fn evaluate_on_node(
    config: &ConfigBaseline,
    geo: &gungnir_geo::InMemoryGeoService,
    tracks: &[gungnir_model::TrackView],
    plan: &PlanView,
    resources: &[gungnir_model::ResourceView],
) -> gungnir_policy::PolicyVerdict {
    let classification = |id: gungnir_model::TrackId| {
        tracks
            .iter()
            .find(|t| t.id == id)
            .map_or(gungnir_model::Classification::Unknown, |t| t.classification)
    };
    let chain = PolicyChain::new(vec![
        Box::new(GeofencePolicy { geo }) as Box<dyn PolicyEngine>,
        Box::new(ControlStatusPolicy {
            settings: &config.policy.control_status,
            track_classification: &classification,
        }),
    ]);
    chain.evaluate(plan, resources)
}

/// The geo service from the baseline's fences (GAP-088). The same conversion as the
/// desktop's `geofences::service_from_config`; the node has no edge to the app.
fn geo_service_from(config: &ConfigBaseline) -> gungnir_geo::InMemoryGeoService {
    gungnir_geo::InMemoryGeoService::new(
        Vec::new(),
        config
            .geofences
            .iter()
            .map(|g| gungnir_geo::Geofence {
                center: gungnir_model::Geodetic {
                    lat_rad: g.center[0],
                    lon_rad: g.center[1],
                    alt_m: g.center[2],
                },
                radius_m: g.radius_m,
                no_go: g.no_go,
            })
            .collect(),
    )
}

/// The node's journal sealing from the baseline's key provider (GAP-060, DN-22 §5).
///
/// The same rule as the desktop's `build_encryption`, and the same sealer: the two
/// binaries share no crate that depends on both `gungnir-store` and `gungnir-security`,
/// so the twenty lines are here too rather than reached through a new edge.
fn seal_journal(
    config: &ConfigBaseline,
    journal: &mut FileEventJournal,
) -> gungnir_security::EncryptionStatus {
    use gungnir_config::KeyProviderConfig;
    use gungnir_security::{EncryptionStatus, KeyPurpose};
    match &config.security.key_provider {
        KeyProviderConfig::None => EncryptionStatus::NotConfigured,
        KeyProviderConfig::Ephemeral => {
            let mut provider = gungnir_security::InProcessKeyProvider::new();
            let key = provider.generate(KeyPurpose::JournalAtRest);
            journal.seal_with(Box::new(EphemeralSealer {
                provider: std::sync::Arc::new(provider),
                key,
            }));
            tracing::warn!(
                "journal encryption uses an ephemeral key: this node's record cannot be \
                 read after the process exits, and a node is a system of record"
            );
            EncryptionStatus::Active {
                provider: "ephemeral".into(),
            }
        }
        other => EncryptionStatus::UnavailableWritingPlaintext {
            reason: format!(
                "the configured key provider is designed and not built ({})",
                other.owning_gap().unwrap_or("GAP-084")
            ),
        },
    }
}

struct EphemeralSealer {
    provider: std::sync::Arc<gungnir_security::InProcessKeyProvider>,
    key: gungnir_security::KeyId,
}

impl gungnir_store::sealing::JournalSealer for EphemeralSealer {
    fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, gungnir_store::StoreError> {
        use gungnir_security::KeyProvider;
        self.provider
            .seal(&self.key, plaintext)
            .map_err(|e| gungnir_store::StoreError::Sealing(e.to_string()))
    }

    fn unseal(&self, sealed: &[u8]) -> Result<Vec<u8>, gungnir_store::StoreError> {
        use gungnir_security::KeyProvider;
        self.provider
            .unseal(&self.key, sealed)
            .map_err(|e| gungnir_store::StoreError::Sealing(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn df_sensor() -> gungnir_config::SensorConfig {
        gungnir_config::SensorConfig {
            id: 21,
            modality: "df".into(),
            position: [0.9, 0.2, 30.0],
            max_range_m: 5_000.0,
            control_endpoint: None,
            maintenance: Vec::new(),
        }
    }

    fn df_feed(bind_addr: &str) -> gungnir_config::RadarFeedConfig {
        gungnir_config::RadarFeedConfig {
            name: "north".into(),
            bind_addr: bind_addr.into(),
            multicast: None,
            radars: Vec::new(),
            df_sites: vec![gungnir_config::DfSiteConfig {
                sensor_id: 21,
                sac: 50,
                sic: 6,
                azimuth_sigma_rad: 1.5_f64.to_radians(),
            }],
            uas_sites: Vec::new(),
        }
    }

    fn uas_sensor() -> gungnir_config::SensorConfig {
        gungnir_config::SensorConfig {
            id: 61,
            modality: "uas-gateway".into(),
            position: [0.9, 0.2, 12.0],
            max_range_m: 20_000.0,
            control_endpoint: None,
            maintenance: Vec::new(),
        }
    }

    /// SAC/SIC `00/00` is what edition 1.2 §5.2.1 recommends for an airborne-to-ground
    /// broadcast, so this is the common single-gateway deployment.
    fn uas_feed(bind_addr: &str) -> gungnir_config::RadarFeedConfig {
        gungnir_config::RadarFeedConfig {
            name: "utm".into(),
            bind_addr: bind_addr.into(),
            multicast: None,
            radars: Vec::new(),
            df_sites: Vec::new(),
            uas_sites: vec![gungnir_config::UasSiteConfig {
                sensor_id: 61,
                sac: 0,
                sic: 0,
            }],
        }
    }

    /// GAP-100: a direction finder named in `RadarFeedConfig::df_sites` reaches
    /// `FeedSpec::df_sites` as the exact `DfBinding` `bind_feed` will hand to
    /// `AsterixFeedAdapter::with_df_sites`, the same derivation this function already
    /// makes for a radar's `RadarBinding`. A feed naming only a direction finder and no
    /// radar still produces one spec.
    #[test]
    fn feed_specs_carries_a_configured_direction_finder_into_the_binding() {
        let config = ConfigBaseline {
            sensors: vec![df_sensor()],
            radar_feeds: vec![df_feed("0.0.0.0:8600")],
            ..ConfigBaseline::default()
        };
        let specs = feed_specs(&config);
        assert_eq!(specs.len(), 1);
        assert!(specs[0].radars.is_empty());
        assert_eq!(
            specs[0].df_sites,
            vec![gungnir_ingest::adapters::asterix::DfBinding {
                sac: 50,
                sic: 6,
                sensor: SensorId(21),
                position: gungnir_model::Geodetic {
                    lat_rad: 0.9,
                    lon_rad: 0.2,
                    alt_m: 30.0,
                },
                azimuth_sigma_rad: 1.5_f64.to_radians(),
            }]
        );
    }

    /// GAP-100, "reachable at start-up": the node's real `build_gateway` -- what `main`
    /// calls to stand the ingest gateway up -- binds a feed that names only a direction
    /// finder and registers it into `FeedReports::radar`, exactly as it already does
    /// for a radar-only feed. `127.0.0.1:0` is a real loopback bind (OS-assigned
    /// port), not a stub, so this proves the node's own construction path actually
    /// runs; `bind_feed_wires_a_configured_direction_finder_into_the_live_adapter` in
    /// `gungnir-ingest` proves what that path builds actually attributes a bearing.
    #[test]
    fn build_gateway_reaches_a_configured_direction_finder_at_start_up() {
        let config = ConfigBaseline {
            sensors: vec![df_sensor()],
            origin: Some([0.9, 0.2, 0.0]),
            radar_feeds: vec![df_feed("127.0.0.1:0")],
            ..ConfigBaseline::default()
        };
        let rt = tokio::runtime::Runtime::new().expect("a throwaway runtime for the handle");
        let (_gateway, _sinks, reports, _sapient) = build_gateway(&config, rt.handle());
        assert_eq!(
            reports.radar.len(),
            1,
            "the one feed, bound for its direction finder alone"
        );
    }

    /// GAP-101: a UAS gateway named in `RadarFeedConfig::uas_sites` reaches
    /// `FeedSpec::uas_sites` as the exact `UasBinding` `bind_feed` will hand to
    /// `AsterixFeedAdapter::with_uas_sites`, the same derivation this function already
    /// makes for a radar's `RadarBinding` -- minus the position, which this category
    /// takes from the report itself rather than from the receiving antenna. A feed
    /// naming only a UAS gateway still produces one spec.
    #[test]
    fn feed_specs_carries_a_configured_uas_gateway_into_the_binding() {
        let config = ConfigBaseline {
            sensors: vec![uas_sensor()],
            radar_feeds: vec![uas_feed("0.0.0.0:8601")],
            ..ConfigBaseline::default()
        };
        let specs = feed_specs(&config);
        assert_eq!(specs.len(), 1);
        assert!(specs[0].radars.is_empty());
        assert!(specs[0].df_sites.is_empty());
        assert_eq!(
            specs[0].uas_sites,
            vec![gungnir_ingest::adapters::asterix::UasBinding {
                sac: 0,
                sic: 0,
                sensor: SensorId(61),
            }]
        );
    }

    /// GAP-101, "reachable at start-up": the node's real `build_gateway` binds a feed
    /// that names only a UAS gateway and registers it into `FeedReports::radar`, exactly
    /// as it already does for a radar-only and a direction-finder-only feed.
    /// `127.0.0.1:0` is a real loopback bind (OS-assigned port), not a stub;
    /// `bind_feed_wires_a_configured_uas_gateway_into_the_live_adapter` in
    /// `gungnir-ingest` proves what that path builds actually attributes a report.
    #[test]
    fn build_gateway_reaches_a_configured_uas_gateway_at_start_up() {
        let config = ConfigBaseline {
            sensors: vec![uas_sensor()],
            origin: Some([0.9, 0.2, 0.0]),
            radar_feeds: vec![uas_feed("127.0.0.1:0")],
            ..ConfigBaseline::default()
        };
        let rt = tokio::runtime::Runtime::new().expect("a throwaway runtime for the handle");
        let (_gateway, _sinks, reports, _sapient) = build_gateway(&config, rt.handle());
        assert_eq!(
            reports.radar.len(),
            1,
            "the one feed, bound for its UAS gateway alone"
        );
    }

    /// GAP-060: the node's outbound peer-link identity is provider-issued, the same as
    /// its serving identity, rather than only ever coming from the environment.
    #[test]
    fn host_tls_issues_its_own_identity_and_carries_the_configured_trust_roots() {
        let config = ConfigBaseline {
            security: gungnir_config::SecurityConfig {
                tls: gungnir_config::TlsClientConfig {
                    trust_roots_pem: vec![
                        "-----BEGIN CERTIFICATE-----fake-----END CERTIFICATE-----".into(),
                    ],
                },
                ..gungnir_config::SecurityConfig::default()
            },
            ..ConfigBaseline::default()
        };
        let tls = host_tls(&config);
        assert!(
            tls.issued.is_some(),
            "an ephemeral provider should always be able to issue"
        );
        assert!(tls.has_identity());
        assert_eq!(tls.trust_roots_pem, config.security.tls.trust_roots_pem);
    }

    /// GAP-004, both directions. Outbound: a SAPIENT feed with a `destination_id` gets
    /// a task adapter whose sink writes onto its own connection, attached to the
    /// registry and reachable through `SensorControl::issue` -- the whole path from
    /// configuration to a real socket, not just the pieces in isolation. Inbound: a
    /// `TaskAck` against the wire `taskId` that issue produced is acknowledged and
    /// published as `SensorTaskEvent::Acknowledged`.
    ///
    /// Long on purpose: splitting outbound from inbound would duplicate the whole
    /// setup (listener, config, gateway, registry, adapter) just to lose the one
    /// thing worth proving together -- that the ack is applied against the *same*
    /// wire task id the issue actually produced, never a hand-encoded one.
    #[allow(clippy::too_many_lines)]
    #[test]
    fn a_taskable_sapient_feed_issues_a_real_task_and_applies_its_ack() {
        use gungnir_sensor_management::SensorControl;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let accepted = std::thread::spawn(move || {
            use std::io::{BufRead, BufReader};
            let (stream, _) = listener.accept().expect("accept");
            let mut line = String::new();
            BufReader::new(stream).read_line(&mut line).expect("read");
            line
        });

        let config = ConfigBaseline {
            sensors: vec![gungnir_config::SensorConfig {
                id: 7,
                modality: "sapient".into(),
                position: [0.9, 0.2, 2.0],
                max_range_m: 5_000.0,
                control_endpoint: Some("sapient".into()),
                maintenance: Vec::new(),
            }],
            origin: Some([0.9, 0.2, 0.0]),
            sapient_feeds: vec![gungnir_config::SapientFeedConfig {
                name: "spotter-7".into(),
                sensor_id: 7,
                node_type: gungnir_config::SapientNodeType::Spotter,
                source: gungnir_config::SapientSource::Tcp {
                    addr: addr.to_string(),
                },
                destination_id: Some("3fa85f64-5717-4562-b3fc-2c963f66afa6".into()),
            }],
            sapient_node_id: Some("gungnir-node-test".into()),
            ..ConfigBaseline::default()
        };

        let mut gateway = IngestGateway::new(Box::new(AllowListAuthenticator {
            allowed: vec![SensorId(7)],
        }));
        let bound = bind_sapient_feeds(&config, &mut gateway, &[]);
        assert_eq!(bound.task_adapters.len(), 1, "the one taskable feed");

        let mut sensors = build_registry(&config);
        sensors.attach_adapter(Arc::new(SapientTaskRouter {
            by_sensor: bound.task_adapters.into_iter().collect(),
        }));
        let local_task = sensors
            .issue(
                SensorId(7),
                gungnir_model::SensorCommand::SetMode {
                    mode: gungnir_model::SensorMode::Search,
                },
                None,
                gungnir_model::MissionTime(1_000.0),
            )
            .expect("issued");

        let line = accepted.join().expect("thread");
        let value: serde_json::Value = serde_json::from_str(line.trim()).expect("valid json");
        assert_eq!(value["nodeId"], "gungnir-node-test");
        assert_eq!(
            value["destinationId"],
            "3fa85f64-5717-4562-b3fc-2c963f66afa6"
        );
        assert_eq!(value["task"]["command"]["modeChange"], "search");

        // The other direction: a TaskAck against the wire taskId this issue itself
        // produced (never hand-encoded), the same rule
        // `gungnir-app/tests/sapient_task_ack.rs` already follows.
        let wire_task_id = value["task"]["taskId"]
            .as_str()
            .expect("taskId")
            .to_string();
        let ack_sink = gungnir_ingest::adapters::sapient::TaskAckSink::default();
        ack_sink.lock().expect("lock").push_back(
            gungnir_ingest::adapters::sapient::TaskAckReport {
                task_id: wire_task_id,
                status: gungnir_ingest::adapters::sapient::TaskAckStatus::Accepted,
                reasons: Vec::new(),
            },
        );
        let bus = InProcessBus::new();
        let events = bus.subscribe();
        apply_sapient_task_acks(
            &[ack_sink],
            &mut sensors,
            &bus,
            gungnir_model::MissionTime(1_001.0),
        )
        .expect("applied");

        let task = sensors
            .tasks()
            .iter()
            .find(|t| t.id == local_task)
            .expect("recorded");
        assert!(
            matches!(
                task.state,
                gungnir_sensor_management::tasking::TaskState::Acknowledged { .. }
            ),
            "{:?}",
            task.state
        );
        match events.try_recv().expect("published").event {
            Event::SensorTask(gungnir_model::events::SensorTaskEvent::Acknowledged {
                task,
                sensor,
                ..
            }) => {
                assert_eq!(task, local_task);
                assert_eq!(sensor, SensorId(7));
            }
            other => panic!("expected an Acknowledged event: {other:?}"),
        }
    }

    /// A `TrackingService` this test does not otherwise need: `IngestGateway::tick`
    /// requires one to poll adapters at all, and what it does with a detection is not
    /// this test's concern -- only that polling happens, so the SAPIENT adapter reads
    /// the wire.
    struct NoTrackingService;
    impl gungnir_tracking_service::TrackingService for NoTrackingService {
        fn submit_detection(
            &mut self,
            _detection: gungnir_model::DetectionView,
        ) -> Result<(), gungnir_tracking_service::SubmitError> {
            Ok(())
        }
        fn poll(&mut self, _now: gungnir_model::MissionTime) {}
        fn tracks(&self) -> &[gungnir_model::TrackView] {
            &[]
        }
        fn is_healthy(&self) -> bool {
            true
        }
    }

    /// **The SAPIENT loopback fixture US-09 needs** (`docs/ux/usability-round-1-
    /// session.md`), proven against the real adapter rather than only written down.
    ///
    /// The test above proves the wire `taskId` is genuine by reading it off the
    /// socket, then applies the ack by building a `TaskAckReport` directly and
    /// pushing it into the sink -- `gungnir-app/tests/sapient_task_ack.rs` takes the
    /// same shortcut. Neither ever writes a real `TaskAck` message back over the
    /// wire, so `SapientDetectionAdapter::handle`'s own parsing of one
    /// (`handle_task_ack`, `parse_task_ack`, reached through `take_messages`) is
    /// never actually exercised end to end -- only the *application* of an
    /// already-parsed ack is. Here the "sensor" writes a genuine `TaskAck` line back,
    /// and `IngestGateway::tick` -- not a hand-populated sink -- is what puts it in
    /// front of `apply_sapient_task_acks`. This is also, line for line, what a
    /// standalone SAPIENT loopback fixture does: read a `Task`, answer with an
    /// `Accepted` `TaskAck` naming the same `taskId`.
    ///
    /// **The connection must stay open**, which the first draft of this test got
    /// wrong: closing the "sensor" side right after writing the ack made
    /// `TcpSapientSource::take_messages` see end-of-file within the same read loop
    /// that had just buffered the ack bytes, and it treats that as "the middleware
    /// closed the connection" -- discarding what it had already buffered rather than
    /// returning it, correctly, for a genuine mid-session disconnect. A real sensor
    /// (or a real loopback fixture) keeps its connection open for the session, so
    /// this one does too.
    #[allow(clippy::too_many_lines)]
    #[test]
    fn a_task_ack_written_back_on_the_wire_is_read_by_the_real_adapter_and_applied() {
        use gungnir_sensor_management::SensorControl;
        use std::io::{BufRead, BufReader, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        // Accepts and reads the task on its own thread, but returns the live stream
        // rather than dropping it: `TcpSapientSource::take_messages` treats a read
        // that reaches EOF as "the middleware closed the connection" and discards
        // whatever it had already buffered in the same call, exactly the way a real
        // persistent SAPIENT connection would report a genuine disconnect. A
        // real (or loopback-fixture) sensor keeps its connection open, so this one
        // does too -- the ack is written from the main thread below, and the stream
        // stays alive for the rest of the test.
        let sensor = std::thread::spawn(move || -> (std::net::TcpStream, String) {
            let (stream, _) = listener.accept().expect("accept");
            let mut line = String::new();
            BufReader::new(stream.try_clone().expect("clone a read handle"))
                .read_line(&mut line)
                .expect("read the task");
            let value: serde_json::Value = serde_json::from_str(line.trim()).expect("valid json");
            let wire_task_id = value["task"]["taskId"]
                .as_str()
                .expect("taskId")
                .to_string();
            (stream, wire_task_id)
        });

        let config = ConfigBaseline {
            sensors: vec![gungnir_config::SensorConfig {
                id: 9,
                modality: "sapient".into(),
                position: [0.9, 0.2, 2.0],
                max_range_m: 5_000.0,
                control_endpoint: Some("sapient".into()),
                maintenance: Vec::new(),
            }],
            origin: Some([0.9, 0.2, 0.0]),
            sapient_feeds: vec![gungnir_config::SapientFeedConfig {
                name: "loopback-9".into(),
                sensor_id: 9,
                node_type: gungnir_config::SapientNodeType::Spotter,
                source: gungnir_config::SapientSource::Tcp {
                    addr: addr.to_string(),
                },
                destination_id: Some("3fa85f64-5717-4562-b3fc-2c963f66afa6".into()),
            }],
            sapient_node_id: Some("gungnir-node-test".into()),
            ..ConfigBaseline::default()
        };

        let mut gateway = IngestGateway::new(Box::new(AllowListAuthenticator {
            allowed: vec![SensorId(9)],
        }));
        let bound = bind_sapient_feeds(&config, &mut gateway, &[]);
        assert_eq!(bound.task_adapters.len(), 1, "the one taskable feed");
        assert_eq!(bound.task_ack_sinks.len(), 1, "its ack sink");

        let mut sensors = build_registry(&config);
        sensors.attach_adapter(Arc::new(SapientTaskRouter {
            by_sensor: bound.task_adapters.into_iter().collect(),
        }));
        let local_task = sensors
            .issue(
                SensorId(9),
                gungnir_model::SensorCommand::SetMode {
                    mode: gungnir_model::SensorMode::Search,
                },
                None,
                gungnir_model::MissionTime(2_000.0),
            )
            .expect("issued");

        let (mut sensor_stream, wire_task_id) = sensor.join().expect("thread");
        // The genuine wire message a real (or loopback-fixture) SAPIENT sensor
        // answers with -- never hand-built into a `TaskAckReport` and pushed into
        // the sink directly, the shortcut every other test in this file and in
        // `sapient_task_ack.rs` takes. `sensor_stream` stays alive (bound to this
        // name, not dropped) for the rest of the test, so the connection is still
        // open when the gateway polls it below.
        let ack = serde_json::json!({
            "nodeId": "sapient-loopback-fixture",
            "taskAck": {
                "taskId": wire_task_id,
                "taskStatus": "TASK_STATUS_ACCEPTED",
            },
        });
        sensor_stream
            .write_all(format!("{ack}\n").as_bytes())
            .expect("write the ack");

        // The real path: the gateway polls the adapter, and the adapter parses the
        // wire message itself. Nothing here constructs a `TaskAckReport` by hand.
        // `TcpSapientSource` reads a non-blocking socket, so the bytes the sensor
        // thread already wrote (and joined on) can still be a tick or two from
        // showing up on this side of a real loopback connection; retried briefly
        // rather than ticked once, the same way the real node's own timer-driven
        // loop would tick again rather than assume one poll must see everything.
        let mut applied = false;
        for _ in 0..50 {
            gateway.tick(gungnir_model::MissionTime(2_001.0), &mut NoTrackingService);
            if !bound.task_ack_sinks[0].lock().expect("lock").is_empty() {
                applied = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            applied,
            "the wire ack for {wire_task_id} never reached the sink after retrying"
        );

        let bus = InProcessBus::new();
        let events = bus.subscribe();
        apply_sapient_task_acks(
            &bound.task_ack_sinks,
            &mut sensors,
            &bus,
            gungnir_model::MissionTime(2_002.0),
        )
        .expect("applied");

        let task = sensors
            .tasks()
            .iter()
            .find(|t| t.id == local_task)
            .expect("recorded");
        assert!(
            matches!(
                task.state,
                gungnir_sensor_management::tasking::TaskState::Acknowledged { .. }
            ),
            "the genuine wire ack for {wire_task_id} was not applied: {:?}",
            task.state
        );
        match events.try_recv().expect("published").event {
            Event::SensorTask(gungnir_model::events::SensorTaskEvent::Acknowledged {
                task,
                sensor,
                ..
            }) => {
                assert_eq!(task, local_task);
                assert_eq!(sensor, SensorId(9));
            }
            other => panic!("expected an Acknowledged event: {other:?}"),
        }
    }

    /// MISB ST 0601's UAS Datalink LS key (`gungnir_interop::misb0601::UDS_KEY`),
    /// copied here because this binary has no dependency edge on `gungnir-interop`
    /// (`ARCHITECTURE.md` draws none) and the key is not re-exported through
    /// `gungnir_ingest::adapters::misb`.
    const MISB_UDS_KEY: [u8; 16] = [
        0x06, 0x0E, 0x2B, 0x34, 0x02, 0x0B, 0x01, 0x01, 0x0E, 0x01, 0x03, 0x01, 0x01, 0x00, 0x00,
        0x00,
    ];

    /// MISB ST 0601.8-08's checksum algorithm
    /// (`gungnir_interop::misb0601::packet_checksum`), duplicated for the same reason
    /// as [`MISB_UDS_KEY`] above: the lower 16 bits of the sum of 16-bit big-endian
    /// words over `packet`, excluding `packet`'s own trailing two bytes.
    fn misb_packet_checksum(packet: &[u8]) -> u16 {
        let summed = &packet[..packet.len() - 2];
        let mut total: u32 = 0;
        let mut pos = 0usize;
        while pos + 2 <= summed.len() {
            total = total.wrapping_add(u32::from(u16::from_be_bytes([
                summed[pos],
                summed[pos + 1],
            ])));
            pos += 2;
        }
        if pos < summed.len() {
            total = total.wrapping_add(u32::from(summed[pos]) << 8);
        }
        u16::try_from(total & 0xFFFF).unwrap_or(0)
    }

    /// A well-formed KLV frame carrying a real platform position: the same Sensor
    /// Latitude/Longitude raw bytes `gungnir-ingest/tests/misb_feed.rs` uses, copied
    /// verbatim from the vendored fixture (`testdata/misb/SOURCE.md`) and already
    /// pinned by `misb0601_fixtures.rs` as decoding to 60.176822966978335 /
    /// 128.42675904204452 -- reused rather than re-derived so this test needs no
    /// second, untested implementation of MISB's "mapped" encoding. Proves the
    /// config-to-gateway wiring; the codec and the adapter are gated elsewhere.
    fn misb_well_formed_frame() -> Vec<u8> {
        let items: [(u8, &[u8]); 2] = [
            (13, &[0x55, 0x95, 0xB6, 0x6D]),
            (14, &[0x5B, 0x53, 0x60, 0xC4]),
        ];
        let mut value = Vec::new();
        for (tag, bytes) in items {
            value.push(tag);
            value.push(u8::try_from(bytes.len()).expect("short-form"));
            value.extend_from_slice(bytes);
        }
        value.push(1);
        value.push(2);
        let checksum_at = MISB_UDS_KEY.len() + 1 + value.len();
        value.push(0);
        value.push(0);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MISB_UDS_KEY);
        bytes.push(u8::try_from(value.len()).expect("short-form"));
        bytes.extend_from_slice(&value);
        let cs = misb_packet_checksum(&bytes).to_be_bytes();
        bytes[checksum_at] = cs[0];
        bytes[checksum_at + 1] = cs[1];
        bytes
    }

    /// GAP-099: a configured `misb_feeds` entry is bound into the real gateway at
    /// start and its platform position reaches the gateway as an accepted detection,
    /// the same standard `gungnir-ingest/tests/misb_feed.rs` already holds the
    /// adapter to on its own -- this proves the config-to-`bind_misb_feeds` half.
    #[test]
    fn a_configured_misb_feed_is_bound_and_its_position_reaches_the_gateway() {
        let origin = [60.0_f64.to_radians(), 128.0_f64.to_radians(), 0.0];
        let dir = std::env::temp_dir().join(format!("gungnir-node-misb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let recording = dir.join("frame.klv");
        std::fs::write(&recording, misb_well_formed_frame()).expect("recording");

        let config = ConfigBaseline {
            sensors: vec![gungnir_config::SensorConfig {
                id: 40,
                modality: "misb".into(),
                position: origin,
                max_range_m: 50_000.0,
                control_endpoint: None,
                maintenance: Vec::new(),
            }],
            origin: Some(origin),
            misb_feeds: vec![gungnir_config::MisbFeedConfig {
                name: "uas-1".into(),
                sensor_id: 40,
                source: gungnir_config::MisbSource::File {
                    path: recording.to_string_lossy().into_owned(),
                },
            }],
            ..ConfigBaseline::default()
        };
        gungnir_config::validate(&config).expect("valid");

        let mut gateway = IngestGateway::new(Box::new(AllowListAuthenticator {
            allowed: vec![SensorId(40)],
        }));
        bind_misb_feeds(&config, &mut gateway, &[]);
        gateway.set_expected_adapters(1);

        let events = gateway.tick(gungnir_model::MissionTime(1_000.0), &mut NoTrackingService);
        assert_eq!(events.len(), 1, "{events:#?}");
        assert!(
            matches!(
                &events[0],
                gungnir_model::events::IngestEvent::Accepted(d) if d.sensor == SensorId(40)
            ),
            "{events:#?}"
        );
        assert_eq!(gateway.stats().accepted, 1);
        assert_eq!(gateway.stats().quarantined, 0);

        let _ = std::fs::remove_dir_all(dir);
    }

    /// The mirror image of the acceptance test above: no `origin` means no local
    /// frame to place a platform position in, so the feed is refused with a warning
    /// rather than bound and silently producing nothing -- matching AIS's and
    /// ADS-B's own no-origin behaviour above.
    #[test]
    fn a_misb_feed_with_no_local_frame_origin_binds_nothing() {
        let config = ConfigBaseline {
            sensors: vec![gungnir_config::SensorConfig {
                id: 41,
                modality: "misb".into(),
                position: [0.9, 0.2, 0.0],
                max_range_m: 50_000.0,
                control_endpoint: None,
                maintenance: Vec::new(),
            }],
            origin: None,
            misb_feeds: vec![gungnir_config::MisbFeedConfig {
                name: "uas-2".into(),
                sensor_id: 41,
                source: gungnir_config::MisbSource::File {
                    path: "does-not-matter.bin".into(),
                },
            }],
            ..ConfigBaseline::default()
        };

        let mut gateway = IngestGateway::new(Box::new(AllowListAuthenticator {
            allowed: vec![SensorId(41)],
        }));
        bind_misb_feeds(&config, &mut gateway, &[]);
        let events = gateway.tick(gungnir_model::MissionTime(1_000.0), &mut NoTrackingService);
        assert!(events.is_empty(), "{events:#?}");
    }
}

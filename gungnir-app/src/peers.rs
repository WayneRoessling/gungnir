//! Peer links on the desktop (GAP-009, DN-16; the inbound half of GAP-065, DN-18).
//!
//! One machine link per configured peer whose endpoint is a partner's node, over the
//! baseline's trust roots and this desktop's certificate (`session::link_tls`). What
//! arrives is admitted by the gateway under the peer's own source id with the quality
//! this deployment assigns, through `PeerSourceAdapter`, never as if it were our own
//! sensor's. A peer whose endpoint is not a node is said in the alerts and skipped.
//!
//! # Launch warnings (DN-16 §5)
//!
//! A peer's launch warning arrives on the same link and is **not** a track. DN-16 §5:
//! "it raises an alert with the peer named, and it never creates a track". So [`tick`]
//! drains each peer's launch-warning sink every frame and does exactly two things with
//! what it finds -- puts a line in the alert list, and puts the fact on this session's
//! record -- and nothing anywhere turns one into a `DetectionView`, which is the only
//! way a track could appear.
//!
//! A quarantined warning raises an alert too. A peer sending warnings this deployment
//! cannot read is a fault an operator has to be able to see; silence would look exactly
//! like a peer that saw nothing.

use gungnir_config::ConfigBaseline;
use gungnir_ingest::adapters::peer::{
    LaunchWarningOutcome, LaunchWarningSink, PeerSourceAdapter, PeerStream,
};
use gungnir_ingest::IngestGateway;
use gungnir_model::events::LaunchWarningEvent;
use gungnir_model::{LaunchWarningReport, SensorId, TrackView};
use gungnir_remote::peer::PeerLink;

use crate::state::AppState;

/// The adapter's stream over the link `gungnir-remote` keeps up.
pub struct LinkPeerStream {
    link: PeerLink,
}

impl PeerStream for LinkPeerStream {
    fn take_tracks(&mut self) -> Vec<TrackView> {
        self.link.take_tracks()
    }

    fn take_launch_warnings(&mut self) -> Vec<LaunchWarningReport> {
        self.link.take_launch_warnings()
    }

    fn describe(&self) -> String {
        format!("link:{}", self.link.endpoint())
    }
}

/// A bound peer, for PN-09.
#[derive(Debug, Clone)]
pub struct BoundPeer {
    pub name: String,
    pub link: PeerLink,
    /// Where this peer's launch warnings land (DN-16 §5), drained by [`tick`].
    pub launch_warnings: LaunchWarningSink,
}

/// The peer lines for PN-09, owned so the view can borrow them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedPeerLine {
    pub name: String,
    pub endpoint: String,
    pub connected: bool,
    pub reason: String,
}

#[must_use]
pub fn peer_lines(peers: &[BoundPeer]) -> Vec<OwnedPeerLine> {
    peers
        .iter()
        .map(|p| OwnedPeerLine {
            name: p.name.clone(),
            endpoint: p.link.endpoint().to_string(),
            connected: p.link.connected(),
            reason: p
                .link
                .last_error()
                .unwrap_or_else(|| "no answer yet".to_string()),
        })
        .collect()
}

/// Bind a link per configured peer whose endpoint is a node.
pub fn bind_peers(
    config: &ConfigBaseline,
    ingest: &mut IngestGateway,
    tls: &gungnir_remote::LinkTls,
    handle: &tokio::runtime::Handle,
    alerts: &mut Vec<String>,
) -> Vec<BoundPeer> {
    let mut bound = Vec::new();
    for peer in &config.peers {
        let Some(endpoint) = config.endpoints.iter().find(|e| e.name == peer.endpoint) else {
            continue;
        };
        if endpoint.kind != "peer" || !endpoint.address.starts_with("http") {
            alerts.push(format!(
                "peer {}: endpoint {} is a {:?} at {}, not a node url; no link bound",
                peer.name, endpoint.name, endpoint.kind, endpoint.address
            ));
            continue;
        }
        let remote = gungnir_remote::RemoteEndpoint {
            url: endpoint.address.clone(),
            tls: tls.clone(),
        };
        match PeerLink::connect(&remote, handle) {
            Ok(link) => {
                let launch_warnings = LaunchWarningSink::default();
                ingest.add_adapter(Box::new(
                    PeerSourceAdapter::new(
                        peer.name.clone(),
                        SensorId(peer.source_id),
                        peer.assigned_quality,
                        peer.max_age_s,
                        LinkPeerStream { link: link.clone() },
                    )
                    .with_launch_warning_sink(launch_warnings.clone()),
                ));
                bound.push(BoundPeer {
                    name: peer.name.clone(),
                    link,
                    launch_warnings,
                });
            }
            Err(err) => alerts.push(format!("peer {}: link not bound: {err}", peer.name)),
        }
    }
    bound
}

/// Drain every peer's launch warnings into the alert list and the record (DN-16 §5).
///
/// Called once per frame from [`crate::update::tick`], after the gateway has polled the
/// adapters that fill the sinks. Two effects and no third: an alert an operator can see,
/// and an envelope the journal keeps. **No track is created**, which is the criterion
/// DN-16 §8 states for CAP-1.6.
pub fn tick(state: &mut AppState) {
    if state.peer_links.is_empty() {
        return;
    }
    let now = state.clock.now();
    let mut drained: Vec<LaunchWarningOutcome> = Vec::new();
    for peer in &state.peer_links {
        if let Ok(mut queue) = peer.launch_warnings.lock() {
            drained.extend(queue.drain(..));
        }
    }
    for outcome in drained {
        match outcome {
            LaunchWarningOutcome::Admitted(warning) => {
                state.alerts.push(warning.alert_summary());
                crate::update::publish(
                    state,
                    now,
                    gungnir_eventing::Event::LaunchWarning(LaunchWarningEvent::Received(warning)),
                );
            }
            LaunchWarningOutcome::Quarantined { peer, reason, at } => {
                state.alerts.push(format!(
                    "peer {peer} sent a launch warning this deployment refused: {reason}"
                ));
                crate::update::publish(
                    state,
                    now,
                    gungnir_eventing::Event::LaunchWarning(LaunchWarningEvent::Refused {
                        peer,
                        reason,
                        at,
                    }),
                );
            }
        }
    }
}

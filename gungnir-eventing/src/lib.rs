//! Transport-neutral event bus, per docs/gungnir-capabilities.md §5.1.
//! `gungnir-tracking-service`/`gungnir-intercept-service` publish here instead of
//! (or in addition to) exposing a pollable snapshot; `gungnir-store`, `gungnir-api`,
//! `gungnir-observability`, and the UI all subscribe from the same bus so they never
//! disagree about "what changed and when."
//!
//! Delivery is **broadcast**: every subscriber receives every envelope published
//! after it subscribed, in publication order, each with a bus-wide monotonic
//! sequence number. (An earlier scaffold handed all subscribers one shared receiver,
//! which was a work queue; that is fixed here, ARCHITECTURE.md §10 resolved item 4.)

use crossbeam_channel::{unbounded, Sender};
use gungnir_model::MissionTime;

/// Re-exported so subscribers need no direct `crossbeam-channel` dependency.
pub use crossbeam_channel::Receiver;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

pub use gungnir_model::events::{
    CommandEvent, EngagementEvent, GovernanceEvent, HandoffEvent, HealthEvent, IngestEvent,
    InterceptEvent, LaunchWarningEvent, ReplayEvent, RequirementEvent, ReviewEvent, RhythmEvent,
    SensorEvent, SensorTaskEvent, TrackingEvent,
};

/// Everything the bus can carry. The payload enums are owned by `gungnir-model` so
/// that the journal, the API, and replay all serialize the same shapes.
///
/// The tracking variants carry a full `TrackView` (a 6x6 covariance plus provenance)
/// while others carry an id; boxing the large ones would add a heap allocation per
/// subscriber per publish on the hottest path for no memory saving, since a
/// `TrackView` is the common case, so the size difference is accepted deliberately.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Event {
    /// What a sensor is doing changed (GAP-003).
    Sensor(SensorEvent),
    /// What was asked of a sensor, and what came back (GAP-004). Distinct from
    /// [`Event::Sensor`], which records what a sensor is actually doing.
    SensorTask(SensorTaskEvent),
    /// The life of a collection requirement (GAP-005): what somebody needed to know,
    /// who concurred with tasking it, and whether it was answered.
    Requirement(RequirementEvent),
    /// The watch's rhythm (GAP-054): scheduled products, handover, planned downtime.
    Rhythm(RhythmEvent),
    /// Which algorithm configuration is in force, and how it got there (GAP-086).
    Governance(GovernanceEvent),
    /// An engagement opened on a decision and closed on evidence, or on the lack of it
    /// (GAP-043, DN-06).
    Engagement(EngagementEvent),
    /// An after-action review and its findings (GAP-049, DN-20).
    Review(ReviewEvent),
    /// System health as it changed (GAP-047, MOE-06).
    Health(HealthEvent),
    /// A session replayed on this desktop (GAP-047, MOE-12).
    Replay(ReplayEvent),
    /// A handoff issued from a decision, and what became of its delivery (GAP-040).
    Handoff(HandoffEvent),
    /// A warning owed to an asset, and what became of it (GAP-042, DN-03).
    Warning(gungnir_model::events::WarningEvent),
    /// What a track was taken to be across sessions, and why (GAP-019, DN-19).
    ///
    /// Additive, so `SCHEMA_VERSION` is not bumped -- the same treatment
    /// [`Event::Link`] and the launch-warning variant had.
    Identity(gungnir_model::events::IdentityEvent),
    /// A peer's statement that something has been launched (GAP-009, DN-16 §5).
    ///
    /// Distinct from [`Event::Warning`] because it is a different fact from a different
    /// party: DN-03's warning is an obligation this deployment owes an asset, and this
    /// is a message a peer sent us. Distinct from [`Event::Tracking`] because DN-16 §5
    /// says a launch warning "never creates a track", and the only structural way to
    /// keep that promise is for it not to be one.
    LaunchWarning(LaunchWarningEvent),
    /// A seeded session for a usability round; the first record of one (GAP-089).
    Rehearsal(gungnir_model::events::RehearsalEvent),
    /// The node link fell back or was restored (GAP-050).
    Link(gungnir_model::events::LinkEvent),
    Tracking(TrackingEvent),
    Intercept(InterceptEvent),
    Ingest(IngestEvent),
    Command(CommandEvent),
}

/// An event as delivered and journaled: sequence number, mission time at
/// publication, payload. `seq` is unique and increasing per bus instance; the
/// journal preserves it so replay can detect gaps.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Envelope {
    pub seq: u64,
    pub mission_time: MissionTime,
    pub event: Event,
}

#[derive(Debug, thiserror::Error)]
pub enum EventingError {
    /// A publisher panicked while holding the subscriber list; the bus recovered
    /// the list but reports it so the caller can decide whether to trust it.
    #[error("event bus subscriber list was poisoned by a panicking publisher")]
    Poisoned,
}

/// Implementations: [`InProcessBus`] (crossbeam channels, default today) and a
/// future durable or networked bus both implement this same trait, so publishers
/// and subscribers never depend on which transport is active.
pub trait EventBus: Send + Sync {
    /// Publish `event` stamped with `mission_time`; returns the sequence number
    /// assigned. Publishing with no subscribers succeeds and is a no-op.
    fn publish(&self, mission_time: MissionTime, event: Event) -> Result<u64, EventingError>;

    /// Register a new subscriber. It receives every envelope published from now on.
    fn subscribe(&self) -> Receiver<Envelope>;
}

/// Broadcast bus for a single process. Each subscriber owns its own unbounded
/// channel; publishers clone the envelope into each. Subscribers that drop their
/// receiver are pruned on the next publish.
#[derive(Debug, Default)]
pub struct InProcessBus {
    subscribers: Mutex<Vec<Sender<Envelope>>>,
    next_seq: AtomicU64,
}

impl InProcessBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of live subscribers as of the last publish (dropped receivers are
    /// only detected when a publish fails to reach them).
    pub fn subscriber_count(&self) -> usize {
        self.subscribers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

impl EventBus for InProcessBus {
    fn publish(&self, mission_time: MissionTime, event: Event) -> Result<u64, EventingError> {
        let mut subscribers = self
            .subscribers
            .lock()
            .map_err(|_| EventingError::Poisoned)?;
        // Sequence numbers are taken under the lock so that concurrent publishers
        // deliver in a single global order to every subscriber.
        let seq = self.next_seq.fetch_add(1, Ordering::SeqCst);
        let envelope = Envelope {
            seq,
            mission_time,
            event,
        };
        subscribers.retain(|tx| tx.send(envelope.clone()).is_ok());
        Ok(seq)
    }

    fn subscribe(&self) -> Receiver<Envelope> {
        let (tx, rx) = unbounded();
        self.subscribers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(tx);
        rx
    }
}

#[cfg(test)]
#[allow(clippy::cast_precision_loss)]
mod tests {
    use super::*;
    use gungnir_model::TrackId;

    fn deleted(id: u64) -> Event {
        Event::Tracking(TrackingEvent::TrackDeleted(TrackId(id)))
    }

    #[test]
    fn every_subscriber_receives_every_event_in_order() {
        let bus = InProcessBus::new();
        let subs: Vec<_> = (0..3).map(|_| bus.subscribe()).collect();
        for i in 0..5 {
            bus.publish(MissionTime(i as f64), deleted(i))
                .expect("publish");
        }
        for rx in &subs {
            let got: Vec<Envelope> = rx.try_iter().collect();
            assert_eq!(got.len(), 5);
            for (i, env) in got.iter().enumerate() {
                assert_eq!(env.seq, i as u64);
                assert_eq!(env.event, deleted(i as u64));
                assert_eq!(env.mission_time, MissionTime(i as f64));
            }
        }
    }

    #[test]
    fn late_subscriber_sees_only_later_events() {
        let bus = InProcessBus::new();
        bus.publish(MissionTime(0.0), deleted(1)).expect("publish");
        let rx = bus.subscribe();
        bus.publish(MissionTime(1.0), deleted(2)).expect("publish");
        let got: Vec<Envelope> = rx.try_iter().collect();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].seq, 1);
    }

    #[test]
    fn dropped_subscribers_are_pruned() {
        let bus = InProcessBus::new();
        let keep = bus.subscribe();
        let gone = bus.subscribe();
        drop(gone);
        assert_eq!(bus.subscriber_count(), 2);
        bus.publish(MissionTime(0.0), deleted(1)).expect("publish");
        assert_eq!(bus.subscriber_count(), 1);
        assert_eq!(keep.try_iter().count(), 1);
    }

    #[test]
    fn publish_without_subscribers_is_ok() {
        let bus = InProcessBus::new();
        assert_eq!(
            bus.publish(MissionTime(0.0), deleted(1)).expect("publish"),
            0
        );
    }
}

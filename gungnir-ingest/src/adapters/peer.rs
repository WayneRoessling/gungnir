//! A peer node as a source (GAP-009, docs/design/DN-16-peer-sources.md).
//!
//! **A peer is a source, so it goes through the gateway**: its tracks arrive here as
//! detections with the peer named on the provenance, the age visible, and the quality
//! **assigned by our configuration, never claimed by the peer** (DN-16 §5, the rule the
//! note calls the most important). Beyond `max_age_s` a track is still emitted -- never
//! silently discarded -- with the staleness stated on the provenance so fusion treats it
//! as the stale rule already does.
//!
//! The transport is injected. This crate may not depend on `gungnir-remote`, and a peer
//! link needs a machine identity to sign in with (D-02, GAP-060), which no host holds
//! yet; so the adapter is built and tested against an in-memory stream, and the host
//! wiring waits on that identity.
//!
//! # Launch warnings (DN-16 §5, GAP-009)
//!
//! **A launch warning arrives on this same path and is never a detection.** DN-16 §5:
//! "a warning is a statement about the future with no kinematic state ... it never
//! creates a track: a track we have not observed is a track we cannot maintain." So the
//! adapter validates and stamps one exactly as it does a peer track -- our name for the
//! peer, our receipt time, quarantine with a reason for anything malformed -- and puts
//! the result on a [`LaunchWarningSink`] the host holds, rather than returning it from
//! [`ProtocolAdapter::poll`], whose signature returns detections and would therefore
//! make it one.
//!
//! The sink is the pattern the ASTERIX feed already uses for its service observations:
//! the adapter is moved into the gateway, so anything the host must read afterwards is
//! shared with it at construction rather than fetched from the adapter later.
//!
//! Nothing in this workspace *issues* a launch warning. What is built is the receiving
//! half, which is what GAP-009 is about.

use crate::{DetectionView, IngestError, ProtocolAdapter};
use gungnir_model::{
    LaunchWarningReport, MissionTime, PeerLaunchWarning, PeerOrigin, SensorId, TrackView,
};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Longest launch-warning text this deployment will hold, in characters.
///
/// A bound rather than a schema, because DN-16 §5 keeps the peer's own words and an
/// enumeration would silently re-categorise them. The bound is what stops a peer from
/// putting a megabyte of text into an operator's alert list; what is over it is
/// quarantined with the reason, never truncated, because a truncated warning reads like
/// a complete one.
pub const MAX_LAUNCH_WARNING_CHARS: usize = 512;

/// Where a peer's tracks and launch warnings come from: a node link, or a recording, or
/// a test.
pub trait PeerStream: Send {
    /// Every track the peer has published since the last call.
    fn take_tracks(&mut self) -> Vec<TrackView>;

    /// Every launch warning the peer has issued since the last call (DN-16 §5).
    ///
    /// A separate call from [`PeerStream::take_tracks`] rather than one that returns
    /// both, so no implementation can return a warning where a track was expected.
    fn take_launch_warnings(&mut self) -> Vec<LaunchWarningReport>;

    fn describe(&self) -> String;
}

/// What became of one launch warning at the gateway (DN-16 §5).
///
/// Two states rather than an `Option`, because "the peer sent nothing" and "the peer
/// sent something we refused" are opposite claims about a peer, and DN-16 §5 requires
/// the second to be visible with its reason.
#[derive(Debug, Clone, PartialEq)]
pub enum LaunchWarningOutcome {
    /// Validated and stamped with our name for the peer and our receipt time.
    Admitted(PeerLaunchWarning),
    /// Refused, with the reason kept for the operator and the record.
    Quarantined {
        /// The configured name of the peer, ours rather than anything it sent.
        peer: String,
        reason: String,
        at: MissionTime,
    },
}

/// The queue of launch-warning outcomes a host drains (DN-16 §5).
///
/// The same shape as `asterix::ServiceObservationSink`, for the same reason: the adapter
/// belongs to the gateway once it is added, and this is what the host keeps hold of.
pub type LaunchWarningSink = Arc<Mutex<VecDeque<LaunchWarningOutcome>>>;

/// Counts for the health panel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PeerStats {
    pub received: u64,
    /// Older than `max_age_s` on receipt: emitted, marked stale.
    pub stale: u64,
    /// Launch warnings admitted (DN-16 §5). Counted apart from `received`, which is
    /// tracks: a peer that sends only warnings has produced no tracks, and one number
    /// for both would hide that.
    pub launch_warnings: u64,
    /// Launch warnings refused, with the reason on the sink.
    pub launch_warnings_quarantined: u64,
}

pub struct PeerSourceAdapter<S: PeerStream> {
    name: String,
    peer: String,
    source: SensorId,
    assigned_quality: f32,
    max_age_s: f64,
    stream: S,
    stats: PeerStats,
    /// Where admitted and quarantined launch warnings go. `None` means the host asked
    /// for none, and the adapter then drains them from the stream and drops them rather
    /// than letting the stream's own queue grow without bound -- counted either way.
    launch_warnings: Option<LaunchWarningSink>,
}

impl<S: PeerStream> PeerSourceAdapter<S> {
    /// `source` is the identifier the gateway admits this peer under (the baseline's
    /// `peers[].source_id`); `assigned_quality` and `max_age_s` are ours.
    pub fn new(
        peer: impl Into<String>,
        source: SensorId,
        assigned_quality: f32,
        max_age_s: f64,
        stream: S,
    ) -> Self {
        let peer = peer.into();
        Self {
            name: format!("peer:{peer}:{}", stream.describe()),
            peer,
            source,
            assigned_quality,
            max_age_s,
            stream,
            stats: PeerStats::default(),
            launch_warnings: None,
        }
    }

    /// Deliver this peer's launch warnings to `sink` (DN-16 §5).
    ///
    /// Without one the adapter still drains them from the stream, so a peer that warns
    /// and is not listened to cannot make the stream's queue grow, and the count still
    /// says how many arrived.
    #[must_use]
    pub fn with_launch_warning_sink(mut self, sink: LaunchWarningSink) -> Self {
        self.launch_warnings = Some(sink);
        self
    }

    #[must_use]
    pub fn stats(&self) -> PeerStats {
        self.stats
    }

    /// One peer track as a detection: its position as the measurement, the peer on the
    /// provenance, the age computed from the peer's own stamp.
    fn convert(&mut self, track: &TrackView, now: MissionTime) -> DetectionView {
        self.stats.received += 1;
        let age_s = now.0 - track.mission_time.0;
        let mut provenance = track.provenance.clone();
        provenance.peer = Some(PeerOrigin {
            peer: self.peer.clone(),
            remote_track: track.id.0.to_string(),
            peer_time: track.mission_time,
            receipt_time: now,
            assigned_quality: self.assigned_quality,
        });
        if age_s > self.max_age_s {
            self.stats.stale += 1;
            provenance.conversion_loss = Some(format!(
                "peer track is {age_s:.0} s old on receipt, beyond the {:.0} s the deployment \
                 accepts as current; stale",
                self.max_age_s
            ));
        }
        let p = track.position_enu();
        // DN-27 §8: a peer track's position stays a position. Its error is the peer's
        // own, off the covariance it sent, rather than the baseline's default -- a peer
        // track is the one source in this system that arrives with a stated uncertainty,
        // and discarding it to stamp a constant would be the loss DN-27 §6 warns about.
        // A peer that sends a broken covariance is caught by the gateway's validation,
        // which is where every other malformed field from a peer is caught.
        DetectionView {
            sensor: self.source,
            source_time: track.mission_time,
            receipt_time: now,
            measurement: gungnir_model::Measurement::Position {
                enu: nalgebra::Vector3::new(p[0], p[1], p[2]),
                variance_m2: [
                    track.covariance[(0, 0)],
                    track.covariance[(1, 1)],
                    track.covariance[(2, 2)],
                ],
            },
            provenance,
        }
    }

    /// Take the peer's launch warnings, validate them, and put the outcomes on the sink.
    ///
    /// Called from [`ProtocolAdapter::poll`] so a warning arrives on exactly the tick a
    /// track would have, which is what DN-16 §5's "on the same path" means. **Nothing
    /// here returns a [`DetectionView`]**, which is the structural reason a launch
    /// warning cannot become a track.
    fn drain_launch_warnings(&mut self, now: MissionTime) {
        let warnings = self.stream.take_launch_warnings();
        if warnings.is_empty() {
            return;
        }
        let mut outcomes = Vec::with_capacity(warnings.len());
        for warning in warnings {
            match validate_launch_warning(&warning, now) {
                Ok(()) => {
                    self.stats.launch_warnings += 1;
                    let admitted = PeerLaunchWarning {
                        peer: self.peer.clone(),
                        report: warning,
                        receipt_time: now,
                    };
                    if admitted.is_stale_beyond(self.max_age_s) {
                        self.stats.stale += 1;
                    }
                    outcomes.push(LaunchWarningOutcome::Admitted(admitted));
                }
                Err(reason) => {
                    self.stats.launch_warnings_quarantined += 1;
                    outcomes.push(LaunchWarningOutcome::Quarantined {
                        peer: self.peer.clone(),
                        reason,
                        at: now,
                    });
                }
            }
        }
        if let Some(sink) = &self.launch_warnings {
            if let Ok(mut queue) = sink.lock() {
                queue.extend(outcomes);
            }
        }
    }
}

impl<S: PeerStream> ProtocolAdapter for PeerSourceAdapter<S> {
    fn name(&self) -> &str {
        &self.name
    }

    fn poll(&mut self, now: MissionTime) -> Result<Vec<DetectionView>, IngestError> {
        // DN-16 §5: warnings first, so a launch warning is never held back behind a
        // track conversion that failed. They leave by the sink, not by this return
        // value, which carries detections and nothing else.
        self.drain_launch_warnings(now);
        let tracks = self.stream.take_tracks();
        Ok(tracks.iter().map(|t| self.convert(t, now)).collect())
    }
}

/// The rules a launch warning must pass to be admitted (DN-16 §5).
///
/// The same shape as `gateway::validate_detection`: sanity, not judgement. Whether
/// the peer is telling the truth about a launch is not a question this deployment
/// can answer, and a rule that tried would be a filter deciding what an operator is
/// allowed to hear. What is checked is that the message is one we can show: it says
/// something, it says which warning it is, and its time is a number that is not in
/// our future.
///
/// Age is **not** a rejection. A late launch warning is still evidence that a peer
/// saw a launch, and DN-16 §5 marks a stale peer message rather than discarding it.
fn validate_launch_warning(warning: &LaunchWarningReport, now: MissionTime) -> Result<(), String> {
    if warning.id.trim().is_empty() {
        return Err(
            "the launch warning carries no identifier, so a repeat of it \
                    could not be told from a second launch"
                .to_owned(),
        );
    }
    if warning.what.trim().is_empty() {
        return Err(
            "the launch warning says nothing; an alert with no statement on \
                    it is one an operator cannot act on"
                .to_owned(),
        );
    }
    if warning.what.chars().count() > MAX_LAUNCH_WARNING_CHARS {
        return Err(format!(
            "the launch warning is {} characters, beyond the {MAX_LAUNCH_WARNING_CHARS} \
             this deployment holds; refused rather than truncated, because a truncated \
             warning reads like a complete one",
            warning.what.chars().count()
        ));
    }
    if !warning.at.0.is_finite() {
        return Err("the launch warning's time is not a finite number".to_owned());
    }
    if warning.at.0 > now.0 + crate::gateway::MAX_SOURCE_AHEAD_OF_RECEIPT_S {
        return Err(format!(
            "the launch warning is stamped {:.1} s in our future, beyond the {:.1} s of \
             clock lead this deployment tolerates",
            warning.at.0 - now.0,
            crate::gateway::MAX_SOURCE_AHEAD_OF_RECEIPT_S
        ));
    }
    Ok(())
}

/// Tracks and launch warnings queued in memory: tests, and a recording of a peer.
#[derive(Debug, Default)]
pub struct QueuedPeerStream {
    pub tracks: Vec<TrackView>,
    /// Launch warnings the peer issued (DN-16 §5). Kept in a field of its own, so a
    /// recording that holds one cannot present it where a track is read.
    pub launch_warnings: Vec<LaunchWarningReport>,
}

impl PeerStream for QueuedPeerStream {
    fn take_tracks(&mut self) -> Vec<TrackView> {
        std::mem::take(&mut self.tracks)
    }

    fn take_launch_warnings(&mut self) -> Vec<LaunchWarningReport> {
        std::mem::take(&mut self.launch_warnings)
    }

    fn describe(&self) -> String {
        "queued".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::{Classification, Provenance, Quality, TrackId, TrackStatus};

    fn track(id: u64, at: f64) -> TrackView {
        TrackView {
            id: TrackId(id),
            status: TrackStatus::Confirmed,
            state: nalgebra::SVector::<f64, 6>::new(100.0, 200.0, 30.0, 5.0, 0.0, 0.0),
            covariance: nalgebra::SMatrix::<f64, 6, 6>::identity(),
            classification: Classification::Hostile,
            provenance: Provenance::default(),
            quality: Quality::default(),
            mission_time: MissionTime(at),
            releasability: gungnir_model::Releasability::default(),
        }
    }

    #[test]
    fn a_peer_track_becomes_a_detection_with_the_peer_named_and_our_quality() {
        let mut adapter = PeerSourceAdapter::new(
            "kal-cell",
            SensorId(900),
            0.6,
            30.0,
            QueuedPeerStream {
                tracks: vec![track(41, 95.0)],
                ..QueuedPeerStream::default()
            },
        );
        let out = adapter.poll(MissionTime(100.0)).expect("polls");
        assert_eq!(out.len(), 1);
        let d = &out[0];
        assert_eq!(d.sensor, SensorId(900));
        assert_eq!(
            d.measurement.position_enu(),
            Some(nalgebra::Vector3::new(100.0, 200.0, 30.0))
        );
        let peer = d.provenance.peer.as_ref().expect("peer origin");
        assert_eq!(peer.peer, "kal-cell");
        assert_eq!(peer.remote_track, "41");
        assert!(
            (peer.assigned_quality - 0.6).abs() < f32::EPSILON,
            "ours, not theirs"
        );
        assert!(
            (peer.receipt_time.0 - peer.peer_time.0 - 5.0).abs() < 1e-9,
            "age is visible"
        );
        assert!(d.provenance.conversion_loss.is_none());
        assert_eq!(
            adapter.stats(),
            PeerStats {
                received: 1,
                ..PeerStats::default()
            }
        );
    }

    #[test]
    fn an_old_peer_track_is_emitted_and_marked_stale_never_dropped() {
        let mut adapter = PeerSourceAdapter::new(
            "kal-cell",
            SensorId(900),
            0.6,
            30.0,
            QueuedPeerStream {
                tracks: vec![track(41, 10.0), track(42, 99.0)],
                ..QueuedPeerStream::default()
            },
        );
        let out = adapter.poll(MissionTime(100.0)).expect("polls");
        assert_eq!(out.len(), 2, "nothing is discarded");
        assert!(out[0]
            .provenance
            .conversion_loss
            .as_deref()
            .is_some_and(|l| l.contains("stale")));
        assert!(out[1].provenance.conversion_loss.is_none());
        assert_eq!(
            adapter.stats(),
            PeerStats {
                received: 2,
                stale: 1,
                ..PeerStats::default()
            }
        );
        assert!(
            adapter.poll(MissionTime(101.0)).expect("polls").is_empty(),
            "taken once"
        );
    }

    fn warning(id: &str, what: &str, at: f64) -> LaunchWarningReport {
        LaunchWarningReport {
            id: id.into(),
            what: what.into(),
            at: MissionTime(at),
            releasability: gungnir_model::Releasability::AllPeers,
        }
    }

    fn sink() -> LaunchWarningSink {
        LaunchWarningSink::default()
    }

    fn drained(sink: &LaunchWarningSink) -> Vec<LaunchWarningOutcome> {
        sink.lock()
            .map(|mut q| q.drain(..).collect())
            .unwrap_or_default()
    }

    /// DN-16 §5's own criterion: a launch warning creates an alert and no track. The
    /// adapter is the place that would have turned it into one, and it produces no
    /// detection at all.
    #[test]
    fn a_launch_warning_is_admitted_as_itself_and_produces_no_detection() {
        let warnings = sink();
        let mut adapter = PeerSourceAdapter::new(
            "kal-cell",
            SensorId(900),
            0.6,
            30.0,
            QueuedPeerStream {
                launch_warnings: vec![warning("LW-7", "ballistic launch, northern sector", 95.0)],
                ..QueuedPeerStream::default()
            },
        )
        .with_launch_warning_sink(warnings.clone());

        let detections = adapter.poll(MissionTime(100.0)).expect("polls");
        assert!(
            detections.is_empty(),
            "a warning is not a detection and must not become a track"
        );

        let out = drained(&warnings);
        assert_eq!(out.len(), 1);
        let LaunchWarningOutcome::Admitted(admitted) = &out[0] else {
            panic!("expected an admitted warning, got {:?}", out[0]);
        };
        assert_eq!(
            admitted.peer, "kal-cell",
            "our name for the peer, not theirs"
        );
        assert_eq!(admitted.report.id, "LW-7");
        assert!((admitted.age_s() - 5.0).abs() < 1e-9, "age is visible");
        assert!(admitted.alert_summary().contains("kal-cell"));
        assert_eq!(
            adapter.stats(),
            PeerStats {
                launch_warnings: 1,
                ..PeerStats::default()
            }
        );
        assert!(
            drained(&warnings).is_empty(),
            "handed over once, not on every poll"
        );
    }

    /// A malformed warning is quarantined with a reason and does not reach the picture,
    /// which is the rule DN-16 §5 states for a malformed peer track and applies to
    /// every peer message.
    #[test]
    fn a_malformed_launch_warning_is_quarantined_with_a_reason() {
        let warnings = sink();
        let mut adapter = PeerSourceAdapter::new(
            "kal-cell",
            SensorId(900),
            0.6,
            30.0,
            QueuedPeerStream {
                launch_warnings: vec![
                    warning("", "launch", 95.0),
                    warning("LW-8", "   ", 95.0),
                    warning("LW-9", &"x".repeat(MAX_LAUNCH_WARNING_CHARS + 1), 95.0),
                    warning("LW-10", "launch", 200.0),
                    warning("LW-11", "launch", f64::NAN),
                ],
                ..QueuedPeerStream::default()
            },
        )
        .with_launch_warning_sink(warnings.clone());

        assert!(adapter.poll(MissionTime(100.0)).expect("polls").is_empty());
        let out = drained(&warnings);
        assert_eq!(out.len(), 5);
        for outcome in &out {
            let LaunchWarningOutcome::Quarantined { peer, reason, .. } = outcome else {
                panic!("expected a quarantine, got {outcome:?}");
            };
            assert_eq!(peer, "kal-cell");
            assert!(
                !reason.is_empty(),
                "a quarantine without a reason is a drop"
            );
        }
        assert_eq!(
            adapter.stats(),
            PeerStats {
                launch_warnings_quarantined: 5,
                ..PeerStats::default()
            }
        );
    }

    /// Age never refuses a warning: a late one is still evidence a peer saw a launch,
    /// and DN-16 §5 marks a stale peer message rather than discarding it.
    #[test]
    fn a_late_launch_warning_is_admitted_and_counted_stale() {
        let warnings = sink();
        let mut adapter = PeerSourceAdapter::new(
            "kal-cell",
            SensorId(900),
            0.6,
            30.0,
            QueuedPeerStream {
                launch_warnings: vec![warning("LW-12", "launch", 10.0)],
                ..QueuedPeerStream::default()
            },
        )
        .with_launch_warning_sink(warnings.clone());

        assert!(adapter.poll(MissionTime(100.0)).expect("polls").is_empty());
        let out = drained(&warnings);
        assert!(matches!(out[0], LaunchWarningOutcome::Admitted(_)));
        assert_eq!(
            adapter.stats(),
            PeerStats {
                launch_warnings: 1,
                stale: 1,
                ..PeerStats::default()
            }
        );
    }

    /// A host that keeps no sink still drains the stream, so a peer that warns into a
    /// deployment listening for tracks alone cannot make its queue grow without bound.
    #[test]
    fn warnings_are_drained_and_counted_even_with_no_sink() {
        let mut adapter = PeerSourceAdapter::new(
            "kal-cell",
            SensorId(900),
            0.6,
            30.0,
            QueuedPeerStream {
                launch_warnings: vec![warning("LW-13", "launch", 99.0)],
                ..QueuedPeerStream::default()
            },
        );
        assert!(adapter.poll(MissionTime(100.0)).expect("polls").is_empty());
        assert_eq!(adapter.stats().launch_warnings, 1);
    }

    /// Tracks and warnings arrive on the same poll and stay apart: the track becomes a
    /// detection, the warning does not.
    #[test]
    fn a_track_and_a_warning_on_one_poll_stay_apart() {
        let warnings = sink();
        let mut adapter = PeerSourceAdapter::new(
            "kal-cell",
            SensorId(900),
            0.6,
            30.0,
            QueuedPeerStream {
                tracks: vec![track(41, 99.0)],
                launch_warnings: vec![warning("LW-14", "launch", 99.0)],
            },
        )
        .with_launch_warning_sink(warnings.clone());

        let detections = adapter.poll(MissionTime(100.0)).expect("polls");
        assert_eq!(detections.len(), 1, "the track, and only the track");
        assert_eq!(drained(&warnings).len(), 1);
        assert_eq!(
            adapter.stats(),
            PeerStats {
                received: 1,
                launch_warnings: 1,
                ..PeerStats::default()
            }
        );
    }
}

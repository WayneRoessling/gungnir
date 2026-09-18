// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Versioned event schema -- what `gungnir-eventing` publishes and subscribes,
//! replacing the polling-a-mutable-slice pattern per docs/gungnir-capabilities.md
//! §5.1 ("Event & Durable Messaging"). `gungnir-eventing::Event` wraps these four
//! enums and `gungnir-store` journals them.

use crate::{
    CollectionRequirement, Concurrence, DecisionId, DetectionView, EffectorLayer, MissionTime,
    PendingApprovalId, PlanId, PlanView, RequestId, RequirementId, SensorId, SensorTaskId, TrackId,
    TrackView,
};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TrackingEvent {
    TrackInitiated(TrackView),
    TrackUpdated(TrackView),
    TrackCoasting(TrackId),
    TrackDeleted(TrackId),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum InterceptEvent {
    PlanProposed(PlanView),
    PlanApproved(PlanView),
    PlanSuperseded(PlanView),
    /// The policy chain's verdict on a proposed plan, and which engines produced it
    /// (GAP-028). Published by both binaries. **The engine list is the claim**: a node
    /// evaluates geofence and control status and not authority, because authority is a
    /// question about who is asking and nobody signs in to a node; a desktop reading a
    /// node's verdict sees exactly what was and was not checked.
    PlanEvaluated {
        plan: PlanId,
        verdict: VerdictSummary,
        engines: Vec<String>,
    },
}

/// What the ingest gateway did with an observation, for provenance and audit.
///
/// `Accepted` carries a whole `DetectionView` while the two refusals carry a sensor and
/// a reason, and the difference grew when the measurement became a [`crate::Measurement`]
/// carrying its own error (docs/design/DN-27-bearing-only-detections.md §4). The size
/// difference is accepted deliberately, for the reason `gungnir_eventing::Event` states
/// for the same trade: accepting is the common case and the hot path, so boxing it would
/// add a heap allocation per accepted observation to shrink the two rare variants.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum IngestEvent {
    Accepted(DetectionView),
    Quarantined {
        sensor: SensorId,
        reason: String,
    },
    /// The detection was valid and the tracking pipeline would not take it (GAP-066).
    ///
    /// **Not quarantine**: nothing was wrong with the detection, and counting it as
    /// quarantined would blame a sensor for a failure on this side. Not `Accepted`
    /// either, which is what the gateway used to record because
    /// `TrackingService::submit_detection` returned nothing and it could not tell.
    NotAccepted {
        sensor: SensorId,
        reason: String,
    },
}

/// What a sensor is doing changing (GAP-003).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SensorEvent {
    /// An operator or an automated retasker changed what a sensor is doing.
    ///
    /// Carries both ends, because "set to Search" without the previous mode does not
    /// say whether coverage grew or shrank, and a coverage answer that changed is the
    /// thing an after-action review asks about.
    ModeChanged {
        sensor: crate::SensorId,
        from: crate::SensorMode,
        to: crate::SensorMode,
        at: MissionTime,
    },
}

/// What a track was taken to be, across sessions
/// (`../../docs/mission/gap-analysis/gap-register.md` GAP-019, DN-19).
///
/// **The reason this event exists is that the answer had nowhere to go.** `TrackView`
/// carries no `GlobalEntityId`, and until 2026-09-06 the only resolver in the workspace
/// ran on a desktop and kept its lineage in memory for one panel to draw. A node works
/// out the same answer and its whole job is the authoritative record, so an identity it
/// could not write down would be one nobody could review afterwards.
///
/// **Both outcomes are recorded, not just the interesting one.** A minted identity is a
/// statement that nothing already seen matched, and a reviewer asking why two sightings
/// were *not* joined needs that as much as the confidence of a join that was made.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum IdentityEvent {
    /// This track was taken to be an entity already seen.
    ///
    /// The confidence and the basis are both carried because a correlation without its
    /// reason cannot be argued with: DN-19's rule is that a product be traceable to the
    /// evidence, and "0.83, kinematic and class agreement" is evidence where "0.83" alone
    /// is an assertion.
    Correlated {
        track: TrackId,
        entity: crate::identity::GlobalEntityId,
        confidence: f64,
        basis: String,
        at: MissionTime,
    },
    /// This track became a new entity: nothing already seen was close enough to join it.
    Minted {
        track: TrackId,
        entity: crate::identity::GlobalEntityId,
        at: MissionTime,
    },
}

/// A sensor's report of an object whose true position is already known
/// (`../../docs/mission/gap-analysis/gap-register.md` GAP-014).
///
/// This is the evidence sensor registration is made from. Registering two platforms
/// against each other recovers only their *relative* offset, and if both are displaced
/// the same way the pair looks perfectly registered while the whole picture is in the
/// wrong place. A surveyed reference -- a corner reflector, a transponder at a known
/// point, a runway threshold -- is absolute, so one platform's bias can be found without
/// a second platform to compare against.
///
/// **The truth position and the sensor's report are both carried, and neither is a
/// residual.** A pre-computed residual cannot be re-examined when the survey is later
/// corrected, and surveys are corrected.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum CalibrationEvent {
    /// A sensor reported a reference object, and both what it said and what is true are
    /// recorded.
    ReferenceObserved {
        sensor: SensorId,
        /// What the reference object is, in the deployment's own words. A survey
        /// identifier, not a name a person would be identified by.
        reference: String,
        /// The surveyed position, local ENU metres.
        truth_enu: [f64; 3],
        /// Where the sensor put it, local ENU metres.
        observed_enu: [f64; 3],
        /// The variance the sensor claims for that report, per axis, metres squared.
        /// Used to weight this observation against others; a sensor that will not state
        /// one has to be given a deployment default, and that is the caller's decision
        /// rather than a number this type invents.
        observation_variance_m2: [f64; 3],
        at: MissionTime,
    },
    /// A registration estimate was made and applied to a sensor.
    ///
    /// Carries the residual spread as well as the offset, because an offset alone
    /// cannot be reviewed: a single translation fitted to an orientation error produces
    /// a plausible-looking offset and a residual spread that gives it away.
    RegistrationApplied {
        sensor: SensorId,
        offset_enu: [f64; 3],
        residual_spread_m: f64,
        references: u32,
        at: MissionTime,
    },
    /// Registration was attempted and refused, with the reason kept verbatim.
    RegistrationRefused {
        sensor: SensorId,
        reason: String,
        at: MissionTime,
    },
}

/// The life of one command to a sensor (docs/design/DN-11-sensor-control-and-tasking.md
/// §6, GAP-004).
///
/// Separate from [`SensorEvent`] rather than folded into it, because these four are the
/// *asking*, and `SensorEvent::ModeChanged` is the *fact*. An after-action review that
/// could not tell "we told the radar to search" from "the radar searched" would be
/// unable to answer the question MT-07 turns on.
///
/// Every variant carries the sensor as well as the task, so a reader can answer "what
/// happened to this sensor" without first building an index of task identifiers.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SensorTaskEvent {
    /// Recorded and handed toward the adapter. **Not a claim that a sensor received
    /// anything**; no adapter exists yet (GAP-001).
    Issued {
        task: SensorTaskId,
        sensor: SensorId,
        at: MissionTime,
    },
    /// The sensor confirmed. The only one of the four that means local state changed.
    Acknowledged {
        task: SensorTaskId,
        sensor: SensorId,
        at: MissionTime,
    },
    /// The sensor refused, or the adapter could not deliver. The reason is the
    /// adapter's own words, kept verbatim: paraphrasing it loses the diagnosis.
    Failed {
        task: SensorTaskId,
        sensor: SensorId,
        reason: String,
        at: MissionTime,
    },
    /// The window closed with nothing back. **Not a failure**: nobody refused, nobody
    /// answered, and the two are as different here as an expiry and a rejection are in
    /// DN-10.
    Unacknowledged {
        task: SensorTaskId,
        sensor: SensorId,
        at: MissionTime,
    },
}

/// Which algorithm configuration is in force, and how it got there
/// (docs/design/DN-24-mission-profiles-and-algorithm-baselines.md §8, GAP-086).
///
/// # Two variants DN-24 §8 does not list, and why
///
/// The note names `Promoted`, `RolledBack` and `PromotionRefused`. Both additions here
/// exist because **the alternative was to say something untrue at session start.**
///
/// `InForceAtStart` is not a `Promoted`: at startup the file said what is in force and
/// nobody promoted anything, so a `Promoted` event would put an act in the record that
/// nobody performed — and `by` would have to be fabricated, since nobody is signed in while
/// the session is being built.
///
/// `NoneInForce` is not silence: a deployment that declares no algorithm configuration
/// governs nothing, and **"nothing was in force" and "we did not record it" are opposite
/// claims** to whoever reads the journal afterwards.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum GovernanceEvent {
    /// What the baseline had in force when the session opened.
    InForceAtStart {
        baseline: crate::AlgorithmBaselineId,
        at: MissionTime,
    },
    /// No algorithm configuration is in force at all, for the named profile or for the
    /// deployment when it declares none.
    NoneInForce {
        profile: Option<crate::MissionProfile>,
        at: MissionTime,
    },
    /// Somebody put a candidate into force during the session.
    Promoted {
        baseline: crate::AlgorithmBaselineId,
        by: String,
        at: MissionTime,
    },
    /// A profile was rolled back; `restored` is what came back into force, and `None` when
    /// there was nothing behind it.
    RolledBack {
        profile: crate::MissionProfile,
        restored: Option<crate::AlgorithmBaselineId>,
        by: String,
        at: MissionTime,
    },
    /// A promotion that did not happen, and why.
    ///
    /// Recorded for the reason a denied plan is: **a refusal nobody can see is a refusal
    /// that gets argued about later.**
    PromotionRefused {
        baseline: crate::AlgorithmBaselineId,
        reason: String,
        at: MissionTime,
    },
}

impl GovernanceEvent {
    #[must_use]
    pub fn at(&self) -> MissionTime {
        match self {
            GovernanceEvent::InForceAtStart { at, .. }
            | GovernanceEvent::NoneInForce { at, .. }
            | GovernanceEvent::Promoted { at, .. }
            | GovernanceEvent::RolledBack { at, .. }
            | GovernanceEvent::PromotionRefused { at, .. } => *at,
        }
    }
}

/// The watch's rhythm: what came due, what was handed over, what went down on purpose
/// (docs/design/DN-21-battle-rhythm.md §6, GAP-054).
///
/// # Why one variant rather than the three DN-21 §6 asks for
///
/// The note puts the handover on `Event::Handover` and the maintenance overrun on
/// `SensorTaskEvent`. The second does not fit: **every `SensorTaskEvent` variant carries a
/// `SensorTaskId`** and `SensorTaskEvent::task()` returns one unconditionally, because a
/// task event without a task is meaningless. A maintenance window is not a task -- nobody
/// asked a sensor for anything -- so putting it there would have made that accessor
/// fallible for every caller to serve one variant. One rhythm variant keeps the note's
/// three kinds together and leaves tasking's invariant intact.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum RhythmEvent {
    /// A scheduled product came due at a mission time the schedule chose.
    ///
    /// Published at the moment the schedule names, not the moment the tick noticed, so a
    /// replayed session records the same times.
    ProductDue {
        name: String,
        kind: crate::ProductKind,
        due: MissionTime,
    },
    /// Produced and held for a person to read, because no endpoint is configured.
    ///
    /// **A real configuration and not a failure** (DN-21 §5), which is why it is not the
    /// same event as the one below.
    ProductHeld { name: String, at: MissionTime },
    /// Produced, an endpoint is configured, and nothing carried it there.
    ///
    /// **Never a silent drop.** There is no delivery path yet (DN-07's rules land with
    /// GAP-040), so a product with an endpoint is recorded as owed rather than treated as
    /// sent. A deployment that believed it was reporting to higher command and was not is
    /// the failure this event exists to prevent.
    ProductUndelivered {
        name: String,
        endpoint: String,
        reason: String,
        at: MissionTime,
    },
    /// The incoming watch took over, by name and at a time.
    ///
    /// **This is what MOE-13 counts.** An unacknowledged handover leaves no event, which
    /// is what makes it visible rather than assumed.
    HandoverAcknowledged {
        by: String,
        period: (MissionTime, MissionTime),
        outstanding: bool,
        at: MissionTime,
    },
    /// A planned window opened; this sensor's silence is expected from here.
    MaintenanceOpened {
        sensor: SensorId,
        until: MissionTime,
        reason: String,
        at: MissionTime,
    },
    /// A planned window closed with the sensor back.
    MaintenanceCompleted { sensor: SensorId, at: MissionTime },
    /// **A window closed and the sensor did not come back.**
    ///
    /// Neither an expected absence nor an ordinary failure, and the case the whole feature
    /// exists to surface: somebody planned this outage and it has not ended.
    MaintenanceOverrun {
        sensor: SensorId,
        window_closed: MissionTime,
        reason: String,
        at: MissionTime,
    },
}

impl RhythmEvent {
    /// When this happened, for the journal and the summary.
    #[must_use]
    pub fn at(&self) -> MissionTime {
        match self {
            RhythmEvent::ProductDue { due, .. } => *due,
            RhythmEvent::ProductHeld { at, .. }
            | RhythmEvent::ProductUndelivered { at, .. }
            | RhythmEvent::HandoverAcknowledged { at, .. }
            | RhythmEvent::MaintenanceOpened { at, .. }
            | RhythmEvent::MaintenanceCompleted { at, .. }
            | RhythmEvent::MaintenanceOverrun { at, .. } => *at,
        }
    }
}

impl SensorTaskEvent {
    #[must_use]
    pub fn task(&self) -> SensorTaskId {
        match self {
            SensorTaskEvent::Issued { task, .. }
            | SensorTaskEvent::Acknowledged { task, .. }
            | SensorTaskEvent::Failed { task, .. }
            | SensorTaskEvent::Unacknowledged { task, .. } => *task,
        }
    }

    #[must_use]
    pub fn sensor(&self) -> SensorId {
        match self {
            SensorTaskEvent::Issued { sensor, .. }
            | SensorTaskEvent::Acknowledged { sensor, .. }
            | SensorTaskEvent::Failed { sensor, .. }
            | SensorTaskEvent::Unacknowledged { sensor, .. } => *sensor,
        }
    }
}

/// The life of one collection requirement
/// (docs/design/DN-11-sensor-control-and-tasking.md, GAP-005).
///
/// **Not in DN-11 §6**: added by amendment 1 (a). §6
/// lists the interface delta and the `SensorTask` event but no requirement event, and
/// without one the requirement lifecycle exists only in memory -- it would vanish on
/// exit, and the CAP-2.12 row's method is an MT-08 *replay*, which can read nothing but
/// the journal. A concurrence that names who concurred and never reaches the journal
/// cannot be reviewed, which is the whole reason §5 requires it to name anybody.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum RequirementEvent {
    /// An analyst stated what they need to know.
    ///
    /// **Carries the whole requirement**, as `TrackingEvent::TrackInitiated` carries a
    /// whole `TrackView` and for the same reason: an event stream that cannot reconstruct
    /// what it describes cannot be replayed. The first version of this variant carried
    /// only the title, which left the area, the priority and the needed-by time reachable
    /// nowhere -- so the lifecycle was on the bus and the requirement itself was not, and
    /// rebuilding a list from the journal was impossible. Corrected under GAP-005.
    Stated {
        requirement: CollectionRequirement,
        at: MissionTime,
    },
    /// A sensor manager concurred, and a task exists serving it. Carries the task, so
    /// the link DN-11 §4 draws between a requirement and its tasks survives into the
    /// record rather than living only in the panel.
    Tasked {
        requirement: RequirementId,
        task: SensorTaskId,
        by: Concurrence,
        at: MissionTime,
    },
    /// A sensor manager declined, with the reason in their own words.
    Declined {
        requirement: RequirementId,
        by: Concurrence,
        reason: String,
        at: MissionTime,
    },
    /// Answered, naming the evidence that answered it. **Never automatic**: a task the
    /// sensor acknowledged means it took the command, not that it answered the question.
    Satisfied {
        requirement: RequirementId,
        evidence: String,
        at: MissionTime,
    },
    /// The needed-by time passed with it still open. **Not a decline**: nobody refused
    /// it, the same distinction DN-10 draws between an expiry and a rejection.
    Lapsed {
        requirement: RequirementId,
        at: MissionTime,
    },
}

impl RequirementEvent {
    #[must_use]
    pub fn requirement(&self) -> RequirementId {
        match self {
            RequirementEvent::Stated { requirement, .. } => requirement.id,
            RequirementEvent::Tasked { requirement, .. }
            | RequirementEvent::Declined { requirement, .. }
            | RequirementEvent::Satisfied { requirement, .. }
            | RequirementEvent::Lapsed { requirement, .. } => *requirement,
        }
    }
}

/// Operator decisions on plans, for the audit trail. Mirrors
/// `gungnir_command::DecisionRecord` in a form the model can carry without depending
/// on that crate.
///
/// `ApprovalRequested(PlanId)` was removed at schema version 4 (GAP-130,
/// `docs/design/DN-31-node-approval-queue.md` §5.3): it carried too little to build a
/// queue from and nothing ever published it, so no journal holds one.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum CommandEvent {
    Decided {
        plan: PlanId,
        /// The decision this event records, so engagement state can key on it
        /// without depending on the crate that records decisions
        /// (docs/design/DN-06-engagement-and-effect.md).
        decision: DecisionId,
        accepted: bool,
        operator: Option<String>,
        /// The role the deciding operator's session carried, in `gungnir_security::Role`'s
        /// debug spelling (the spelling `Concurrence::Operator` carries), so the
        /// arbitration rule can rank this decision against a conflicting one
        /// (`crate::arbitration`, D-03; the GAP-067 walk, 2026-09-16).
        ///
        /// **Recorded only from an authenticated session** (DN-23 §5 rule 1): `None` when
        /// nobody was signed in, even though a desktop always has a selected role, because
        /// a selection is an authority nobody verified and ranking it would let an
        /// unauthenticated choice outrank a signed-in person. `None` is also what a journal
        /// written before this field existed reads as, which is why it defaults: the change
        /// is additive and `SCHEMA_VERSION` stands. A conflict with a side whose role is
        /// `None` is left to a person.
        #[serde(default)]
        role: Option<String>,
        /// What the policy chain said, in the model's words (MOE-05 needs it on the
        /// record; the policy crate's own enum may not be depended on from here).
        verdict: VerdictSummary,
        /// The reason the record carries. **Only a rejection has one today**: an
        /// acceptance records no rationale until the course of action reaches the
        /// record (GAP-032), and MOE-05 counts that absence rather than hiding it.
        rationale: Option<String>,
        /// The client's idempotency key, so a retry is the same request rather than a
        /// second decision (DN-31 §5.2 and §5.3, GAP-132).
        ///
        /// `None` for a decision taken where no route carried one -- a desktop's own
        /// queue, and every journal written before this field existed. Defaulted, so the
        /// change is additive and `SCHEMA_VERSION` stands
        /// (`docs/gungnir-api-v1.md`, "Adding a field with a default is compatible").
        #[serde(default)]
        request: Option<RequestId>,
        /// The machine a forwarded decision was taken on (DN-31 §5.3, §6.8).
        ///
        /// **Always `None` in this build**: forwarding is GAP-134's, and this field is
        /// what it fills. It is declared with the rest of the decision's shape rather
        /// than added later so the event and
        /// `gungnir_command::DecisionRecord` carry one description of a decision between
        /// them. `None` means "taken here", which is what every decision is today.
        #[serde(default)]
        origin: Option<String>,
    },
    /// The window closed with nobody deciding. **Not a rejection**: nobody chose,
    /// and an after-action review must be able to tell the two apart
    /// (docs/design/DN-10-queue-expiry-and-escalation.md).
    Expired { plan: PlanId, at: MissionTime },
    /// A plan entered a queue and is waiting for a person (DN-31 §5.3 and §6.2,
    /// GAP-132).
    ///
    /// **Additive**, so a client that does not know the variant ignores it rather than
    /// failing (`docs/gungnir-api-v1.md`, "Enum variants may be added"), and a desktop
    /// that does builds the node's queue from the stream without polling for it.
    ///
    /// It carries what a queue row is judged by, because a reader that had to ask the
    /// node for the rest would be back to polling: which item, which plan, the layer
    /// whose window governs the deadline, every role it is offered to in escalation
    /// order, and the two deadlines. `None` for a deadline means the layer configures
    /// none, which is DN-08's silence-preserves default and not "no deadline yet".
    Queued {
        item: PendingApprovalId,
        plan: PlanId,
        layer: EffectorLayer,
        offered_to: Vec<String>,
        expires_at: Option<MissionTime>,
        escalate_at: Option<MissionTime>,
    },
    /// Offered to a role with the authority or the attention to take it. The item
    /// does not leave the original role's view.
    Escalated {
        plan: PlanId,
        to_role: String,
        at: MissionTime,
    },
}

/// Engagement lifecycle, for the journal and the after-action measures
/// (docs/design/DN-06-engagement-and-effect.md).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum EngagementEvent {
    Opened {
        decision: DecisionId,
        plan: PlanId,
        track: TrackId,
    },
    Executing {
        decision: DecisionId,
        at: MissionTime,
    },
    /// Closed with its outcome. The outcome is a string here rather than the
    /// facade's enum because the model may not depend on a service facade; the
    /// facade's `EngagementState` is the authority on the vocabulary.
    Closed {
        decision: DecisionId,
        outcome: String,
        at: MissionTime,
    },
}

/// The outcome vocabulary [`EngagementEvent::Closed`] carries (DN-06 §5, GAP-043).
///
/// The facade's `EngagementState` is the authority; these are its spellings on the bus,
/// with the evidence source in the words, so the reporting layer can count track-inferred
/// and corroborated outcomes apart (DN-06 §8, the fourth criterion) without an edge to
/// the facade.
pub mod engagement_outcome {
    pub const EFFECTIVE_CORROBORATED: &str = "effective (effector or operator evidence)";
    pub const EFFECTIVE_TRACK_INFERRED: &str = "effective (track-lifecycle evidence)";
    pub const INEFFECTIVE_CORROBORATED: &str = "ineffective (effector or operator evidence)";
    pub const INEFFECTIVE_TRACK_INFERRED: &str = "ineffective (track-lifecycle evidence)";
    pub const INDETERMINATE: &str = "indeterminate";
    pub const ABORTED: &str = "aborted";
}

/// A handoff's life (docs/design/DN-07-handoff.md §5, GAP-040). **Never `Delivered` from
/// this side until a transport carries it**: today a handoff is issued and is either
/// manual (no endpoint configured, a radio call the operator must make) or undelivered
/// (an endpoint is configured and nothing carries it there).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum HandoffEvent {
    Issued {
        decision: DecisionId,
        endpoint: Option<String>,
        at: MissionTime,
    },
    Manual {
        decision: DecisionId,
        at: MissionTime,
    },
    Undelivered {
        decision: DecisionId,
        endpoint: String,
        reason: String,
        at: MissionTime,
    },
    /// The endpoint accepted it (GAP-040's transport, 2026-09-06), on the attempt named.
    Delivered {
        decision: DecisionId,
        endpoint: String,
        attempts: u32,
        at: MissionTime,
    },
    /// The endpoint refused it. The decision stands; delivery is what failed, and it is
    /// not retried: a refusal is an answer.
    Refused {
        decision: DecisionId,
        endpoint: String,
        reason: String,
        at: MissionTime,
    },
    /// The effector reported back, through the node's `POST /v3/handoffs/{decision}/report`
    /// (GAP-040). On every desktop's stream; applied by the one that issued the handoff,
    /// which is the only one that can say whether the decision is known.
    Reported {
        decision: DecisionId,
        /// The endpoint the certificate speaks for, or `operator:<id>` for a report a
        /// person keyed in from the radio.
        endpoint: String,
        report: crate::handoff::EffectorReport,
        at: MissionTime,
    },
}

/// After-action review lifecycle (docs/design/DN-20-after-action-review.md, GAP-049).
///
/// **Every state change names who made it**, or carries `None` when nobody was signed
/// in -- recorded as that rather than attributed to a default operator, the same rule
/// the decision record follows.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ReviewEvent {
    Opened {
        session: crate::SessionId,
        operator: Option<String>,
        at: MissionTime,
    },
    FindingRecorded {
        session: crate::SessionId,
        finding: u64,
        /// The kind in the workflow's serialised spelling; findings are counted by kind
        /// and a practice finding must never be read as a defect.
        kind: String,
        /// The moment in the session the finding refers to, when the reviewer was at
        /// one. A finding without it cannot be seeked to.
        refers_to: Option<MissionTime>,
        operator: Option<String>,
        at: MissionTime,
    },
    FindingPromoted {
        session: crate::SessionId,
        finding: u64,
        gap: String,
        operator: Option<String>,
        at: MissionTime,
    },
    Concluded {
        session: crate::SessionId,
        findings: usize,
        operator: Option<String>,
        at: MissionTime,
    },
    Closed {
        session: crate::SessionId,
        operator: Option<String>,
        at: MissionTime,
    },
}

/// The policy verdict as the decision record carries it (MOE-05, GAP-047).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum VerdictSummary {
    Approved,
    Denied { reason: String },
    RequiresHumanApproval,
}

/// System health as it changed (MOE-06, GAP-047). Published on the transition, not
/// every frame: the record says when a backend degraded and when it came back, and a
/// decision between the two was taken with the degraded state on the record.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum HealthEvent {
    Changed {
        tracking_healthy: bool,
        intercept_healthy: bool,
        ingest_healthy: bool,
        at: MissionTime,
    },
}

impl HealthEvent {
    /// True when any backend is reporting unhealthy.
    #[must_use]
    pub fn is_degraded(&self) -> bool {
        match self {
            HealthEvent::Changed {
                tracking_healthy,
                intercept_healthy,
                ingest_healthy,
                ..
            } => !(*tracking_healthy && *intercept_healthy && *ingest_healthy),
        }
    }
}

/// A rehearsal: a session replayed on this desktop (MOE-12, GAP-047). Published on the
/// **live** session's bus, so the record says which watch rehearsed what.
/// A warning owed to an asset (docs/design/DN-03-warning.md, GAP-042). Every state
/// change carries a mission time; a waiver names the person.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum WarningEvent {
    Raised {
        asset: crate::AssetId,
        track: crate::TrackId,
        due_by: MissionTime,
        at: MissionTime,
    },
    Sent {
        asset: crate::AssetId,
        track: crate::TrackId,
        at: MissionTime,
    },
    /// The warned party answered (DN-03 §5 rule 2, GAP-042).
    ///
    /// §6 named this variant when the note was written and the code never gained it, so
    /// `Warning::acknowledged` had no way to be reached from outside the desktop that
    /// raised the warning. It is the acknowledgement arriving over the v3 transport:
    /// `party` is the warning channel the acknowledging certificate speaks for, or
    /// `operator:<id>` when a person keyed in what came over the radio, and `at` is the
    /// time **that party gave**. The envelope's own mission time is when this deployment
    /// recorded it, so a claimed time and a recorded time are both on the record and
    /// neither is invented.
    Acknowledged {
        asset: crate::AssetId,
        track: crate::TrackId,
        party: String,
        at: MissionTime,
    },
    Failed {
        asset: crate::AssetId,
        track: crate::TrackId,
        reason: String,
        at: MissionTime,
    },
    Late {
        asset: crate::AssetId,
        track: crate::TrackId,
        at: MissionTime,
    },
    Waived {
        asset: crate::AssetId,
        track: crate::TrackId,
        operator: String,
        reason: String,
        at: MissionTime,
    },
    Closed {
        asset: crate::AssetId,
        track: crate::TrackId,
        final_state: String,
        at: MissionTime,
    },
}

/// A launch warning: a peer's statement that something has been launched
/// (docs/design/DN-16-peer-sources.md §5, GAP-009).
///
/// **Separate from [`WarningEvent`], which is a different thing with the same word on
/// it.** A `WarningEvent` is an obligation *this* deployment owes a defended asset it
/// protects (DN-03); a `LaunchWarningEvent` is a message a *peer* sent us about
/// something it saw. Folding the two together would put a peer's claim into the ledger
/// of obligations this deployment is measured against, and MOE-01 counts that ledger.
///
/// **None of these variants creates a track.** DN-16 §5: "a track we have not observed
/// is a track we cannot maintain". The type carries no kinematic state to build one
/// from, and nothing in the workspace turns one into a [`crate::DetectionView`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum LaunchWarningEvent {
    /// Published by a deployment warning its peers.
    ///
    /// This is the form that crosses the wire, gated outbound by the agreement and the
    /// marking together (DN-18 §5). **Nothing in this workspace issues one**: the
    /// variant exists so a peer's warning has a shape to arrive in, and a producer was
    /// not invented to make the path look built.
    Issued(crate::LaunchWarningReport),
    /// Taken off a peer's stream and admitted: the record of what we were told, by
    /// whom, and when we heard it.
    Received(crate::PeerLaunchWarning),
    /// Taken off a peer's stream and refused, with the reason kept verbatim.
    ///
    /// On the record rather than dropped, for the reason DN-16 §5 quarantines a
    /// malformed peer track rather than discarding it: a peer sending warnings we
    /// cannot read is a fault an operator has to be able to see, and silence looks
    /// exactly like a peer that saw nothing.
    Refused {
        /// The configured name of the peer, ours rather than anything it sent.
        peer: String,
        reason: String,
        at: MissionTime,
    },
}

/// The node link on a connected desktop (GAP-050, D-23): fell back to embedded
/// services after the node went silent, and answered again afterwards.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum LinkEvent {
    FellBack {
        endpoint: String,
        silent_s: f64,
        at: MissionTime,
    },
    Restored {
        endpoint: String,
        fallback_since: MissionTime,
        at: MissionTime,
    },
    /// A person switched the desktop back to the node after seeing the reconciliation
    /// (GAP-050, D-15). `node_history` is false when the node's journal for the outage
    /// could not be fetched and the switch was made on the desktop's record alone.
    SwitchedBack {
        endpoint: String,
        at: MissionTime,
        merged: usize,
        conflicts: usize,
        node_history: bool,
    },
    /// A person resolved one conflicting decision from an outage (GAP-050, D-03):
    /// which side's decision stands on the record, and who said so.
    ///
    /// Since the GAP-067 walk (2026-09-16) only a conflict the arbitration rule could not
    /// rank reaches a person; the rest are [`LinkEvent::ConflictArbitrated`].
    ConflictResolved {
        plan: crate::PlanId,
        kept_local: bool,
        operator: Option<String>,
        at: MissionTime,
    },
    /// The arbitration rule resolved one conflicting decision from an outage, and no
    /// person did (D-03, DN-10 §9; the GAP-067 walk, 2026-09-16).
    ///
    /// **Not [`LinkEvent::ConflictResolved`], on purpose.** That one records a person's
    /// choice, audited under `plan.decide` and naming who made it; this one records a
    /// rule's, audited as nobody's decision and naming nobody, because nobody chose. An
    /// after-action review that could not tell the two apart could not say whether a
    /// person ever looked at the conflict.
    ///
    /// **Both sides travel with the verdict**, as the rule read them. The node's half of
    /// an outage lives in the node's journal, and a ground that could only be checked
    /// against another deployment's record is a ground nobody reviewing this one can check.
    ConflictArbitrated {
        plan: crate::PlanId,
        /// Whether this desktop's side was kept; `false` means the node's.
        kept_local: bool,
        ground: crate::arbitration::ArbitrationGround,
        /// This desktop's side, which the rule reads first.
        local: crate::arbitration::ConflictSide,
        /// The node's side.
        remote: crate::arbitration::ConflictSide,
        at: MissionTime,
    },
    /// Two engagements of one track were opened on the two sides of an outage (D-58,
    /// DN-31 §5.3 and §6.8; GAP-134).
    ///
    /// **Raised whatever the plans were and whatever the arbitration rule decided about
    /// any of them**, because choosing which record stands does not undo an effect in the
    /// world. A cut-off desktop plans from its own picture, so its plans never share
    /// identifiers with the node's and [`LinkEvent::ConflictArbitrated`] can never find
    /// this: the two sides agree about every plan they hold and still both engaged the
    /// same track. That is the gap D-58 exists to close, and it is an incident for a
    /// person rather than something the rule resolves -- nothing here says which
    /// engagement stands, because both happened.
    BothActed {
        track: crate::TrackId,
        /// The engagement this desktop opened while it was cut off.
        local: EngagementSide,
        /// The engagement the node opened meanwhile.
        remote: EngagementSide,
        at: MissionTime,
    },
    /// D-15's delegations lapsed on a desktop that has been cut off from its node for
    /// longer than `policy.delegation.disconnected_lapse_s` (DN-31 §6.7; GAP-134).
    ///
    /// **Not a panel's note.** A lapse withdraws the delegated role's offer from every
    /// queue item the delegation was the only grant for, so this records a change in what
    /// those items are actionable by, and `withdrawn` names each one. An empty
    /// `withdrawn` is a real answer: the delegations lapsed and nothing was waiting on
    /// one.
    DelegationsLapsed {
        endpoint: String,
        /// When this desktop lost the node, which is when the interval started running.
        cut_off_since: MissionTime,
        /// The interval that has now elapsed, or `None` where the baseline stated none
        /// and no delegation survived the disconnection at all.
        lapse_s: Option<f64>,
        /// The plans whose queue items stopped being actionable by the role they had
        /// been delegated to.
        withdrawn: Vec<crate::PlanId>,
        at: MissionTime,
    },
}

/// One side of a both-acted incident: the engagement one side of an outage opened
/// (D-58, GAP-134).
///
/// **Beside [`LinkEvent`] rather than in `crate::arbitration`**, where
/// [`crate::arbitration::ConflictSide`] lives, because nothing ranks these two against
/// each other. A `ConflictSide` carries what D-03's rule reads -- an outcome, a role, a
/// time -- so that one side can be kept over the other. Both sides of this happened, and
/// putting the type where the rule's inputs live would invite a later reader to rank them.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EngagementSide {
    /// The decision the engagement was opened on.
    pub decision: DecisionId,
    /// The plan that decision decided. The two sides name different plans in the case
    /// this event exists for, which is why the incident is keyed on the track.
    pub plan: PlanId,
    /// When the engagement was opened, as that side's journal timed it.
    pub at: MissionTime,
}

/// A seeded session for a usability round (GAP-089). **The first record of a seeded
/// session**, so nothing that follows can be read as an operation.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum RehearsalEvent {
    Started {
        seed: String,
        seed_sha256: String,
        at: MissionTime,
    },
}

/// A replay session being opened or closed.
///
/// `Closed` carries how many envelopes were stepped through, so the journal records what
/// a replay actually covered and not merely that one happened.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ReplayEvent {
    Opened {
        session: crate::SessionId,
        at: MissionTime,
    },
    Closed {
        session: crate::SessionId,
        /// Envelopes stepped through before it was closed.
        stepped: usize,
        at: MissionTime,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arbitration::{ArbitrationGround, ConflictSide, SideOutcome};

    /// The deciding role is additive: a `Decided` journaled before the field existed still
    /// reads, as a decision whose role was never recorded. The older shape is made by
    /// taking the field out of today's encoding rather than typed by hand, so the test
    /// cannot pass against a shape no journal ever held.
    #[test]
    fn a_decision_journaled_before_roles_were_recorded_still_reads_with_no_role() {
        let decided = CommandEvent::Decided {
            plan: PlanId(7),
            decision: DecisionId(3),
            accepted: false,
            operator: Some("7".into()),
            role: None,
            verdict: VerdictSummary::RequiresHumanApproval,
            rationale: Some("friendly airliner".into()),
            request: None,
            origin: None,
        };
        let mut before = serde_json::to_value(&decided).expect("encode");
        let removed = before
            .get_mut("Decided")
            .and_then(serde_json::Value::as_object_mut)
            .and_then(|fields| fields.remove("role"));
        assert_eq!(
            removed,
            Some(serde_json::Value::Null),
            "the field is written"
        );
        let read: CommandEvent = serde_json::from_value(before).expect("an older line reads");
        assert_eq!(read, decided);

        let now = CommandEvent::Decided {
            plan: PlanId(7),
            decision: DecisionId(4),
            accepted: true,
            operator: Some("7".into()),
            role: Some("Supervisor".into()),
            verdict: VerdictSummary::RequiresHumanApproval,
            rationale: None,
            request: None,
            origin: None,
        };
        let json = serde_json::to_string(&now).expect("encode");
        assert_eq!(
            serde_json::from_str::<CommandEvent>(&json).expect("decode"),
            now
        );
    }

    /// GAP-132, DN-31 §5.3: the request key and the origin are additive in exactly the way
    /// the deciding role was, so `SCHEMA_VERSION` stands. The older shape is made by taking
    /// both fields out of today's encoding, so the test cannot pass against a shape no
    /// journal ever held.
    #[test]
    fn a_decision_journaled_before_request_keys_still_reads_with_neither() {
        let decided = CommandEvent::Decided {
            plan: PlanId(7),
            decision: DecisionId(3),
            accepted: true,
            operator: Some("7".into()),
            role: Some("Operator".into()),
            verdict: VerdictSummary::RequiresHumanApproval,
            rationale: None,
            request: None,
            origin: None,
        };
        let mut before = serde_json::to_value(&decided).expect("encode");
        let fields = before
            .get_mut("Decided")
            .and_then(serde_json::Value::as_object_mut)
            .expect("a struct variant");
        for field in ["request", "origin"] {
            assert_eq!(
                fields.remove(field),
                Some(serde_json::Value::Null),
                "{field} is written, so removing it is the older shape"
            );
        }
        let read: CommandEvent = serde_json::from_value(before).expect("an older line reads");
        assert_eq!(read, decided);

        // Struct-variant update syntax is not a thing, so the one field that differs is
        // set by rebuilding: what the assertion is about is that the key survives the
        // round trip, not how the value was made.
        let CommandEvent::Decided {
            plan,
            decision,
            accepted,
            operator,
            role,
            verdict,
            rationale,
            origin,
            ..
        } = decided
        else {
            unreachable!("built as a decision just above")
        };
        let with_key = CommandEvent::Decided {
            plan,
            decision,
            accepted,
            operator,
            role,
            verdict,
            rationale,
            origin,
            request: Some(RequestId::new("console-2/17").expect("a key")),
        };
        let json = serde_json::to_string(&with_key).expect("encode");
        assert_eq!(
            serde_json::from_str::<CommandEvent>(&json).expect("decode"),
            with_key
        );
    }

    /// GAP-132: a queued item round-trips with both deadlines and every role it is offered
    /// to, which is what a desktop builds its row from without polling (DN-31 §5.3).
    #[test]
    fn a_queued_item_round_trips_with_its_deadlines_and_offers() {
        let event = CommandEvent::Queued {
            item: PendingApprovalId(0x0199_5a3b_7c2d_7e4f_8a1b_2c3d_9f3a_61c2),
            plan: PlanId(11),
            layer: EffectorLayer::Point,
            offered_to: vec!["Operator".into(), "Supervisor".into()],
            expires_at: Some(MissionTime(130.0)),
            escalate_at: None,
        };
        let json = serde_json::to_string(&event).expect("encode");
        assert!(
            json.contains("01995a3b-7c2d-7e4f-8a1b-2c3d9f3a61c2"),
            "the item goes over as the hyphenated form (D-60): {json}"
        );
        assert_eq!(
            serde_json::from_str::<CommandEvent>(&json).expect("decode"),
            event
        );
    }

    /// The rule's verdict round-trips with both sides and its ground, which is what makes
    /// it reviewable from the record alone.
    #[test]
    fn a_rule_verdict_round_trips_with_both_sides() {
        let event = LinkEvent::ConflictArbitrated {
            plan: PlanId(7),
            kept_local: false,
            ground: ArbitrationGround::DecisionOverExpiry,
            local: ConflictSide {
                outcome: SideOutcome::Expired,
                operator: None,
                role: None,
                at: MissionTime(110.0),
            },
            remote: ConflictSide {
                outcome: SideOutcome::Accepted,
                operator: Some("9".into()),
                role: None,
                at: MissionTime(111.5),
            },
            at: MissionTime(150.0),
        };
        let json = serde_json::to_string(&event).expect("encode");
        assert_eq!(
            serde_json::from_str::<LinkEvent>(&json).expect("decode"),
            event
        );
    }
}

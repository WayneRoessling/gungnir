// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! v3 external contract. Read paths: a snapshot and an event stream. Write paths:
//! detection submission and plan decisions, both authorized per caller through
//! `gungnir-security`. Every payload carries `gungnir_model::SCHEMA_VERSION` so a
//! client can refuse data from an incompatible node.
//!
//! Version 2, 2026-09-05: the plan type changed shape so a plan can be an intercept
//! or a fires task (docs/design/DN-05-fires.md). Under the contract's own
//! compatibility rules that needs a new schema version and a new path version, and
//! the owner took option B: replace outright, no deprecated mirror. Removing `/v1`
//! meets the rule's condition rather than excepting it, because the transport is not
//! in the workspace and no client is deployed against it (docs/gungnir-api-v1.md,
//! "Version 2, decided 2026-09-05").
//!
//! Version 3, 2026-09-17: decision, plan and queue-item identifiers became UUID v7 written
//! as hyphenated strings (D-56, D-60; GAP-130), so every payload carrying a plan or a
//! decision changed type and the path moved whole. `/v2` is not removed: each of its
//! routes answers `410 Gone` naming its successor, after authenticating the caller as the
//! successor does (`crate::transport::router`; docs/gungnir-api-v1.md, "Version 3").

use gungnir_model::events::VerdictSummary;
use gungnir_model::{
    BearingRayView, CollectionRequirement, DecisionId, DetectionView, EffectorLayer, ExchangeItem,
    MissionTime, PendingApprovalId, PipelineStatsView, PlanId, PlanView, Releasability,
    SystemHealth, TrackView, SCHEMA_VERSION,
};
use gungnir_security::OperatorId;

pub use gungnir_eventing::Envelope as EventFrame;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SnapshotResponse {
    pub schema_version: u32,
    pub tracks: Vec<TrackView>,
    pub plan: Option<PlanView>,
    pub health: SystemHealth,
    /// The collection requirements this node holds (DN-11 §6, GAP-005).
    ///
    /// Additive, and defaulted so a client written against the first v2 payload still
    /// decodes. Empty means none stated, which is different from a node that does not
    /// track them -- and every node does, since GAP-005.
    #[serde(default)]
    pub requirements: Vec<CollectionRequirement>,
    /// Bearings the node's pipeline has retained but matched to no track (GAP-096's wire
    /// contract; DN-27 §5 rule 3), alongside `tracks`.
    ///
    /// **Additive and defaulted**, the same rule `requirements` above already follows
    /// (`docs/gungnir-api-v1.md`, "Adding a field with a default is compatible"): a
    /// client built before this field existed still decodes the payload, reading none,
    /// which is the same honest empty answer `TrackingService::bearing_rays`'s own
    /// default gives a backend with no pipeline behind it. No `SCHEMA_VERSION` bump,
    /// because nothing that already read this payload is misled by the addition.
    #[serde(default)]
    pub bearing_rays: Vec<BearingRayView>,
    /// The node pipeline's own bearing counters (GAP-096's wire contract), mirroring
    /// `TrackingService::pipeline_stats` locally.
    ///
    /// **Additive and defaulted**, exactly as `bearing_rays` above. A
    /// [`gungnir_model::PipelineStatsView`], not `gungnir_fusion_async::PipelineStats`
    /// itself -- see that type's own doc comment for why.
    #[serde(default)]
    pub pipeline_stats: PipelineStatsView,
    /// Items removed from this response for the caller's party (DN-17 §5 rule 3,
    /// GAP-062). Zero for an operator inside the deployment. Never silent: a peer that
    /// is told its picture is partial can act on that; one that is not believes it has
    /// the whole picture.
    #[serde(default)]
    pub withheld: usize,
    /// The node's approval queue, in its own order (DN-31 §5.3, GAP-132).
    ///
    /// **Additive and defaulted**, exactly as `bearing_rays` above: a client built before
    /// the node had a queue still decodes the payload and reads none, which is what a
    /// node with no queue honestly has. No `SCHEMA_VERSION` bump, because nothing that
    /// already read this payload is misled by the addition
    /// (`docs/gungnir-api-v1.md`, "Adding a field with a default is compatible").
    ///
    /// Beside `plan` rather than inside it: the plan is what this node last proposed and
    /// the queue is what is waiting for a person, and a desktop shows the two in
    /// different panels (PN-05 and PN-06).
    #[serde(default)]
    pub queue: Vec<QueueItemView>,
    /// **The node's own clock when it answered** (GAP-140).
    ///
    /// Every deadline on `queue` is this node's mission time, and until now nothing on
    /// the wire said what that was: a desktop drew the countdown against its own clock,
    /// so a console a minute fast showed every item a minute closer to expiry than it
    /// was, and neither side said so. A desktop measures the offset between the two
    /// machines from this and draws the node's deadlines against the node's time.
    ///
    /// **Stamped where the route answers**, not where the loop publishes, so the offset
    /// a desktop takes from it is as close to the transit as this node can make it.
    ///
    /// **Additive and defaulted**, exactly as `queue` above: a node that does not send it
    /// leaves a desktop where it was, drawing against its own clock and saying nothing it
    /// cannot support (`docs/gungnir-api-v1.md`).
    #[serde(default)]
    pub node_time: Option<MissionTime>,
}

impl SnapshotResponse {
    pub fn new(
        tracks: Vec<TrackView>,
        plan: Option<PlanView>,
        health: SystemHealth,
        requirements: Vec<CollectionRequirement>,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            tracks,
            plan,
            health,
            requirements,
            bearing_rays: Vec::new(),
            pipeline_stats: PipelineStatsView::default(),
            withheld: 0,
            queue: Vec::new(),
            // Filled in by the route that answers with it, which is the moment worth
            // stamping (GAP-140).
            node_time: None,
        }
    }

    /// Attach the node's approval queue (DN-31 §5.3, GAP-132), the same builder idiom
    /// [`SnapshotResponse::with_bearing_data`] uses rather than growing `new`'s
    /// positional list again.
    #[must_use]
    pub fn with_queue(mut self, queue: Vec<QueueItemView>) -> Self {
        self.queue = queue;
        self
    }

    /// Attach the node pipeline's retained bearings and counters (GAP-096's wire
    /// contract), the same builder idiom `LiveTrackingService::with_staleness` and
    /// `NodeApi::with_exchange` already use elsewhere in this workspace rather than
    /// growing `new`'s own positional list a fifth and sixth time.
    #[must_use]
    pub fn with_bearing_data(
        mut self,
        bearing_rays: Vec<BearingRayView>,
        pipeline_stats: PipelineStatsView,
    ) -> Self {
        self.bearing_rays = bearing_rays;
        self.pipeline_stats = pipeline_stats;
        self
    }
}

/// Subscribe to envelopes with `seq >= from_seq` (0 for "everything from now").
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SubscribeRequest {
    pub from_seq: u64,
    /// The session token, which every route but `POST /v3/session` requires (DN-23 §6).
    ///
    /// Carried in the frame rather than an `Authorization` header because a WebSocket
    /// client cannot always set headers on the upgrade. Defaulted so a payload written
    /// against the first v2 shape still decodes -- and is then refused for having no
    /// token, which is a clearer failure than one that will not parse.
    #[serde(default)]
    pub token: String,
}

/// `GET /v3/history?since_seq=N` (GAP-050): the envelopes the node retains from `N`
/// onward, for a desktop reconciling an outage. The same window `SubscribeRequest`
/// resumes from, by another door: a `since_seq` older than the window is `410 Gone`,
/// never a shorter list, so a client cannot mistake truncation for completeness.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HistoryResponse {
    pub since_seq: u64,
    pub envelopes: Vec<EventFrame>,
    /// Envelopes withheld for the caller's party (GAP-062); zero for an operator.
    #[serde(default)]
    pub withheld: usize,
}

/// The query half of `GET /v3/history`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HistoryQuery {
    pub since_seq: u64,
}

/// Sign in and receive a session token (`POST /v3/session`, DN-23 §6).
///
/// **The one route reachable without a token**, because it is what establishes identity.
///
/// The passphrase crosses the wire in the clear, which is exactly why a node serves
/// loopback only until GAP-060 puts TLS on the transport.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SessionRequest {
    pub operator: u64,
    pub passphrase: String,
}

/// What `POST /v3/session` returns.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionResponse {
    pub token: String,
    /// Mission time at which the token stops being believed. **Always present**: a
    /// node-issued session always expires (DN-23 §5).
    pub expires_s: f64,
}

/// What `GET /v3/coverage` returns (DN-12 §6, GAP-006).
///
/// **Not a bare `Vec<CoverageGap>`, which is what §6 wrote.** DN-12 §5 puts the sampling
/// spacing and whether terrain masking was applied *on the result*, so a coarse run
/// cannot be mistaken for a fine one -- and a response carrying only the gaps would throw
/// away exactly what that rule exists to preserve. Recorded as a correction to §6.
///
/// And a node that has not computed one says so rather than returning an empty list:
/// "no gaps were found" and "no coverage was computed" are opposite claims about a
/// sector, which is the distinction DN-12 §7 turns on.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum CoverageResponse {
    Computed(gungnir_analytics::CoverageReport),
    /// Nothing was computed, and why. A deployment with no declared local frame origin
    /// or no approaches cannot produce a coverage answer, and an empty list would read as
    /// a clean sector.
    NotComputed {
        reason: String,
    },
}

/// Who the caller is, for `GET /v3/session`.
///
/// Lets a desktop tell an expired session from an unreachable node, which look the same
/// from the outside and mean different things.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionStatus {
    pub operator: u64,
    pub role: String,
    pub expires_s: f64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SubmitDetectionRequest {
    /// The schema version the caller believes it is speaking.
    ///
    /// **Defaults to 0, which is no version and is refused.** A caller that omits it is
    /// one written before this field existed, and it is refused by name rather than left
    /// to succeed or fail on whether its payload happens to deserialise. Defaulting to
    /// the current version instead would make every old client silently claim to be
    /// current, which is the opposite of what the field is for.
    #[serde(default)]
    pub schema_version: u32,
    pub detection: DetectionView,
}

/// Refuse a request whose caller speaks a different canonical model.
///
/// # Why this exists, and why the compatibility rule now depends on it
///
/// `docs/gungnir-api-v1.md`'s rule used to say that changing a field's type needs a new
/// schema version **and** a new path version. The path move is expensive -- every route
/// moves and every client re-points for a change confined to one payload -- so the rule
/// was amended to require it only where a client that does not know about the change
/// could **silently misinterpret** a payload, and to accept a schema bump alone where
/// such a client is cleanly refused.
///
/// **That amendment is only honest if the refusal exists**, and when it was written it
/// did not. The outbound direction had one: a desktop compares a node's snapshot version
/// against its own. No inbound path had anything. `SubmitDetectionRequest` carried a
/// detection and no version, so a machine posting the previous shape was refused only
/// because serde could not read a bare array as an enum -- an accident of that particular
/// change rather than a designed refusal, answering "the body did not decode" and saying
/// nothing about versions. `gungnir_model::check_schema_version` existed with **no caller
/// anywhere in the workspace**.
///
/// # Errors
///
/// [`gungnir_model::ModelError`] naming both versions, so the caller is told what it
/// speaks and what this node speaks rather than being left to guess from a decode failure.
pub fn refuse_other_schema(found: u32) -> Result<(), gungnir_model::ModelError> {
    gungnir_model::check_schema_version(found)
}

/// `POST /v3/sensors/{sensor_id}/task` (GAP-004): a command for the node's registry to
/// issue to its sensor. The node answers with its own task id; acknowledgement,
/// refusal or silence arrive on the event stream as `SensorTaskEvent`s naming that id.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SensorTaskRequest {
    pub command: gungnir_model::SensorCommand,
    #[serde(default)]
    pub requirement: Option<gungnir_model::RequirementId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SensorTaskResponse {
    /// The node's task id, which its `SensorTaskEvent`s name.
    pub task: gungnir_model::SensorTaskId,
}

/// `POST /v3/handoffs/{decision_id}/report` (GAP-040): what the effector says about a
/// handoff. The node puts it on the record as `HandoffEvent::Reported`; the desktop that
/// issued the handoff applies it, and rejects a report on a decision it does not know.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EffectorReportRequest {
    pub report: gungnir_model::handoff::EffectorReport,
}

/// `POST /v3/warnings/{asset_id}/{track_id}/acknowledge` (GAP-042, DN-03 §5 rule 2): the
/// warned party says it was told.
///
/// The pair the warning is keyed by is in the path, because that is what identifies it in
/// the ledger; the body carries only the time. The node puts it on the record as
/// `WarningEvent::Acknowledged`, and the desktop that raised the warning applies it and
/// rejects one naming a pair it never raised.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WarningAcknowledgementRequest {
    /// When the acknowledging party says it was told.
    ///
    /// **Its claim, not this deployment's finding.** The envelope the node publishes
    /// carries the mission time the acknowledgement was recorded here, so the record holds
    /// both and neither is invented -- the same treatment `EffectorReport`'s own `at` gets
    /// beside `HandoffEvent::Reported`'s.
    pub at: MissionTime,
}

/// One product this deployment holds for exchange (DN-18 §5, GAP-065).
///
/// The identity, the time and the marking are typed here because DN-18 §5's two gates and
/// a partner's ability to tell two products apart turn on them. **The body is the owning
/// crate's own canonical serialization**, because the crates that own a warning
/// (`gungnir-workflow`) and a report (`gungnir-reporting`) sit above `gungnir-api` in
/// ARCHITECTURE.md §7.1's one-way direction: naming their types here would be a dependency
/// edge the architecture forbids, and restating their shapes here would be the second,
/// divergent contract this module exists not to have. `gungnir-interop` converts the body
/// to a partner's format at the edge, which is where DN-18 §5 puts conversion.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExchangeProduct {
    /// Identifies the product within its item, so a partner can tell two apart and ask
    /// about one: a warning is its asset and track, a handoff its decision, a report its
    /// own identifier.
    pub id: String,
    /// When this deployment made it.
    pub at: MissionTime,
    /// The marking, which is the second of DN-18 §5's two gates and the one an
    /// implementation shortcut would skip.
    pub releasability: Releasability,
    pub body: serde_json::Value,
}

/// What `GET /v3/exchange/{warnings,reports,handoffs}` returns (DN-18 §5, GAP-065).
///
/// Two states rather than one list, for the reason [`CoverageResponse`] has two: "we hold
/// none of these" and "we hold some and released none of them to you" are opposite claims
/// about a sector, and a bare empty list says the first when it may mean the second.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum ExchangeResponse {
    Held {
        item: ExchangeItem,
        products: Vec<ExchangeProduct>,
        /// Products removed for this caller's party by the agreement or the marking.
        /// **Never silent**: a partner told its list is partial can ask for the rest;
        /// one that is not told believes it has everything (DN-17 §5 rule 3).
        withheld: usize,
        /// When the **least recently refreshed** part of this answer was written, on the
        /// answering node's clock (GAP-145).
        ///
        /// The register holds one set per producer and merges them (GAP-137), so an
        /// answer is only as current as its quietest producer. This is that producer's
        /// last write: a partner comparing it with the age of the engagement it is asking
        /// about can tell a deployment that holds nothing new from one whose console
        /// stopped talking. Absent when nothing has been published for the item, and
        /// absent from an older node that does not send it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        as_of: Option<MissionTime>,
    },
    /// This deployment publishes none of this item, and why: a producer that keeps no
    /// such ledger saying so, which is different from reporting that there are none.
    ///
    /// **The merged answer** (GAP-137): this is what a partner reads only when no
    /// producer in the register holds any, and it carries what each of them said. A node
    /// keeps no warnings and no reports; since GAP-132 it does keep handoffs, and claims
    /// an empty set for them rather than this.
    NotHeld {
        item: ExchangeItem,
        reason: String,
        /// When the least recently refreshed of those claims was written (GAP-145), as on
        /// [`ExchangeResponse::Held`]. Absent when nothing has been published at all,
        /// which is the one answer that has no age.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        as_of: Option<MissionTime>,
    },
}

/// `POST /v3/exchange/{warnings,reports,handoffs}` (DN-18 §5 amendment 2, GAP-065): the
/// desktop that holds an item posts what it currently holds, and the node replaces its
/// held set for that item with this list.
///
/// **A replacement of this caller's own set, not an addition to it** (GAP-137).
/// [`crate::transport::NodeApi::publish_exchange`] overwrites rather than appends, so the
/// caller sends its whole current set each time -- the same contract
/// [`ExchangeResponse::Held`]'s own doc comment describes from the read side. What it
/// does not touch is any other producer's set: the node's own handoffs and every other
/// desktop's stay where they are, and a read merges them.
///
/// The item is not a field here: it is already in the path, exactly as the three `GET`
/// routes this shares a path with take no item field either. Neither is the producer,
/// which is the common name the connection was verified under and so cannot be claimed.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PublishExchangeRequest {
    pub products: Vec<ExchangeProduct>,
}

/// An operator's decision on a plan the node proposed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ApprovalRequest {
    pub plan: PlanId,
    pub accepted: bool,
    pub operator: OperatorId,
}

/// One item in a node's queue, as `GET /v3/queue` and the snapshot carry it
/// (DN-31 §5.2, GAP-132).
///
/// Everything PN-06 draws a row from, so a desktop needs no second request to show one:
/// which item, the plan, what policy said, the layer whose window governs the deadline,
/// when it was submitted, when it expires and escalates, every role it is offered to, and
/// whether it was pre-delegated.
///
/// **`offered_to` is a list, not the current role.** DN-10 amendment 1 c: escalation adds
/// a role without removing the first, and "whoever decides first ends it" is only
/// meaningful if a reader can see all of them.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QueueItemView {
    pub item: PendingApprovalId,
    pub plan: PlanView,
    pub verdict: VerdictSummary,
    /// The layer whose window governs the deadline (`gungnir_command::governing_layer`).
    pub layer: EffectorLayer,
    pub submitted: MissionTime,
    /// `None` means the governing layer configures no expiry, which is DN-08's
    /// silence-preserves default and not "no deadline yet".
    pub expires_at: Option<MissionTime>,
    pub escalate_at: Option<MissionTime>,
    /// Every role the item is offered to, in escalation order (DN-10 amendment 1 c).
    pub offered_to: Vec<String>,
    /// Actionable for the Operator from submission under D-15's delegation. **It still
    /// expires and escalates** (D-59), so this changes what a panel says and nothing
    /// about the deadlines beside it.
    pub pre_delegated: bool,
    pub priority: f32,
}

/// What a person chose, as PN-07 offers it (DN-31 §5.2).
///
/// An override records the queued plan under the override permission; a substituted
/// assignment is not built and DN-31 §11 does not design one.
///
/// Externally tagged and kebab-cased, because this is a *field* of
/// [`DecisionRequest`] rather than a body of its own: `"choice": "accept"` and
/// `"choice": { "reject": { "reason": "..." } }`. An internal tag would write the word
/// "choice" twice for a client to read once.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DecisionChoice {
    Accept,
    Override,
    /// A person considered this and declined it, and said why. An empty reason is
    /// refused `400` (DN-10 §3): MOE-01 tells a considered rejection from an abandoned
    /// decision by the reason alone.
    Reject {
        reason: String,
    },
}

/// A person's decision on one queue item: the body of
/// `POST /v3/queue/{item}/decision` (DN-31 §5.2).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DecisionRequest {
    /// Chosen by the client and journaled with the decision, so a retry is the same
    /// request rather than a second decision. What makes a retry after a `504` safe.
    pub request: gungnir_model::RequestId,
    /// The item, which is also in the path. Carried here as well so a body cannot be
    /// replayed against a different item: the route refuses a mismatch rather than
    /// silently preferring one of the two.
    pub item: PendingApprovalId,
    pub choice: DecisionChoice,
}

/// `201`: the decision this request recorded, or recorded before under the same
/// `request` (DN-31 §5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DecisionRecorded {
    pub decision: DecisionId,
}

/// `409`: why the item takes no decision now (DN-31 §5.2).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "refused", rename_all = "kebab-case")]
pub enum DecisionRefused {
    /// Somebody decided first; their decision stands. Named in full so PN-07 can say who
    /// decided, as which role and when (DN-31 §6.6).
    AlreadyDecided {
        decision: DecisionId,
        /// `None` when the decision that stands was taken with nobody signed in, which a
        /// desktop's own queue allows (DN-23 §5 rule 1). Never invented.
        operator: Option<String>,
        role: Option<String>,
        at: MissionTime,
    },
    /// The window closed (DN-10 §6). **Not a rejection**: nobody chose.
    Expired { at: MissionTime },
}

/// A decision as the machine that took it recorded it (DN-31 §5.2: "identifiers, plan,
/// choice, operator, role, time"; GAP-134).
///
/// **The forwarding machine's record, whole, and nothing this node derived.** Every field
/// is what that machine's `DecisionRecord` holds, so the node's record of the outage is the
/// desktop's record rather than a reconstruction of it: the plan with its assignments, the
/// verdict it was queued under, what the person chose and why, who decided as which role,
/// and when. A field this node could fill in for itself would be a field the two records
/// could come to disagree about.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DecisionRecordView {
    /// The forwarding machine's identifier for the decision, minted there (D-56). The key
    /// the node's exactly-once is taken on: unique across machines and restarts.
    pub decision: DecisionId,
    /// The item it ended on the forwarding machine's own queue, which this node never
    /// issued. Carried so the record is whole and never looked up here; `None` for a
    /// decision no queue item named.
    #[serde(default)]
    pub item: Option<PendingApprovalId>,
    pub plan: PlanView,
    /// What policy said when the plan was queued. Only `RequiresHumanApproval` is ever
    /// queued, so only it is ever decided, and the route refuses anything else.
    pub verdict: VerdictSummary,
    pub choice: DecisionChoice,
    /// `None` where nobody was signed in at that console (DN-23 §5 rule 1). Never
    /// invented, here or on the node.
    pub operator: Option<String>,
    pub role: Option<String>,
    /// The client key, where a route carried one; a decision taken at a cut-off
    /// desktop's own console carries none.
    #[serde(default)]
    pub request: Option<gungnir_model::RequestId>,
    /// When it was decided, on the forwarding machine's clock. When this node learned of
    /// it is its envelope's time, and both are kept.
    pub at: MissionTime,
}

/// A decision a desktop took while it was cut off from this node, forwarded on
/// reconnect (DN-31 §5.2, §6.8; GAP-134).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ForwardedDecision {
    pub record: DecisionRecordView,
    /// The desktop's machine identity: the common name of the identity it presents on its
    /// link (D-02). See GAP-141 for what that name can and cannot tell apart today.
    pub origin: String,
    /// What the outage's reconciliation settled about this decision's plan, where the
    /// plan was in conflict (D-53; MT-10 step 5). `None` where it was not.
    ///
    /// **Additive, defaulted, and beyond §5.2's two fields**: §6.8 asks for the rule's
    /// verdicts and a person's resolutions to be forwarded "so the node's record says what
    /// stands", and §7 gives exactly one route to forward on. A decision in conflict
    /// travels with its settlement in the same element, so the node never holds a
    /// conflicting decision without what stands beside it.
    #[serde(default)]
    pub settled: Option<Settlement>,
}

/// What an outage's reconciliation settled about one plan (D-53, DN-31 §6.8).
///
/// Named from the **forwarding desktop's** seat, exactly as that desktop journaled it:
/// `kept_local` is whether its decision stands, and `false` means the node's. The node
/// puts the same fact on its own record under the same event, so a reviewer reading
/// either journal reads one sentence.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "by", rename_all = "kebab-case")]
pub enum Settlement {
    /// D-03's rule ranked the two sides and kept one. Nobody was asked; the fields are
    /// `LinkEvent::ConflictArbitrated`'s.
    Rule {
        kept_local: bool,
        ground: gungnir_model::arbitration::ArbitrationGround,
        local: gungnir_model::arbitration::ConflictSide,
        remote: gungnir_model::arbitration::ConflictSide,
    },
    /// A person permitted `plan.decide` kept one side; `LinkEvent::ConflictResolved`'s
    /// fields.
    Person {
        kept_local: bool,
        operator: Option<String>,
    },
}

/// `202` from `POST /v3/decisions/forwarded`: the batch is on the node's record
/// (DN-31 §7).
///
/// **Counted, so a retry can be seen to have recorded nothing.** A batch sent again after
/// a connection failed answers with every decision `already_held` and none `recorded`,
/// which is what "forwarding twice records nothing new" looks like from the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ForwardAccepted {
    /// Decisions new to the node's record.
    pub recorded: usize,
    /// Decisions it already held, identical, acknowledged and not recorded again.
    pub already_held: usize,
    /// Settlements put on the node's record by this batch.
    pub settled: usize,
}

/// `409` from `POST /v3/decisions/forwarded`: a record in the batch contradicts one the
/// node already holds under the same identifier (DN-31 §7).
///
/// **Nothing in the batch was applied.** The node takes an outage whole or not at all, so
/// a desktop that sees this has left the node holding none of the batch rather than the
/// part before the contradiction.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "refused", rename_all = "kebab-case")]
pub enum ForwardRefused {
    /// The same decision identifier, a different record. What the node holds stands.
    Contradicts {
        decision: DecisionId,
        held: DecisionRecordView,
    },
    /// The identifier names a window that closed on this node with nobody deciding.
    /// **Its own variant rather than a record**, because a record view carries a person's
    /// choice and an expiry is not one (DN-10 §3): naming it as a rejection would put a
    /// refusal nobody made into the answer.
    ContradictsAnExpiry {
        decision: DecisionId,
        plan: PlanId,
        at: MissionTime,
    },
    /// The same plan settled two different ways.
    SettledOtherwise { plan: PlanId, held: Settlement },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_carries_schema_version() {
        let s = SnapshotResponse::new(Vec::new(), None, SystemHealth::default(), Vec::new());
        assert_eq!(s.schema_version, SCHEMA_VERSION);
        let json = serde_json::to_string(&s).expect("encode");
        let back: SnapshotResponse = serde_json::from_str(&json).expect("decode");
        assert_eq!(s, back);
    }

    /// GAP-096's wire contract: `bearing_rays`/`pipeline_stats` round-trip like every
    /// other field.
    #[test]
    fn bearing_data_survives_the_wire_round_trip() {
        let ray = BearingRayView {
            sensor: gungnir_model::SensorId(4),
            origin_enu: [10.0, 20.0, 3.0],
            azimuth_rad: 0.4,
            elevation_rad: None,
            azimuth_one_sigma_rad: 0.01,
            valid_until: MissionTime(90.0),
        };
        let stats = PipelineStatsView {
            bearings_offered: 3,
            bearings_retained: 1,
            ..PipelineStatsView::default()
        };
        let s = SnapshotResponse::new(Vec::new(), None, SystemHealth::default(), Vec::new())
            .with_bearing_data(vec![ray], stats);
        let json = serde_json::to_string(&s).expect("encode");
        let back: SnapshotResponse = serde_json::from_str(&json).expect("decode");
        assert_eq!(s, back);
        assert_eq!(back.bearing_rays, vec![ray]);
        assert_eq!(back.pipeline_stats, stats);
    }

    /// Backward compatibility (GAP-096's wire contract): a snapshot encoded before
    /// `bearing_rays`/`pipeline_stats` existed -- the same shape `requirements`' own
    /// comment describes for a client built before that field existed -- still decodes,
    /// degrading to the same honest empty answer `TrackingService::bearing_rays`/
    /// `pipeline_stats` default to locally, rather than failing to parse
    /// (`docs/gungnir-api-v1.md`, "Adding a field with a default is compatible").
    #[test]
    fn an_older_snapshot_with_no_bearing_fields_still_decodes() {
        let current = SnapshotResponse::new(Vec::new(), None, SystemHealth::default(), Vec::new());
        let mut value = serde_json::to_value(&current).expect("encodes");
        let obj = value.as_object_mut().expect("an object");
        assert!(
            obj.remove("bearing_rays").is_some(),
            "the field must exist on the current shape for this test to mean anything"
        );
        assert!(
            obj.remove("pipeline_stats").is_some(),
            "the field must exist on the current shape for this test to mean anything"
        );
        let decoded: SnapshotResponse = serde_json::from_value(value)
            .expect("a payload missing the new fields must still decode");
        assert!(
            decoded.bearing_rays.is_empty(),
            "a missing field must default to empty, not fail"
        );
        assert_eq!(
            decoded.pipeline_stats,
            PipelineStatsView::default(),
            "a missing field must default to the honest empty counters, not fail"
        );
    }
}

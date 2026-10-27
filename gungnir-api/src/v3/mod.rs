// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! v3 external contract. Read paths: a snapshot and an event stream. Write paths:
//! detection submission and a decision on a queue item ([`DecisionRequest`]), both
//! authorized per caller through `gungnir-security`. Every payload carries
//! `gungnir_model::SCHEMA_VERSION` so a client can refuse data from an incompatible node.
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
    BearingRayView, CollectionRequirement, ControlStatusChange, DecisionId, DenseGroupView,
    DetectionView, EffectorLayer, ExchangeItem, HeldLayerView, MissionTime, PendingApprovalId,
    PipelineStatsView, PlanId, PlanStandingView, PlanView, Releasability, RequestId, SystemHealth,
    TrackView, WeaponsControlStatus, SCHEMA_VERSION,
};
use std::collections::BTreeMap;

/// How many of this run's control-status changes a snapshot carries, oldest first
/// (GAP-203). A desktop reads the whole history from the node's journal; what the
/// snapshot is for is the changes a console linking mid-watch needs in front of it, and
/// the key a retry is matched against.
pub const CONTROL_STATUS_CHANGES_CARRIED: usize = 64;

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
    /// The dense group the node's pipeline could not resolve into tracks, as of its last
    /// epoch (GAP-015), mirroring `TrackingService::dense_group` locally. `None` is the
    /// ordinary answer: a resolvable scene.
    ///
    /// **Additive and defaulted**, exactly as `bearing_rays` above: a client built before
    /// the field existed reads none, which is what a node whose mode is off honestly has.
    #[serde(default)]
    pub dense_group: Option<DenseGroupView>,
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
    /// **Whether `plan` answers this node's current picture** (GAP-157, D-94): current,
    /// interim (a one-step answer standing in, GAP-156), stale since when and why, or
    /// never answered. Kept live between snapshots by `InterceptEvent::PlanStanding`.
    ///
    /// Until this a linked desktop drew the node's plan as current whatever the node's
    /// planner said, so a plan the node had stopped being able to refresh reached PN-05
    /// with no stale line and no age. `computed_at` is the node's clock, as `node_time`
    /// and every queue deadline are.
    ///
    /// **Additive and defaulted**, exactly as `node_time` above: `None` from a node that
    /// does not say, which leaves a desktop where it was -- drawing the plan as the
    /// node's latest, with the node's health flag beside it (GAP-161).
    #[serde(default)]
    pub plan_standing: Option<PlanStandingView>,
    /// **The effector layers at hold that are refusing every plan** (GAP-183, D-114;
    /// DN-09 §9), as the node's own desk counts them. Kept live between snapshots by
    /// `InterceptEvent::HeldLayers`; carried here so a desktop that links while a hold is
    /// refusing everything says so from its first frame rather than after the next plan.
    ///
    /// **Additive and defaulted**, as `plan_standing` is: empty from a node that does not
    /// say, which is also what "no layer is refusing every plan" looks like -- the one
    /// ambiguity this field has, and the same one `plan_standing`'s `None` has.
    #[serde(default)]
    pub held_layers: Vec<HeldLayerView>,
    /// **The weapons control status in force on this node, per layer** (GAP-203;
    /// DN-09 §10): the baseline's, with every live change folded over it. Kept live
    /// between snapshots by `PolicyEvent::ControlStatusSet`; carried here so a desktop
    /// that links after a change draws the status the node decides by rather than the
    /// one its own copy of the baseline says.
    ///
    /// **Additive and defaulted**, as `held_layers` is: empty from a node that does not
    /// say, which a desktop reads as "the baseline's", the same ambiguity `held_layers`
    /// carries and for the same reason. **Withheld from a machine party**: what this
    /// deployment decides by is internal, as its queue is.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub control_status: BTreeMap<EffectorLayer, WeaponsControlStatus>,
    /// **This run's live changes, oldest first, at most
    /// [`CONTROL_STATUS_CHANGES_CARRIED`]** (GAP-203): what the node folded from its
    /// journal at start and what it recorded since. Each carries its client request key,
    /// which is how `POST /v3/policy/control-status` answers a retry with the change it
    /// already recorded rather than recording a second one.
    ///
    /// Additive, defaulted and withheld from a machine party, as `control_status` is.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub control_status_changes: Vec<ControlStatusChange>,
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
            dense_group: None,
            withheld: 0,
            queue: Vec::new(),
            // Filled in by the route that answers with it, which is the moment worth
            // stamping (GAP-140).
            node_time: None,
            // Said by the node that publishes the snapshot (GAP-157); a snapshot built
            // with nothing to say about its plan says nothing.
            plan_standing: None,
            // GAP-183: said by the node's desk, attached by the publisher.
            held_layers: Vec::new(),
            // GAP-203: said by the node's baseline and its folded changes, attached by
            // the publisher. Empty is "the node does not say", never "every layer free".
            control_status: BTreeMap::new(),
            control_status_changes: Vec::new(),
        }
    }

    /// Attach the control status in force and this run's changes (GAP-203; DN-09 §10).
    ///
    /// The changes are kept to the newest [`CONTROL_STATUS_CHANGES_CARRIED`], oldest
    /// first, so a long watch cannot grow the snapshot without bound; the node's journal
    /// holds the rest.
    #[must_use]
    pub fn with_control_status(
        mut self,
        in_force: BTreeMap<EffectorLayer, WeaponsControlStatus>,
        mut changes: Vec<ControlStatusChange>,
    ) -> Self {
        if changes.len() > CONTROL_STATUS_CHANGES_CARRIED {
            changes.drain(..changes.len() - CONTROL_STATUS_CHANGES_CARRIED);
        }
        self.control_status = in_force;
        self.control_status_changes = changes;
        self
    }

    /// Attach the node's approval queue (DN-31 §5.3, GAP-132), the same builder idiom
    /// [`SnapshotResponse::with_bearing_data`] uses rather than growing `new`'s
    /// positional list again.
    #[must_use]
    pub fn with_queue(mut self, queue: Vec<QueueItemView>) -> Self {
        self.queue = queue;
        self
    }

    /// Attach the layers at hold that are refusing every plan (GAP-183, D-114).
    #[must_use]
    pub fn with_held_layers(mut self, held: Vec<HeldLayerView>) -> Self {
        self.held_layers = held;
        self
    }

    /// Attach the node pipeline's dense-group estimate (GAP-015), `None` when the mode
    /// did not engage.
    #[must_use]
    pub fn with_dense_group(mut self, dense_group: Option<DenseGroupView>) -> Self {
        self.dense_group = dense_group;
        self
    }

    /// Attach whether the plan answers the node's current picture (GAP-157, D-94).
    #[must_use]
    pub fn with_plan_standing(mut self, standing: PlanStandingView) -> Self {
        self.plan_standing = Some(standing);
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
    Computed(ComputedCoverage),
    /// Nothing was computed, and why. A deployment with no declared local frame origin
    /// or no approaches cannot produce a coverage answer, and an empty list would read as
    /// a clean sector.
    NotComputed {
        reason: String,
    },
}

/// A computed coverage answer: the report, and the acceptance that stands for each of its
/// gaps on this node (GAP-194, D-131).
///
/// **Additive on the wire.** The report's fields are where they were, beside `state`, and
/// `acceptances` is new beside them; a body written before it reads as no gap accepted,
/// which is what such a node held. An accepted gap is still in `gaps` and still counted:
/// the mark names it, it does not remove it (D-118).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ComputedCoverage {
    #[serde(flatten)]
    pub report: gungnir_analytics::CoverageReport,
    /// For each of `report.gaps`, in the same order, the acceptance that stands for it on
    /// this node, or `None`. Empty when nothing stands.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acceptances: Vec<Option<gungnir_model::GapAcceptance>>,
}

impl ComputedCoverage {
    /// A report with no gap accepted.
    #[must_use]
    pub fn unaccepted(report: gungnir_analytics::CoverageReport) -> Self {
        ComputedCoverage {
            report,
            acceptances: Vec::new(),
        }
    }

    /// The acceptance that stands for gap `index`, if any.
    #[must_use]
    pub fn acceptance_of(&self, index: usize) -> Option<&gungnir_model::GapAcceptance> {
        self.acceptances.get(index).and_then(Option::as_ref)
    }
}

/// What `POST /v3/coverage/acceptances` takes (GAP-194, D-131): the gap as the caller drew
/// it from this node's coverage answer, and why it is tolerated.
///
/// **No operator, no role, no revision, no laydown.** The person is whoever the token
/// says, and what the acceptance holds for is the node's own baseline revision and the
/// laydown it marks current, read when the node records it -- not what the caller's
/// console was running.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GapAcceptanceRequest {
    pub gap: gungnir_model::AcceptedGap,
    /// Why, in the commander's words. Required (D-118).
    pub reason: String,
    /// Set when a desktop cut off from this node accepted the gap and forwards it now
    /// (D-131): who accepted it, as that console recorded them. The token is then the
    /// forwarder's, named on the node's audit entry beside this; the acceptance is the
    /// accepting commander's, as a forwarded decision is its decider's (D-55). Absent for
    /// an acceptance made while linked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_offline: Option<OfflineAcceptance>,
}

/// Who accepted a forwarded gap, and when and where, as the cut-off console recorded it
/// (GAP-194, D-131). The strings are `GapAcceptance`'s own, so the node records them as
/// that console wrote them.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OfflineAcceptance {
    pub operator: String,
    pub role: String,
    /// When it was accepted, on the forwarding desktop's clock.
    pub at: gungnir_model::MissionTime,
    /// The desktop's machine identity, as on a forwarded decision.
    pub origin: String,
}

/// The node's answer when it recorded an acceptance: `201`, with the acceptance as the
/// node holds it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GapAcceptanceRecorded {
    pub acceptance: gungnir_model::GapAcceptance,
}

/// Why the node recorded no acceptance, for a request the route itself let through:
/// `409`. Nothing is journaled or audited as accepted for any of these; the refusal's own
/// audit entry is written.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "refused", rename_all = "kebab-case")]
pub enum GapAcceptanceRefused {
    /// The node computes no coverage, so there is no gap to accept.
    NotMeasured { reason: String },
    /// No gap of that shape is on the node's coverage answer now (DN-33 §5's shape): the
    /// caller drew it from a different answer, or it changed before the node saw it.
    GapNotOnAnswer { gap: String },
    /// The gap is already accepted on this node.
    AlreadyAccepted {
        gap: String,
        acceptance: gungnir_model::GapAcceptanceId,
    },
}

impl GapAcceptanceRefused {
    /// The refusal in words, for the caller's panel and the audit entry.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            GapAcceptanceRefused::NotMeasured { reason } => {
                format!("the node computes no coverage ({reason}), so there is no gap to accept")
            }
            GapAcceptanceRefused::GapNotOnAnswer { gap } => {
                format!("{gap} is not on the node's coverage answer, so nothing was accepted")
            }
            GapAcceptanceRefused::AlreadyAccepted { gap, acceptance } => {
                format!("{gap} is already accepted on the node ({acceptance})")
            }
        }
    }
}

/// What `POST /v3/audit/anchors` takes (GAP-208, D-142): heads of the desktop's own audit
/// log, which the node judges against its baseline's audit-signing key and holds.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AuditAnchorsForwarded {
    /// Heads of the desktop's own audit log, oldest first, each with its signature when
    /// the desktop's audit-signing key was unlocked.
    pub heads: Vec<gungnir_model::events::AuditHead>,
    /// What the desktop found checking its file against the heads this node holds for
    /// it, when it has just done so; absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub findings: Option<Vec<String>>,
}

/// The heads a node holds for the desktop asking (GAP-208): for each segment, the one
/// with the most entries.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AuditAnchorsHeld {
    pub heads: Vec<gungnir_model::events::AuditHead>,
}

/// The most heads one `POST /v3/audit/anchors` may carry (GAP-208): a desktop forwards
/// its inventory, one head per segment, and a body past this is not one.
pub const MAX_FORWARDED_ANCHORS: usize = 4_096;

/// What `POST /v3/policy/control-status` takes (GAP-203; DN-09 §10; D-140): a signed-in
/// supervisor or commander puts one layer at a status, live, and says why.
///
/// **No operator, no role, no revision.** The person is whoever the token says, the role
/// is the one that token carries, and the baseline revision the change is made under is
/// the node's own, read when the node records it. The one thing the body may name about a
/// second person is a confirmation, which the node verifies against its own account store
/// exactly as it verifies a sign-in; a confirmation that is only claimed is nothing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ControlStatusRequest {
    /// Chosen by the client and recorded with the change, so a retry after a `504` is
    /// answered with the change it recorded rather than recording a second one (the
    /// decision route's rule, DN-31 §5.2).
    pub request: RequestId,
    pub layer: EffectorLayer,
    pub status: WeaponsControlStatus,
    /// Why, in the person's words. Required: an empty reason is refused `400`.
    pub reason: String,
    /// The second person, where the baseline's two-person rule is on (D-140). Absent
    /// where it is off; refused `409 two-person-required` where it is on and this is
    /// absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmation: Option<ConfirmationRequest>,
}

/// The second person's credential (D-140): the same pair `POST /v3/session` takes, and
/// verified the same way. **Debug redacts the passphrase**, as a request that reaches a
/// log line must.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ConfirmationRequest {
    pub operator: u64,
    pub passphrase: String,
}

impl std::fmt::Debug for ConfirmationRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfirmationRequest")
            .field("operator", &self.operator)
            .field("passphrase", &"<redacted>")
            .finish()
    }
}

/// `201`: the change this request recorded, or recorded before under the same `request`,
/// and the queue items the new status took off the queue (GAP-203; DN-09 §10).
///
/// `withdrawn` is empty on a retry: the first answer carried them, and nothing was
/// withdrawn a second time.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ControlStatusRecorded {
    pub change: ControlStatusChange,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub withdrawn: Vec<WithdrawnItem>,
}

/// One queue item a status change ended, named by its item and its plan so PN-06 can say
/// which row left and why (GAP-203). The change itself is on the stream as
/// `PolicyEvent::ControlStatusSet`, and each withdrawal as `CommandEvent::Withdrawn`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WithdrawnItem {
    pub item: PendingApprovalId,
    pub plan: PlanId,
}

/// Why the node recorded no change (GAP-203; DN-09 §10; D-140). Nothing is journaled or
/// published for any of these; the refusal's own audit entry is written under
/// `weapons.control_status` naming the requester.
///
/// Tagged `refused`, as [`GapAcceptanceRefused`] and [`DecisionRefused`] are, so a desktop
/// reads every refusal body on this transport the same way. The confirmation refusals are
/// `403`, because they are about who is asking; the other two are `409`, because they are
/// about the node's state.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "refused", rename_all = "kebab-case")]
pub enum ControlStatusRefused {
    /// The layer is already at that status: there is no change to record, and recording
    /// one would put a decision nobody needed on the record.
    Unchanged {
        layer: EffectorLayer,
        status: WeaponsControlStatus,
    },
    /// The baseline's `policy.two_person.control_status` is on and no confirmation came.
    TwoPersonRequired,
    /// The confirmation did not verify: an unknown operator or a wrong passphrase,
    /// **deliberately indistinguishable**, as a sign-in's refusal is.
    ConfirmationRejected,
    /// The confirming account verified and its role does not hold
    /// `weapons.control_status`: a second person is a second person who could have made
    /// the change alone.
    ConfirmationNotPermitted { role: String },
    /// The confirmer is the requester. Two credentials for one person is one person.
    SameOperator,
}

impl ControlStatusRefused {
    /// The refusal in words, for the caller's panel and the audit entry.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            ControlStatusRefused::Unchanged { layer, status } => {
                format!("the {layer:?} layer is already at {status:?}; nothing was changed")
            }
            ControlStatusRefused::TwoPersonRequired => {
                "this deployment needs a second person to confirm a control-status change \
                 (D-140), and none confirmed it"
                    .to_owned()
            }
            ControlStatusRefused::ConfirmationRejected => {
                "the confirming credential was rejected".to_owned()
            }
            ControlStatusRefused::ConfirmationNotPermitted { role } => {
                format!("the confirming account's role, {role}, may not set weapons control status")
            }
            ControlStatusRefused::SameOperator => {
                "the confirming operator is the requesting operator; a second person is \
                 somebody else"
                    .to_owned()
            }
        }
    }
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
    /// Built with [`ExchangeProduct::body_of`], never `serde_json::to_value`, so a
    /// non-finite float reaches the partner as DN-18 §15's "value unavailable" object and
    /// not as a `null` (GAP-171, D-103).
    pub body: serde_json::Value,
}

impl ExchangeProduct {
    /// A product's body as a partner is sent it (GAP-171, D-103; DN-18 §15).
    ///
    /// The owning crate's canonical serialization, exactly as `serde_json::to_value`
    /// writes it when every float is finite -- so every product without a NaN or an
    /// infinity goes out as it always did -- and with each non-finite float written as
    /// `{"unavailable": "nan" | "+inf" | "-inf"}` in the number's place. A `Value` cannot
    /// hold a non-finite float at all, and `to_value` writes one as `null` without an error,
    /// which would put a mission report with a NaN measure in front of a partner with that
    /// figure silently blanked, indistinguishable from an absent one.
    ///
    /// # Errors
    ///
    /// What `serde_json::to_value` refuses for any other reason, such as a map whose keys
    /// are not strings.
    pub fn body_of<T: serde::Serialize + ?Sized>(
        value: &T,
    ) -> Result<serde_json::Value, serde_json::Error> {
        gungnir_eventing::nonfinite::to_partner_value(value)
    }
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
    /// What the person deciding was told and acknowledged at their console before acting
    /// on the plan (GAP-193): the rehearsal standing (GAP-107) and each degraded condition
    /// in force (GAP-195, D-132). The node records it on its `DecisionRecord` and on
    /// `CommandEvent::Decided` as given, since only the console knows what it drew; the
    /// console asks the question before it posts.
    ///
    /// **Additive**: defaulted and left out when empty, so a body without it is the body
    /// it always was and `SCHEMA_VERSION` stands.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acknowledged: Vec<gungnir_model::Acknowledgement>,
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
    /// A live status change took the item off the queue (GAP-203, DN-09 §10): the layer
    /// named was put at a status that refuses the plan. Not a rejection and not an
    /// expiry; the change itself is on the stream as `PolicyEvent::ControlStatusSet`.
    Withdrawn {
        layer: EffectorLayer,
        status: WeaponsControlStatus,
        at: MissionTime,
    },
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
    /// What the person deciding acknowledged, as the forwarding machine's record holds it
    /// (GAP-193). Defaulted and left out when empty, as on [`DecisionRequest`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acknowledged: Vec<gungnir_model::Acknowledgement>,
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

/// Most entries one `GET /v3/audit` answers with (GAP-179, D-116). A page, so a segment
/// a long-running node wrote is read a page at a time rather than in one answer the size
/// of the file.
pub const AUDIT_PAGE_LIMIT: u64 = 500;

/// The query half of `GET /v3/audit` (GAP-179, D-116;
/// `docs/design/DN-23-operator-authentication.md` §15). Every field is optional: no query
/// at all asks for the node's last verification and its segment list.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AuditQuery {
    /// Verify the record against the heads the node's journal holds before answering,
    /// rather than answering with the last verification the node ran.
    #[serde(default)]
    pub verify: bool,
    /// A segment to read entries from, by its file name as a verification lists it.
    #[serde(default)]
    pub segment: Option<String>,
    /// The first entry of the page, counting the segment's oldest as 0.
    #[serde(default)]
    pub from: u64,
    /// How many entries at most; [`AUDIT_PAGE_LIMIT`] when absent, and never more.
    #[serde(default)]
    pub limit: Option<u64>,
}

impl AuditQuery {
    /// The page size this query is answered with.
    #[must_use]
    pub fn page_size(&self) -> u64 {
        self.limit
            .map_or(AUDIT_PAGE_LIMIT, |l| l.min(AUDIT_PAGE_LIMIT))
    }

    /// The query string a client sends, `?` included, or empty for no query.
    ///
    /// Written by hand because `reqwest`'s `query` feature is off in §2.9's pin. A segment
    /// name is a file name the node listed (`audit-NNNNNN.jsonl`); one holding a character
    /// that would need encoding is `None`, because no segment is named that way and a
    /// client must not send a query the node would read differently from what it meant.
    #[must_use]
    pub fn to_query_string(&self) -> Option<String> {
        let mut parts = Vec::new();
        if self.verify {
            parts.push("verify=true".to_owned());
        }
        if let Some(segment) = &self.segment {
            if segment.is_empty()
                || !segment
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
            {
                return None;
            }
            parts.push(format!("segment={segment}"));
        }
        if self.from > 0 {
            parts.push(format!("from={}", self.from));
        }
        if let Some(limit) = self.limit {
            parts.push(format!("limit={limit}"));
        }
        Some(if parts.is_empty() {
            String::new()
        } else {
            format!("?{}", parts.join("&"))
        })
    }
}

/// `GET /v3/audit`: a node's audit record, read from a linked desktop's PN-20 by a role
/// holding `audit.read` (GAP-179, D-116; DN-23 §15). **Every read is on the node's record**,
/// one `audit.read` entry each, written before the page is read, so a page of the running
/// segment ends with the read that fetched it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AuditRecordResponse {
    /// The node's mission time when it answered.
    pub at: MissionTime,
    /// What the node's last verification found, or why it has none.
    pub verification: AuditVerificationView,
    /// The page of entries the query asked for, when it named a segment.
    #[serde(default)]
    pub page: Option<AuditPageView>,
}

/// A node's verification of its audit record against the heads its journal holds
/// (GAP-163, D-104), as it is sent.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum AuditVerificationView {
    /// The node has not verified its record in this run. A node verifies at every start,
    /// so this is a node answering before its start finished, never a clean record.
    NotRun,
    /// The verification could not run -- the journal could not be listed, or the audit
    /// directory not read -- and why. **Not a sound record**: nothing was checked.
    Failed { reason: String, at: MissionTime },
    /// What the verification found.
    Ran {
        at: MissionTime,
        /// False for the verification at start, true for one a reader asked for.
        on_request: bool,
        segments: u64,
        entries: u64,
        /// Every chain break and finding as a sentence naming its file; empty when the
        /// record verifies.
        problems: Vec<String>,
        /// Journal sessions that could not be read, so a head or purge in them was not
        /// checked.
        unread_sessions: Vec<String>,
        /// Every segment, earlier runs' included, with its state.
        reports: Vec<AuditSegmentView>,
    },
}

/// One segment as a verification found it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AuditSegmentView {
    /// The file name, which a page is asked for by.
    pub segment: String,
    /// Complete lines in it when it was verified.
    pub entries: u64,
    /// Its state in words, naming the file (`gungnir_security::SegmentReport::describe`).
    pub description: String,
    /// No chain break and no finding.
    pub sound: bool,
    /// On the node's disk, so a page of it can be read.
    pub readable: bool,
}

/// A page of one segment's entries, or why it could not be read.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum AuditPageView {
    Read {
        segment: String,
        /// Entries that read in the whole segment, oldest first; the page is
        /// `from..from + entries.len()` of them.
        total: u64,
        from: u64,
        entries: Vec<AuditEntryView>,
        /// Lines in the segment that are not entries of this format.
        unreadable: u64,
        /// The segment's own chain as it was read for this page; empty when it verifies.
        breaks: Vec<String>,
    },
    Unreadable {
        segment: String,
        reason: String,
    },
}

/// One audit entry as the node's record holds it: who, which machine, what, when.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AuditEntryView {
    /// The verified operator, or none.
    pub operator: Option<u64>,
    /// The verified machine, by its certificate's common name, or none.
    #[serde(default)]
    pub party: Option<String>,
    pub action: String,
    /// Mission time, seconds, on the node's clock.
    pub mission_time: f64,
    pub detail: String,
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

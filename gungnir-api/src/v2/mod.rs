//! v2 external contract. Read paths: a snapshot and an event stream. Write paths:
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

use gungnir_model::{
    CollectionRequirement, DetectionView, ExchangeItem, MissionTime, PlanId, PlanView,
    Releasability, SystemHealth, TrackView, SCHEMA_VERSION,
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
    /// Items removed from this response for the caller's party (DN-17 §5 rule 3,
    /// GAP-062). Zero for an operator inside the deployment. Never silent: a peer that
    /// is told its picture is partial can act on that; one that is not believes it has
    /// the whole picture.
    #[serde(default)]
    pub withheld: usize,
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
            withheld: 0,
        }
    }
}

/// Subscribe to envelopes with `seq >= from_seq` (0 for "everything from now").
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SubscribeRequest {
    pub from_seq: u64,
    /// The session token, which every route but `POST /v2/session` requires (DN-23 §6).
    ///
    /// Carried in the frame rather than an `Authorization` header because a WebSocket
    /// client cannot always set headers on the upgrade. Defaulted so a payload written
    /// against the first v2 shape still decodes -- and is then refused for having no
    /// token, which is a clearer failure than one that will not parse.
    #[serde(default)]
    pub token: String,
}

/// `GET /v2/history?since_seq=N` (GAP-050): the envelopes the node retains from `N`
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

/// The query half of `GET /v2/history`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HistoryQuery {
    pub since_seq: u64,
}

/// Sign in and receive a session token (`POST /v2/session`, DN-23 §6).
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

/// What `POST /v2/session` returns.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionResponse {
    pub token: String,
    /// Mission time at which the token stops being believed. **Always present**: a
    /// node-issued session always expires (DN-23 §5).
    pub expires_s: f64,
}

/// What `GET /v2/coverage` returns (DN-12 §6, GAP-006).
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

/// Who the caller is, for `GET /v2/session`.
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

/// `POST /v2/sensors/{sensor_id}/task` (GAP-004): a command for the node's registry to
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

/// `POST /v2/handoffs/{decision_id}/report` (GAP-040): what the effector says about a
/// handoff. The node puts it on the record as `HandoffEvent::Reported`; the desktop that
/// issued the handoff applies it, and rejects a report on a decision it does not know.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EffectorReportRequest {
    pub report: gungnir_model::handoff::EffectorReport,
}

/// `POST /v2/warnings/{asset_id}/{track_id}/acknowledge` (GAP-042, DN-03 §5 rule 2): the
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

/// What `GET /v2/exchange/{warnings,reports,handoffs}` returns (DN-18 §5, GAP-065).
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
    },
    /// This deployment publishes none of this item, and why. A node holds no warnings, no
    /// reports and no handoffs of its own -- a desktop does -- and saying so is different
    /// from reporting that there are none.
    NotHeld { item: ExchangeItem, reason: String },
}

/// An operator's decision on a plan the node proposed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ApprovalRequest {
    pub plan: PlanId,
    pub accepted: bool,
    pub operator: OperatorId,
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
}

// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The weapons control status in force on the node, as a linked desktop sees it, and the
//! changes this desktop sends it (GAP-203; `docs/design/DN-09-authority-and-control-status.md`
//! §10; D-140).
//!
//! **The node holds the status for its linked desktops**, as it holds the queue (D-55):
//! what a linked desktop decides by is the node's baseline with the node's live changes
//! folded over it, read from the snapshot's `control_status` on each connection and kept
//! live by every `PolicyEvent::ControlStatusSet` on the stream. A change made at a linked
//! console goes to `POST /v3/policy/control-status` and is applied there, never to this
//! desktop's own copy of the baseline: a desktop that changed its own copy while linked
//! would be judging by a status the node did not hold, which is the second, divergent
//! picture this crate exists not to build.
//!
//! **A change is posted until the node answers**, as a decision is: unreachable, a `503`
//! or a `504`, or a lapsed token, keeps it queued under the same request key, and a retry
//! of one the node had recorded is answered `201` with that change and records nothing.
//! A `400`, `403` or `409` is an answer and is delivered.

use gungnir_model::{EffectorLayer, RequestId, WeaponsControlStatus};

/// The wire types a desktop reads and sends here, re-exported because `gungnir-remote` is
/// the only crate the desktop speaks to a node through.
pub use gungnir_api::v3::{
    ControlStatusRecorded, ControlStatusRefused, ControlStatusRequest, WithdrawnItem,
    CONTROL_STATUS_CHANGES_CARRIED,
};

/// A status change on its way to the node.
///
/// **Debug redacts the passphrase**: an outbound change can reach a log line through the
/// panel that shows what is in flight, and the second person's credential must not.
#[derive(Clone, PartialEq, Eq)]
pub struct OutboundControlStatus {
    pub layer: EffectorLayer,
    pub status: WeaponsControlStatus,
    pub reason: String,
    /// The client's key, chosen here and sent on every attempt, so the retries are one
    /// request (DN-31 §5.2's rule).
    pub request: RequestId,
    /// The second person's operator and passphrase, where the deployment's two-person
    /// rule is on (D-140). Verified by the node, never here.
    pub confirmation: Option<(u64, String)>,
    /// Posts that went unanswered, for the panel.
    pub attempts: u32,
}

impl std::fmt::Debug for OutboundControlStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutboundControlStatus")
            .field("layer", &self.layer)
            .field("status", &self.status)
            .field("reason", &self.reason)
            .field("request", &self.request)
            .field(
                "confirmation",
                &self.confirmation.as_ref().map(|(operator, _)| *operator),
            )
            .field("attempts", &self.attempts)
            .finish()
    }
}

impl OutboundControlStatus {
    /// The body the node is sent.
    #[must_use]
    pub fn request_body(&self) -> ControlStatusRequest {
        ControlStatusRequest {
            request: self.request.clone(),
            layer: self.layer,
            status: self.status,
            reason: self.reason.clone(),
            confirmation: self.confirmation.as_ref().map(|(operator, passphrase)| {
                gungnir_api::v3::ConfirmationRequest {
                    operator: *operator,
                    passphrase: passphrase.clone(),
                }
            }),
        }
    }
}

/// What the node said about one change.
#[derive(Debug, Clone, PartialEq)]
pub enum ControlStatusAnswer {
    /// `201`: the node recorded it, or had recorded it under the same key.
    Recorded(ControlStatusRecorded),
    /// `409` or a `403` with a refusal body: the node recorded nothing, and why.
    Refused(ControlStatusRefused),
    /// `400`, a `403` with no refusal body, or a body this desktop could not read:
    /// nothing was recorded.
    Rejected { status: u16, reason: String },
}

impl ControlStatusAnswer {
    /// The answer in words, for an alert.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            ControlStatusAnswer::Recorded(recorded) => {
                let c = &recorded.change;
                let withdrawn = match recorded.withdrawn.len() {
                    0 => String::new(),
                    1 => "; one queued item was withdrawn".to_owned(),
                    n => format!("; {n} queued items were withdrawn"),
                };
                format!(
                    "the node put the {:?} layer at {:?} (was {:?}){withdrawn}",
                    c.layer, c.status, c.previous
                )
            }
            ControlStatusAnswer::Refused(refused) => refused.sentence(),
            ControlStatusAnswer::Rejected { status, reason } => {
                format!("the node would not take the status change ({status}): {reason}")
            }
        }
    }
}

/// One change, answered.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlStatusOutcome {
    pub request: RequestId,
    pub layer: EffectorLayer,
    pub status: WeaponsControlStatus,
    pub answer: ControlStatusAnswer,
}

/// Whether a post's status means the node has not answered yet, so the change goes
/// again under the same key: the decision route's rule (`crate::queue::retry_under_same_key`).
#[must_use]
pub fn retry(status: u16) -> bool {
    crate::queue::retry_under_same_key(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The passphrase never reaches a `Debug` line; the operator may.
    #[test]
    fn debug_redacts_the_confirming_passphrase() {
        let outbound = OutboundControlStatus {
            layer: EffectorLayer::Area,
            status: WeaponsControlStatus::Hold,
            reason: "deconfliction".into(),
            request: RequestId::new("console-a/1").expect("a key"),
            confirmation: Some((9, "correct horse battery staple".into())),
            attempts: 0,
        };
        let line = format!("{outbound:?}");
        assert!(!line.contains("correct horse"), "{line}");
        assert!(line.contains("confirmation: Some(9)"), "{line}");
        let body = format!("{:?}", outbound.request_body());
        assert!(!body.contains("correct horse"), "{body}");
        assert!(body.contains("<redacted>"), "{body}");
    }
}

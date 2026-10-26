// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A live weapons-control-status change from this console (GAP-203; FL-08 in
//! `docs/ux/interaction-design.md`; `docs/design/DN-09-authority-and-control-status.md`
//! §10; D-140).
//!
//! # The owner's choices of 2026-10-26
//!
//! - **A change survives a restart by being folded from the journal.** The baseline file is
//!   never rewritten: the status in force at start is the baseline's value overridden by
//!   the latest journaled change made under that baseline's revision ([`recover`]). A later
//!   revision supersedes every live change made before it, because a baseline that
//!   rewrites the status is a change request under phase H.
//! - **Linked and the link up, the change is sent to the node** and nothing is recorded
//!   here: the node holds the queue (D-55) and the status the queue is judged by, and every
//!   linked console sees the change on the stream. **Cut off, the change is made here**,
//!   because a cut-off desktop runs its own queue; on reconnect the node's status is in
//!   force again ([`sync_from_node`]) and the offline change is reported on PN-18, not
//!   forwarded.
//! - **A tightening withdraws the queued items it refuses**, with the reason; a loosening
//!   resurrects nothing (`gungnir_approval::ApprovalDesk::set_control_status`).
//! - **D-140's two-person rule** is enforced here for control status: when the baseline's
//!   `policy.two_person.control_status` is on, a second signed-in person who also holds
//!   `weapons.control_status` confirms with their own credential, and the record names
//!   both. The override half of the rule is its own gap.

use crate::state::AppState;
use gungnir_model::{
    ConfirmingOperator, ControlStatusChange, EffectorLayer, MissionTime, WeaponsControlStatus,
};
use gungnir_security::actions::SET_CONTROL_STATUS;
use gungnir_security::authz::role_permits;
use gungnir_security::{AuthFailure, OperatorId};
use std::collections::BTreeMap;

/// How many changes PN-17 keeps for the session. The journal is the record; this is a
/// recent-activity list.
const CHANGES_KEPT: usize = 64;

/// What a person asked for on the status dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetRequest {
    pub layer: EffectorLayer,
    pub status: WeaponsControlStatus,
    pub reason: String,
    /// The second person's credential, when D-140's rule is in force.
    pub confirmation: Option<Confirmation>,
}

/// A second person's credential, held for the one frame that verifies it.
#[derive(Clone, PartialEq, Eq)]
pub struct Confirmation {
    pub operator: u64,
    pub passphrase: String,
}

impl std::fmt::Debug for Confirmation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Confirmation")
            .field("operator", &self.operator)
            .field("passphrase", &"<redacted>")
            .finish()
    }
}

/// What became of a request.
#[derive(Debug, Clone, PartialEq)]
pub enum SetOutcome {
    /// Recorded on this desktop's own record, with how many queued items it withdrew.
    Recorded {
        change: ControlStatusChange,
        withdrawn: usize,
    },
    /// Sent to the node; its answer arrives through [`answers_from_node`].
    SentToNode,
}

/// Why a request was refused. Nothing is recorded or sent.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SetError {
    #[error("nobody is signed in; a status change names the person who made it")]
    NotSignedIn,
    #[error(
        "the {role} role does not hold weapons.control_status; the supervisor and the commander do"
    )]
    NotPermitted { role: String },
    #[error("a reason is required")]
    NoReason,
    #[error("the {layer:?} layer is already at {status:?}")]
    Unchanged {
        layer: EffectorLayer,
        status: WeaponsControlStatus,
    },
    #[error("the two-person rule is in force (D-140): a second signed-in person must confirm")]
    TwoPersonRequired,
    #[error("the confirming credential was rejected")]
    ConfirmationRejected,
    #[error("too many confirmation attempts; try again in {retry_after_s:.0} s")]
    ConfirmationTooFast { retry_after_s: f64 },
    #[error("the account store is unavailable, so no second person can be verified")]
    ConfirmationUnavailable,
    #[error("the second person must be someone other than the person asking")]
    SameOperator,
    #[error("the confirming account's {role} role does not hold weapons.control_status")]
    ConfirmationNotPermitted { role: String },
}

/// This desktop's view of the status changes that matter to it.
#[derive(Debug, Default)]
pub struct ControlStatusLedger {
    /// Per layer, the latest change that set the status in force, from the fold at start
    /// or from this session; `None` for a layer the baseline alone sets.
    pub set_by: BTreeMap<EffectorLayer, ControlStatusChange>,
    /// This session's changes, oldest first, bounded by [`CHANGES_KEPT`]: PN-17's list.
    pub this_session: Vec<ControlStatusChange>,
    /// Why the fold at start read less than the whole journal, if it did.
    pub unreadable: Option<String>,
    /// The serial the next request key to the node carries.
    pub next_request: u64,
}

impl ControlStatusLedger {
    /// Fold every session of `journal`, oldest first, applying each change made under
    /// `config.revision` to the settings in force (GAP-203). A change under another
    /// revision was superseded by the baseline that followed it and is skipped.
    ///
    /// A session that cannot be read stops the fold and is said, as the acceptance fold
    /// does: the changes after it would otherwise be lost silently.
    #[must_use]
    pub fn recover(
        journal: &dyn gungnir_store::EventJournal,
        config: &mut gungnir_config::ConfigBaseline,
    ) -> Self {
        use gungnir_eventing::Event;
        use gungnir_model::events::PolicyEvent;
        let mut out = Self::default();
        let mut sessions = match journal.sessions() {
            Ok(s) => s,
            Err(err) => {
                out.unreadable = Some(format!("the session list could not be read: {err}"));
                return out;
            }
        };
        sessions.sort_unstable_by_key(|s| s.0);
        for session in sessions {
            let envelopes = match journal.read_session(session) {
                Ok(envelopes) => envelopes,
                Err(err) => {
                    out.unreadable = Some(format!("session {}: {err}", session.0));
                    break;
                }
            };
            for envelope in envelopes {
                let Event::Policy(PolicyEvent::ControlStatusSet(change)) = envelope.event else {
                    continue;
                };
                if !change.applies_to(config.revision) {
                    continue;
                }
                config
                    .policy
                    .control_status
                    .set(change.layer, change.status);
                out.set_by.insert(change.layer, change);
            }
        }
        if !out.set_by.is_empty() {
            tracing::info!(
                layers = ?out.set_by.keys().collect::<Vec<_>>(),
                "weapons control status folded from the journal over the baseline"
            );
        }
        out
    }

    fn note(&mut self, change: ControlStatusChange) {
        self.set_by.insert(change.layer, change.clone());
        if self.this_session.len() >= CHANGES_KEPT {
            self.this_session.remove(0);
        }
        self.this_session.push(change);
    }
}

/// Put a layer at a status, for a reason (FL-08).
///
/// Linked and the link up, the request goes to the node and nothing is recorded here.
/// Otherwise the change is applied to the settings in force, journaled, audited, and the
/// queue re-judged, all through `gungnir_approval`'s one operation.
///
/// # Errors
///
/// [`SetError`]; nothing is recorded or sent.
pub fn set(state: &mut AppState, request: SetRequest) -> Result<SetOutcome, SetError> {
    let Some(session) = state.signed_in() else {
        return Err(SetError::NotSignedIn);
    };
    if !role_permits(session.role, SET_CONTROL_STATUS) {
        return Err(SetError::NotPermitted {
            role: format!("{:?}", session.role),
        });
    }
    let reason = request.reason.trim().to_owned();
    if reason.is_empty() {
        return Err(SetError::NoReason);
    }
    if state
        .link
        .as_ref()
        .is_some_and(gungnir_remote::link::NodeLink::connected)
    {
        // The idempotency key names this console, the person, the layer and a serial
        // this console keeps, so a retry after a `504` is the same request and two
        // consoles can never collide (DN-31 §5.2).
        state.control_status_ledger.next_request += 1;
        let key = gungnir_model::RequestId::new(format!(
            "{}/{}/{:?}/{}",
            crate::session::origin_of(state),
            session.operator.0,
            request.layer,
            state.control_status_ledger.next_request
        ))
        .map_err(|_| SetError::NoReason)?;
        let Some(link) = state.link.as_ref() else {
            return Err(SetError::NoReason);
        };
        link.set_control_status(gungnir_remote::policy::OutboundControlStatus {
            layer: request.layer,
            status: request.status,
            reason,
            request: key,
            confirmation: request.confirmation.map(|c| (c.operator, c.passphrase)),
            attempts: 0,
        });
        return Ok(SetOutcome::SentToNode);
    }
    let current = state.config.policy.control_status.for_layer(request.layer);
    if current == request.status {
        return Err(SetError::Unchanged {
            layer: request.layer,
            status: request.status,
        });
    }
    let confirmed_by = if state.config.policy.two_person.control_status {
        Some(confirm(
            state,
            session.operator,
            request.confirmation.as_ref(),
        )?)
    } else {
        None
    };
    let now = state.clock.now();
    let previous = state
        .config
        .policy
        .control_status
        .set(request.layer, request.status);
    let change = ControlStatusChange {
        layer: request.layer,
        status: request.status,
        previous,
        reason,
        operator: session.operator.0.to_string(),
        role: format!("{:?}", session.role),
        confirmed_by,
        revision: state.config.revision,
        origin: None,
        at: now,
        request: None,
    };
    let withdrawn = crate::desk::with_policy(state, |desk, cx, _policy, host| {
        desk.set_control_status(cx, host, change.clone())
    });
    state.control_status_ledger.note(change.clone());
    Ok(SetOutcome::Recorded {
        change,
        withdrawn: withdrawn.len(),
    })
}

/// D-140: verify the second person against the same store the sign-in uses, and that
/// they are somebody else who holds the action.
fn confirm(
    state: &mut AppState,
    asking: OperatorId,
    confirmation: Option<&Confirmation>,
) -> Result<ConfirmingOperator, SetError> {
    let Some(confirmation) = confirmation else {
        return Err(SetError::TwoPersonRequired);
    };
    let operator = OperatorId(confirmation.operator);
    if operator == asking {
        return Err(SetError::SameOperator);
    }
    let now = state.clock.now().0;
    let role = state
        .session
        .verify_credential(operator, &confirmation.passphrase, now)
        .map_err(|failure| match failure {
            AuthFailure::Rejected => SetError::ConfirmationRejected,
            AuthFailure::TooFast { retry_after_s } => {
                SetError::ConfirmationTooFast { retry_after_s }
            }
            AuthFailure::Unavailable => SetError::ConfirmationUnavailable,
        })?;
    if !role_permits(role, SET_CONTROL_STATUS) {
        return Err(SetError::ConfirmationNotPermitted {
            role: format!("{role:?}"),
        });
    }
    Ok(ConfirmingOperator {
        operator: operator.0.to_string(),
        role: format!("{role:?}"),
    })
}

/// How many queued items putting `layer` at `status` would withdraw, judged the way the
/// desk would judge them (the control-status engine alone), for the dialog's sentence.
///
/// Over this console's own queue when it runs one, and over the node's queue as this
/// desktop last saw it when linked: the node is the one that would withdraw, and this is
/// the best this console can say beforehand.
#[must_use]
pub fn would_withdraw(
    state: &AppState,
    layer: EffectorLayer,
    status: WeaponsControlStatus,
) -> usize {
    use gungnir_command::ApprovalWorkflow;
    use gungnir_policy::{ControlStatusPolicy, DenialReason, PolicyEngine, PolicyVerdict};
    let mut settings = state.config.policy.control_status.clone();
    settings.set(layer, status);
    let tracks = state.tracking.tracks();
    let classification = |id: gungnir_model::TrackId| {
        tracks
            .iter()
            .find(|t| t.id == id)
            .map_or(gungnir_model::Classification::Unknown, |t| t.classification)
    };
    let engine = ControlStatusPolicy {
        settings: &settings,
        track_classification: &classification,
    };
    let refused = |plan: &gungnir_model::PlanView| {
        matches!(
            engine.evaluate(plan, &state.resources),
            PolicyVerdict::Denied {
                reason_code: DenialReason::ControlStatus { .. }
            }
        )
    };
    if state
        .link
        .as_ref()
        .is_some_and(gungnir_remote::link::NodeLink::connected)
    {
        state
            .projection
            .queue
            .waiting
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .filter(|item| refused(&item.plan))
            .count()
    } else {
        state
            .desk
            .approvals
            .pending()
            .into_iter()
            .filter(|(_, plan)| refused(plan))
            .count()
    }
}

/// Keep the settings in force equal to the node's while the link is up (GAP-203, the
/// owner's rule that the node's status wins). Called every frame from `update::tick`;
/// costs a map comparison while nothing changed.
///
/// Applied without an event: the node's own record carries the change and this desktop's
/// journal records what this desktop did, which this is not. The ledger takes the node's
/// latest change per layer so the strip can say who set it.
pub fn sync_from_node(state: &mut AppState) {
    let Some(link) = state.link.as_ref().filter(|l| l.connected()) else {
        return;
    };
    let Some(in_force) = link.control_status() else {
        return;
    };
    let changes = link.control_status_changes();
    for (layer, status) in in_force {
        if state.config.policy.control_status.for_layer(layer) != status {
            state.config.policy.control_status.set(layer, status);
        }
    }
    for change in changes {
        let newer = state
            .control_status_ledger
            .set_by
            .get(&change.layer)
            .is_none_or(|held| held.at < change.at);
        if newer {
            state
                .control_status_ledger
                .set_by
                .insert(change.layer, change.clone());
        }
        if !state
            .control_status_ledger
            .this_session
            .iter()
            .any(|c| c.layer == change.layer && c.at == change.at && c.operator == change.operator)
        {
            if state.control_status_ledger.this_session.len() >= CHANGES_KEPT {
                state.control_status_ledger.this_session.remove(0);
            }
            state.control_status_ledger.this_session.push(change);
        }
    }
}

/// The node's answers to the changes this desktop sent (GAP-203), each said as an alert.
/// Called every frame from `update::tick`; costs nothing while nothing is owed.
pub fn answers_from_node(state: &mut AppState) {
    use gungnir_remote::policy::ControlStatusAnswer;
    let Some(link) = state.link.as_ref() else {
        return;
    };
    for outcome in link.take_control_status_outcomes() {
        let sentence = match outcome.answer {
            ControlStatusAnswer::Recorded(recorded) => format!(
                "Control status of the {:?} layer set to {:?} on the node; {} queued item(s) \
                 withdrawn",
                recorded.change.layer,
                recorded.change.status,
                recorded.withdrawn.len()
            ),
            ControlStatusAnswer::Refused(refused) => format!(
                "The node would not change the {:?} layer to {:?}: {}",
                outcome.layer,
                outcome.status,
                refused.sentence()
            ),
            ControlStatusAnswer::Rejected { status, reason } => format!(
                "The node would not take the change to the {:?} layer ({status}): {reason}. \
                 Nothing was recorded, here or there.",
                outcome.layer
            ),
        };
        state.alerts.push(sentence);
    }
}

/// The time a change was made, for a sentence.
#[must_use]
pub fn at_sentence(at: MissionTime) -> String {
    format!("T+{:.0} s", at.0)
}

/// Whether the person at this console may change a status now, and why not when not:
/// the strip's chip and the dialog's controls read the same answer.
///
/// Cut off from a node is not a reason: a cut-off desktop runs its own queue and changes
/// its own status (the owner's choice of 2026-10-26); the dialog's route sentence says
/// where the change goes.
///
/// # Errors
///
/// The sentence the dialog draws.
pub fn may_set(state: &AppState) -> Result<(), &'static str> {
    let Some(session) = state.signed_in() else {
        return Err("Sign in to change a status: a change names the person who made it.");
    };
    if !role_permits(session.role, SET_CONTROL_STATUS) {
        return Err(
            "Your role does not hold weapons.control_status; the supervisor and the \
             commander do.",
        );
    }
    Ok(())
}

/// The dialog's view of one layer, built once per frame (FL-08).
///
/// `would` is what each other status would withdraw, computed by the caller with
/// [`would_withdraw`] so the frame owns the slice.
#[must_use]
pub fn dialog_view<'a>(
    state: &'a AppState,
    layer: EffectorLayer,
    would: &'a [(WeaponsControlStatus, usize)],
) -> gungnir_ui::panels::control_status_dialog::ControlStatusDialogView<'a> {
    use gungnir_ui::panels::control_status_dialog::{LastChange, Route};
    let current = state.config.policy.control_status.for_layer(layer);
    let last_change = state
        .control_status_ledger
        .set_by
        .get(&layer)
        .map(|c| LastChange {
            operator: c.operator.as_str(),
            role: c.role.as_str(),
            at: c.at,
            reason: c.reason.as_str(),
            confirmed_by: c.confirmed_by.as_ref().map(|p| p.operator.as_str()),
        });
    let route = match &state.backend {
        gungnir_config::BackendConfig::Remote { endpoint }
            if state
                .link
                .as_ref()
                .is_some_and(gungnir_remote::link::NodeLink::connected) =>
        {
            Route::Node { endpoint }
        }
        _ => Route::ThisDesktop,
    };
    gungnir_ui::panels::control_status_dialog::ControlStatusDialogView {
        layer,
        current,
        configured: state
            .config
            .policy
            .control_status
            .by_layer
            .contains_key(&layer),
        last_change,
        would_withdraw: would,
        queued_on_layer: queued_on_layer(state, layer),
        two_person: state.config.policy.two_person.control_status,
        may_set: may_set(state),
        route,
        vocabulary: &state.config.vocabulary,
    }
}

/// What each status other than the one in force would withdraw, for [`dialog_view`].
#[must_use]
pub fn would_withdraw_each(
    state: &AppState,
    layer: EffectorLayer,
) -> Vec<(WeaponsControlStatus, usize)> {
    let current = state.config.policy.control_status.for_layer(layer);
    [
        WeaponsControlStatus::Hold,
        WeaponsControlStatus::Tight,
        WeaponsControlStatus::Free,
    ]
    .into_iter()
    .filter(|s| *s != current)
    .map(|s| (s, would_withdraw(state, layer, s)))
    .collect()
}

/// How many queued items engage a resource on `layer`.
fn queued_on_layer(state: &AppState, layer: EffectorLayer) -> usize {
    use gungnir_command::ApprovalWorkflow;
    let on_layer = |plan: &gungnir_model::PlanView| {
        plan.assignments().iter().any(|(resource, _)| {
            state
                .resources
                .iter()
                .any(|r| r.id == *resource && r.layer == layer)
        })
    };
    if state
        .link
        .as_ref()
        .is_some_and(gungnir_remote::link::NodeLink::connected)
    {
        state
            .projection
            .queue
            .waiting
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .filter(|item| on_layer(&item.plan))
            .count()
    } else {
        state
            .desk
            .approvals
            .pending()
            .into_iter()
            .filter(|(_, plan)| on_layer(plan))
            .count()
    }
}

/// Turn the dialog's draft into a request.
#[must_use]
pub fn request_from(
    draft: gungnir_ui::panels::control_status_dialog::ControlStatusDraft,
) -> SetRequest {
    SetRequest {
        layer: draft.layer,
        status: draft.status,
        reason: draft.reason,
        confirmation: draft.confirmation.map(|c| Confirmation {
            operator: c.operator,
            passphrase: c.passphrase,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_confirmation_debug_redacts_the_passphrase() {
        let c = Confirmation {
            operator: 9,
            passphrase: "correct horse battery staple".into(),
        };
        let text = format!("{c:?}");
        assert!(text.contains("operator: 9"));
        assert!(!text.contains("horse"), "{text}");
    }
}

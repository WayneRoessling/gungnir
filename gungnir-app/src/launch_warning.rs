// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Declaring a launch warning (GAP-009, DN-16 amendment 2): the outbound producer
//! `gungnir_model::events::LaunchWarningEvent::Issued`'s own doc comment names as
//! unbuilt -- "nothing in this workspace issues one... a producer was not invented to
//! make the path look built."
//!
//! **No automation exists to detect "something launched"**: there is no sensor, no
//! algorithm, nothing in the ingest path that could raise one on its own. This is
//! therefore a manual operator action, the same shape `requirements.rs::
//! state_requirement` already is for a different fact nothing else observes: a person
//! asserting something the system did not detect on its own. It is not automated
//! *because* nothing was invented to look automated where nothing is.
//!
//! **Gated on `RELEASE_PRODUCT`, not a new action.** A launch warning is an
//! intelligence product meant for a peer, exactly the case GAP-065 already reasoned
//! about when it granted `PUBLISH_EXCHANGE` alongside `RELEASE_PRODUCT`: whoever may
//! mark a product releasable is who may send one. Reusing that grant rather than
//! minting `DECLARE_LAUNCH_WARNING` keeps one action for one kind of decision instead
//! of two names for the same authority.
//!
//! **Kept apart from DN-03's warnings, the same rule DN-16 §9 already states for the
//! two report types.** `gungnir_workflow::warning::Warning` is an obligation this
//! deployment owes a defended asset, raised automatically by `WarningLedger::evaluate`
//! and never manually; nothing here touches it. This module's `LaunchWarningReport` is
//! a claim about the world outside this deployment, and only ever locally issued
//! here -- DN-16 §9's rule against forwarding one peer's warning to another does not
//! apply, because a warning this function makes never came from a peer at all.

use gungnir_eventing::Event;
use gungnir_model::events::LaunchWarningEvent;
use gungnir_model::{ExchangeItem, LaunchWarningReport, MissionTime, Releasability};

use crate::state::AppState;

/// Why a launch warning could not be declared.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LaunchWarningError {
    #[error("operator lacks permission to release a launch warning")]
    Forbidden,
    #[error("a launch warning needs a non-empty description of what launched")]
    EmptyDescription,
}

/// Declare a launch warning: `what` in the operator's own words, `releasability`
/// naming who may be told. `id` and `at` are never taken from the caller -- `id` is
/// this deployment's own serial, continued past whatever the journal recovered, and
/// `at` is the clock at the moment of declaration, not a time the operator could set
/// to something that already passed.
///
/// # Errors
///
/// `Forbidden` when the signed-in role may not release a product
/// (`gungnir_security::actions::RELEASE_PRODUCT`). `EmptyDescription` for a blank or
/// whitespace-only `what`.
pub fn declare(
    state: &mut AppState,
    what: &str,
    releasability: Releasability,
) -> Result<LaunchWarningReport, LaunchWarningError> {
    if !gungnir_security::authz::role_permits(
        state.role(),
        gungnir_security::actions::RELEASE_PRODUCT,
    ) {
        return Err(LaunchWarningError::Forbidden);
    }
    let what = what.trim();
    if what.is_empty() {
        return Err(LaunchWarningError::EmptyDescription);
    }
    let what = what.to_string();
    let now = state.clock.now();
    let report = LaunchWarningReport {
        id: state.next_launch_warning_id(),
        what,
        at: now,
        releasability,
    };
    state.issued_launch_warnings.push(report.clone());
    publish(state, now, LaunchWarningEvent::Issued(report.clone()));
    crate::audit::record(
        state,
        gungnir_security::actions::RELEASE_PRODUCT,
        format!("launch warning {} declared: {}", report.id, report.what),
    );
    publish_to_exchange(state);
    Ok(report)
}

fn publish(state: &mut AppState, now: MissionTime, event: LaunchWarningEvent) {
    if let Err(err) = state.events.publish(now, Event::LaunchWarning(event)) {
        tracing::error!(%err, "launch warning event publish failed");
    }
}

/// Every issued launch warning, republished on each new one -- the same shape
/// `handoffs.rs::issue_for`/`publish_to_exchange` already use for `Handoffs`:
/// unfiltered by marking, since `NodeApi::exchange_for` applies that gate per party at
/// serve time.
fn publish_to_exchange(state: &AppState) {
    let Some(link) = state.link.clone() else {
        return;
    };
    let products = state
        .issued_launch_warnings
        .iter()
        .map(|report| gungnir_remote::link::ExchangeProductRecord {
            id: report.id.clone(),
            at: report.at,
            releasability: report.releasability.clone(),
            body: serde_json::to_value(report).unwrap_or(serde_json::Value::Null),
        })
        .collect();
    link.queue_exchange(ExchangeItem::Warnings, products);
}

/// What recovering issued launch warnings from the journal found. Same shape as
/// `requirements::Recovered`, for the same reason: a journal that cannot be read must
/// say so rather than presenting an empty list as though nothing had been issued.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recovered {
    NothingIssued,
    FromJournal { sessions: usize },
    Unreadable { reason: String },
}

/// Rebuild the issued list from every session in the journal, oldest first.
///
/// **Simpler than `requirements::recover`**: a launch warning has no lifecycle to fold
/// -- once issued it is never withdrawn, amended, or answered -- so recovery is a
/// straight append of every `Issued` event in order, never an update by id.
#[must_use]
pub fn recover(journal: &dyn gungnir_store::EventJournal) -> (Vec<LaunchWarningReport>, Recovered) {
    let sessions = match journal.sessions() {
        Ok(sessions) => sessions,
        Err(err) => {
            return (
                Vec::new(),
                Recovered::Unreadable {
                    reason: err.to_string(),
                },
            )
        }
    };
    let mut ordered = sessions;
    ordered.sort_unstable_by_key(|s| s.0);

    let mut issued = Vec::new();
    let mut read = 0usize;
    for session in ordered {
        let envelopes = match journal.read_session(session) {
            Ok(envelopes) => envelopes,
            Err(err) => {
                return (
                    issued,
                    Recovered::Unreadable {
                        reason: format!("session {}: {err}", session.0),
                    },
                )
            }
        };
        read += 1;
        for envelope in envelopes {
            if let Event::LaunchWarning(LaunchWarningEvent::Issued(report)) = envelope.event {
                issued.push(report);
            }
        }
    }
    let outcome = if issued.is_empty() {
        Recovered::NothingIssued
    } else {
        Recovered::FromJournal { sessions: read }
    };
    (issued, outcome)
}

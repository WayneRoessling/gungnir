// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Deliveries in flight to configured endpoints (GAP-040, DN-07 §5; GAP-042, DN-03 §5):
//! handoffs and warnings posted through `gungnir-remote`'s endpoint client, their
//! outcomes swept every frame onto the records and the journal.
//!
//! **The handoff half is `gungnir_approval::deliveries`'** (GAP-131, D-57,
//! `docs/design/DN-31-node-approval-queue.md` §3 point 5): the retry schedule and the rule
//! that a handoff is never dropped went with the record they protect, over a
//! `HandoffTransport` this binary implements with the endpoint client (`desk.rs`). What is
//! left here is the warnings half, which no node runs, and the endpoint table lookup both
//! halves share.
//!
//! **Three outcomes, never collapsed.** Accepted becomes `Delivered` (a handoff) or
//! leaves `Sent` standing (a warning). Refused is the endpoint's decision: recorded with
//! the body, alerted, never retried. Unreachable is store-and-forward's case: the record
//! says undelivered, the alert says so, and the post is made again after
//! [`RETRY_AFTER_S`] for as long as the endpoint stays silent. A handoff is never dropped
//! (DN-07 §5 case 3); a warning that cannot be carried is `Failed` loudly (DN-03 §5).

use gungnir_eventing::Event;
use gungnir_model::events::WarningEvent;
use gungnir_model::{AssetId, MissionTime, TrackId};
use gungnir_remote::endpoint::{DeliveryOutcome, PendingDelivery};

use crate::state::AppState;
use crate::update::publish;

/// The handoff schedule, as the desk applies it.
pub use gungnir_approval::{PendingHandoff, RETRY_AFTER_S};

/// The endpoint kind the transport carries. Anything else has no transport and the
/// record says so. Re-exported, not declared: it moved with the rule that reads it
/// (GAP-132).
pub use gungnir_approval::HTTP_KIND;

/// A warning posted and not yet answered.
#[derive(Debug)]
pub struct PendingWarning {
    pub asset: AssetId,
    pub track: TrackId,
    pub endpoint: String,
    pub in_flight: PendingDelivery,
}

/// The configured endpoint's address, when it is one the transport carries.
///
/// # Errors
///
/// Why the endpoint cannot be posted to, for the record.
pub fn http_address(state: &AppState, endpoint: &str) -> Result<String, String> {
    http_address_in(
        &state.config.endpoints,
        state.endpoint_client.is_some(),
        endpoint,
    )
}

/// The same over the parts, for a caller that has taken the ledger out of the state.
///
/// Both halves ask this: the warnings below, and the approval desk through its host's
/// `HandoffTransport::address_for` (`desk.rs`). Which endpoint kinds a transport carries is
/// a fact about the transport, so it is answered once, here, beside the client.
///
/// # Errors
///
/// As [`http_address`].
pub fn http_address_in(
    endpoints: &[gungnir_config::EndpointConfig],
    client_available: bool,
    endpoint: &str,
) -> Result<String, String> {
    // The rule itself moved to `gungnir-approval` in GAP-132, when the node began handing
    // off too: which endpoint kinds a transport carries is one fact about the transport,
    // and two binaries answering it separately is how they come to refuse different
    // endpoints for the same baseline.
    gungnir_approval::http_address_in(endpoints, client_available, endpoint)
}

/// The tick step.
pub fn sweep(state: &mut AppState) {
    let now = state.clock.now();
    crate::desk::with_desk(state, |desk, cx, host| desk.sweep_handoffs(host, cx.now));
    sweep_warnings(state, now);
}

fn sweep_warnings(state: &mut AppState, now: MissionTime) {
    let mut pending = std::mem::take(&mut state.pending_warnings);
    let mut keep = Vec::new();
    for p in pending.drain(..) {
        match p.in_flight.poll() {
            None => keep.push(p),
            Some(DeliveryOutcome::Accepted { .. }) => {
                // `Sent` stands until the party acknowledges (DN-03 §5 rule 2).
            }
            Some(DeliveryOutcome::Refused { status, body }) => {
                fail_warning(
                    state,
                    &p,
                    &format!("{} refused it: {status} {body}", p.endpoint),
                    now,
                );
            }
            Some(DeliveryOutcome::Unreachable { reason }) => {
                fail_warning(
                    state,
                    &p,
                    &format!("{} unreachable: {reason}", p.endpoint),
                    now,
                );
            }
        }
    }
    state.pending_warnings = keep;
}

fn fail_warning(state: &mut AppState, p: &PendingWarning, reason: &str, now: MissionTime) {
    if state
        .warnings
        .fail(p.asset, p.track, reason.to_string(), now)
    {
        publish(
            state,
            now,
            Event::Warning(WarningEvent::Failed {
                asset: p.asset,
                track: p.track,
                reason: reason.to_string(),
                at: now,
            }),
        );
        state.alerts.push(format!(
            "warning because of track {} was not delivered: {reason}; warn by voice",
            p.track.0
        ));
    }
}

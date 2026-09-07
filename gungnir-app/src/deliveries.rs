//! Deliveries in flight to configured endpoints (GAP-040, DN-07 §5; GAP-042, DN-03 §5):
//! handoffs and warnings posted through `gungnir-remote`'s endpoint client, their
//! outcomes swept every frame onto the records and the journal.
//!
//! **Three outcomes, never collapsed.** Accepted becomes `Delivered` (a handoff) or
//! leaves `Sent` standing (a warning). Refused is the endpoint's decision: recorded with
//! the body, alerted, never retried. Unreachable is store-and-forward's case: the record
//! says undelivered, the alert says so, and the post is made again after
//! [`RETRY_AFTER_S`] for as long as the endpoint stays silent. A handoff is never dropped
//! (DN-07 §5 case 3); a warning that cannot be carried is `Failed` loudly (DN-03 §5).

use gungnir_eventing::Event;
use gungnir_model::events::{HandoffEvent, WarningEvent};
use gungnir_model::handoff::DeliveryState;
use gungnir_model::{AssetId, DecisionId, MissionTime, TrackId};
use gungnir_remote::endpoint::{DeliveryOutcome, PendingDelivery};

use crate::state::AppState;
use crate::update::publish;

/// Mission seconds between attempts on an unreachable endpoint.
pub const RETRY_AFTER_S: f64 = 30.0;

/// The endpoint kind the transport carries. Anything else has no transport and the
/// record says so.
pub const HTTP_KIND: &str = "http";

/// A handoff posted and not yet answered, or waiting to be posted again.
#[derive(Debug)]
pub struct PendingHandoff {
    pub decision: DecisionId,
    pub endpoint: String,
    pub address: String,
    pub payload: serde_json::Value,
    pub attempts: u32,
    pub in_flight: Option<PendingDelivery>,
    pub next_attempt: MissionTime,
}

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
/// # Errors
///
/// As [`http_address`].
pub fn http_address_in(
    endpoints: &[gungnir_config::EndpointConfig],
    client_available: bool,
    endpoint: &str,
) -> Result<String, String> {
    let Some(e) = endpoints.iter().find(|e| e.name == endpoint) else {
        return Err(format!(
            "endpoint {endpoint:?} is not in the endpoint table"
        ));
    };
    if e.kind != HTTP_KIND {
        return Err(format!(
            "endpoint {endpoint:?} is kind {:?}, which no transport carries",
            e.kind
        ));
    }
    if !client_available {
        return Err("the endpoint client could not be built at start (see the alerts)".into());
    }
    Ok(e.address.clone())
}

/// Post a handoff for the first time. Called from the handoff record path.
pub fn post_handoff(
    state: &mut AppState,
    decision: DecisionId,
    endpoint: String,
    address: String,
    payload: serde_json::Value,
    now: MissionTime,
) {
    let in_flight = state
        .endpoint_client
        .as_ref()
        .map(|c| c.post_json(&address, payload.clone()));
    state.pending_handoffs.push(PendingHandoff {
        decision,
        endpoint,
        address,
        payload,
        attempts: 1,
        in_flight,
        next_attempt: now,
    });
}

/// The tick step.
pub fn sweep(state: &mut AppState) {
    let now = state.clock.now();
    sweep_handoffs(state, now);
    sweep_warnings(state, now);
}

fn sweep_handoffs(state: &mut AppState, now: MissionTime) {
    let mut pending = std::mem::take(&mut state.pending_handoffs);
    let mut keep = Vec::new();
    for mut p in pending.drain(..) {
        let outcome = match &p.in_flight {
            Some(f) => f.poll(),
            None if now.0 >= p.next_attempt.0 => {
                // Store-and-forward: post again.
                p.attempts += 1;
                p.in_flight = state
                    .endpoint_client
                    .as_ref()
                    .map(|c| c.post_json(&p.address, p.payload.clone()));
                None
            }
            None => None,
        };
        match outcome {
            None => keep.push(p),
            Some(DeliveryOutcome::Accepted { .. }) => {
                set_delivery(state, p.decision, DeliveryState::Delivered { at: now });
                publish(
                    state,
                    now,
                    Event::Handoff(HandoffEvent::Delivered {
                        decision: p.decision,
                        endpoint: p.endpoint.clone(),
                        attempts: p.attempts,
                        at: now,
                    }),
                );
                state.alerts.push(format!(
                    "decision {}: handoff delivered to {} (attempt {})",
                    p.decision.0, p.endpoint, p.attempts
                ));
            }
            Some(DeliveryOutcome::Refused { status, body }) => {
                let reason = format!("{status}: {body}");
                set_delivery(
                    state,
                    p.decision,
                    DeliveryState::Refused {
                        reason: reason.clone(),
                        at: now,
                    },
                );
                publish(
                    state,
                    now,
                    Event::Handoff(HandoffEvent::Refused {
                        decision: p.decision,
                        endpoint: p.endpoint.clone(),
                        reason: reason.clone(),
                        at: now,
                    }),
                );
                state.alerts.push(format!(
                    "decision {}: handoff to {} refused: {reason}; the decision stands and \
                     the effector must be reached another way",
                    p.decision.0, p.endpoint
                ));
            }
            Some(DeliveryOutcome::Unreachable { reason }) => {
                publish(
                    state,
                    now,
                    Event::Handoff(HandoffEvent::Undelivered {
                        decision: p.decision,
                        endpoint: p.endpoint.clone(),
                        reason: reason.clone(),
                        at: now,
                    }),
                );
                state.alerts.push(format!(
                    "decision {}: handoff to {} undelivered (attempt {}): {reason}; retrying in {RETRY_AFTER_S:.0} s",
                    p.decision.0, p.endpoint, p.attempts
                ));
                p.in_flight = None;
                p.next_attempt = MissionTime(now.0 + RETRY_AFTER_S);
                keep.push(p);
            }
        }
    }
    state.pending_handoffs = keep;
}

fn set_delivery(state: &mut AppState, decision: DecisionId, delivery: DeliveryState) {
    if let Some(record) = state
        .handoffs
        .iter_mut()
        .find(|h| h.handoff.decision == decision)
    {
        record.delivery = delivery;
    }
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

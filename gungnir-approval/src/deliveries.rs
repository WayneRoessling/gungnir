// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Handoffs in flight to a configured endpoint (GAP-040, DN-07 §5), and the rule that one
//! is never dropped (`docs/design/DN-31-node-approval-queue.md` §3 point 5; GAP-131, D-57).
//!
//! **Three outcomes, never collapsed.** Accepted becomes `Delivered`. Refused is the
//! endpoint's decision: recorded with the body, alerted, never retried. Unreachable is
//! store-and-forward's case: the record says undelivered, the alert says so, and the post
//! is made again after [`RETRY_AFTER_S`] for as long as the endpoint stays silent.
//!
//! **The transport is the host's** ([`HandoffTransport`]). D-57 refused an edge from this
//! crate to `gungnir-remote`, so what a host supplies is an address for a configured
//! endpoint and something that carries bytes to it; the schedule, the attempt count and
//! the record they land on stay here, with the rule they protect.

use crate::{ApprovalDesk, ApprovalHost};
use gungnir_eventing::Event;
use gungnir_model::events::HandoffEvent;
use gungnir_model::handoff::DeliveryState;
use gungnir_model::{DecisionId, MissionTime};

/// Mission seconds between attempts on an unreachable endpoint.
pub const RETRY_AFTER_S: f64 = 30.0;

/// What the endpoint did with the message, as the host reports it.
///
/// The same three states `gungnir-remote`'s endpoint client distinguishes, restated here
/// so the desk does not name a transport type: a host with another transport answers in
/// the same three, and a host that collapsed them would be deciding the retry rule rather
/// than reporting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryAnswer {
    Accepted { status: u16 },
    Refused { status: u16, body: String },
    Unreachable { reason: String },
}

/// One post the host has made and not yet had an answer to. Polled, never blocking.
pub trait HandoffInFlight: std::fmt::Debug + Send {
    /// The answer, once there is one.
    fn poll(&self) -> Option<DeliveryAnswer>;
}

/// How a host carries a handoff to an effector (DN-31 §3 point 5).
///
/// Two questions, both of which only the host can answer: whether a configured endpoint is
/// one its transport carries, and -- if so -- posting to it. Neither decides what happens
/// next; that is [`ApprovalDesk::sweep_handoffs`].
pub trait HandoffTransport {
    /// The address for a configured endpoint, or why a handoff cannot be posted to it.
    ///
    /// # Errors
    ///
    /// The reason, for the record and for the operator: an endpoint that is not in the
    /// table, one of a kind no transport carries, or a transport that could not be built.
    fn address_for(&self, endpoint: &str) -> Result<String, String>;

    /// Post the payload. `None` when the host has no transport at all, which leaves the
    /// handoff owed and due again rather than lost.
    fn post(&self, address: &str, payload: serde_json::Value) -> Option<Box<dyn HandoffInFlight>>;
}

/// A handoff posted and not yet answered, or waiting to be posted again.
#[derive(Debug)]
pub struct PendingHandoff {
    pub decision: DecisionId,
    pub endpoint: String,
    pub address: String,
    pub payload: serde_json::Value,
    pub attempts: u32,
    pub in_flight: Option<Box<dyn HandoffInFlight>>,
    pub next_attempt: MissionTime,
}

impl ApprovalDesk {
    /// Post a handoff for the first time. Called from the handoff record path.
    pub fn post_handoff(
        &mut self,
        host: &dyn ApprovalHost,
        decision: DecisionId,
        endpoint: String,
        address: String,
        payload: serde_json::Value,
        now: MissionTime,
    ) {
        let in_flight = host.post(&address, payload.clone());
        self.pending_handoffs.push(PendingHandoff {
            decision,
            endpoint,
            address,
            payload,
            attempts: 1,
            in_flight,
            next_attempt: now,
        });
    }

    /// What the endpoints answered: delivered, refused, or unreachable and due again.
    pub fn sweep_handoffs(&mut self, host: &mut dyn ApprovalHost, now: MissionTime) {
        let mut pending = std::mem::take(&mut self.pending_handoffs);
        let mut keep = Vec::new();
        for mut p in pending.drain(..) {
            let outcome = match &p.in_flight {
                Some(f) => f.poll(),
                None if now.0 >= p.next_attempt.0 => {
                    // Store-and-forward: post again.
                    p.attempts += 1;
                    p.in_flight = host.post(&p.address, p.payload.clone());
                    None
                }
                None => None,
            };
            match outcome {
                None => keep.push(p),
                Some(DeliveryAnswer::Accepted { .. }) => {
                    self.set_delivery(p.decision, DeliveryState::Delivered { at: now });
                    host.publish(
                        now,
                        Event::Handoff(HandoffEvent::Delivered {
                            decision: p.decision,
                            endpoint: p.endpoint.clone(),
                            attempts: p.attempts,
                            at: now,
                        }),
                    );
                    host.alert(format!(
                        "decision {}: handoff delivered to {} (attempt {})",
                        p.decision.short(),
                        p.endpoint,
                        p.attempts
                    ));
                }
                Some(DeliveryAnswer::Refused { status, body }) => {
                    let reason = format!("{status}: {body}");
                    self.set_delivery(
                        p.decision,
                        DeliveryState::Refused {
                            reason: reason.clone(),
                            at: now,
                        },
                    );
                    host.publish(
                        now,
                        Event::Handoff(HandoffEvent::Refused {
                            decision: p.decision,
                            endpoint: p.endpoint.clone(),
                            reason: reason.clone(),
                            at: now,
                        }),
                    );
                    host.alert(format!(
                        "decision {}: handoff to {} refused: {reason}; the decision stands and \
                         the effector must be reached another way",
                        p.decision.short(),
                        p.endpoint
                    ));
                }
                Some(DeliveryAnswer::Unreachable { reason }) => {
                    host.publish(
                        now,
                        Event::Handoff(HandoffEvent::Undelivered {
                            decision: p.decision,
                            endpoint: p.endpoint.clone(),
                            reason: reason.clone(),
                            at: now,
                        }),
                    );
                    host.alert(format!(
                        "decision {}: handoff to {} undelivered (attempt {}): {reason}; retrying in {RETRY_AFTER_S:.0} s",
                        p.decision.short(),
                        p.endpoint,
                        p.attempts
                    ));
                    p.in_flight = None;
                    p.next_attempt = MissionTime(now.0 + RETRY_AFTER_S);
                    keep.push(p);
                }
            }
        }
        self.pending_handoffs = keep;
    }

    fn set_delivery(&mut self, decision: DecisionId, delivery: DeliveryState) {
        if let Some(record) = self
            .handoffs
            .iter_mut()
            .find(|h| h.handoff.decision == decision)
        {
            record.delivery = delivery;
        }
    }
}

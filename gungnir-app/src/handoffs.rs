//! The handoff record (GAP-040, DN-07 §5): what a decided assignment carries to an
//! effector, built only from a recorded decision.
//!
//! `Handoff::from_decision` existed with no caller. It is called here, once per actionable
//! decision, at the moment the engagements open. **Delivery is claimed only when the
//! endpoint said so**: a resource with no `handoff_endpoint` is manual -- a radio call the
//! operator is told to make; one with an `http` endpoint is posted through the transport
//! (GAP-040, 2026-09-06) and stays undelivered until the endpoint accepts, refused if it
//! refuses, retried and never dropped if it is silent; one with an endpoint of any other
//! kind is undelivered with the reason. DN-07 §5 case 3's rule throughout.

use crate::state::AppState;
use crate::update::publish;
use gungnir_command::DecisionRecord;
use gungnir_eventing::Event;
use gungnir_model::events::HandoffEvent;
use gungnir_model::handoff::{DecisionAttribution, DeliveryState, Handoff};
use gungnir_model::Releasability;

/// One issued handoff and where its delivery stands.
#[derive(Debug, Clone, PartialEq)]
pub struct HandoffRecord {
    pub handoff: Handoff,
    pub endpoint: Option<String>,
    pub delivery: DeliveryState,
    /// Every report the effector sent about this handoff, oldest first (GAP-040).
    ///
    /// Kept rather than folded away because the delivery state cannot carry the
    /// sequence: `Acknowledged` then `Completed` leaves `delivery` on `Delivered`, and
    /// `Executing` leaves no trace on it at all. PN-20's after-action account asks what
    /// came back, and a review that could see only the final state could not tell an
    /// engagement that was acknowledged and then went quiet from one that was never
    /// acknowledged.
    pub reports: Vec<gungnir_model::handoff::EffectorReport>,
}

/// Apply what the effector reported through the node (GAP-040, DN-06).
///
/// A report naming a decision this desktop did not hand off is rejected and said, as an
/// untrusted external input is; the node could not know, because it holds no handoffs.
/// `Executing` moves the engagement; `Completed` closes it with effector-reported
/// evidence, which the effect measures count apart from what the track's lifecycle
/// suggested; `Refused` sets the delivery state so PN-06 says the effector will not.
pub fn apply_report(
    state: &mut AppState,
    decision: gungnir_model::DecisionId,
    endpoint: &str,
    report: &gungnir_model::handoff::EffectorReport,
    at: gungnir_model::MissionTime,
) {
    use gungnir_model::handoff::{accept_report, EffectorReport};
    let handoffs: Vec<Handoff> = state.handoffs.iter().map(|r| r.handoff.clone()).collect();
    if let Err(err) = accept_report(&handoffs, decision, report) {
        state.alerts.push(format!(
            "effector report from {endpoint} rejected: {err}; this desktop issued no such handoff"
        ));
        return;
    }
    // Kept before it is acted on, so PN-20 shows what came back even when the engagement
    // could not take it: a report the desktop rejected downstream still happened.
    if let Some(record) = state
        .handoffs
        .iter_mut()
        .find(|h| h.handoff.decision == decision)
    {
        record.reports.push(report.clone());
    }
    let outcome = match report {
        EffectorReport::Acknowledged { .. } => {
            format!(
                "decision {}: {endpoint} acknowledged the handoff",
                decision.0
            )
        }
        EffectorReport::Executing { at } => {
            match state
                .engagements
                .iter_mut()
                .find(|e| e.decision == decision)
                .map(|e| e.executing(*at))
            {
                Some(Ok(())) => format!("decision {}: {endpoint} is executing", decision.0),
                Some(Err(err)) => format!(
                    "decision {}: {endpoint} reports executing, and the engagement could not \
                     take it: {err}",
                    decision.0
                ),
                None => format!(
                    "decision {}: {endpoint} reports executing; no open engagement",
                    decision.0
                ),
            }
        }
        EffectorReport::Completed {
            at,
            effective,
            detail,
        } => close_engagement(state, decision, endpoint, (*at, *effective, detail)),
        EffectorReport::Refused { at, reason } => {
            if let Some(record) = state
                .handoffs
                .iter_mut()
                .find(|h| h.handoff.decision == decision)
            {
                record.delivery = DeliveryState::Refused {
                    reason: reason.clone(),
                    at: *at,
                };
            }
            format!(
                "decision {}: {endpoint} refused the handoff: {reason}",
                decision.0
            )
        }
    };
    let _ = at;
    crate::audit::record(
        state,
        gungnir_security::actions::EFFECTOR_REPORT,
        outcome.clone(),
    );
    state.alerts.push(outcome);
}

/// Close the engagement a `Completed` report names, and say what happened (DN-06).
///
/// The evidence is stamped `EffectSource::EffectorReport` so the effect measures can
/// count what the effector said apart from what the track's lifecycle suggested; the two
/// are different grades of evidence and averaging them would flatter the second.
///
/// A report the engagement cannot take -- because it is already closed, or because none
/// was ever opened -- is said rather than swallowed. It is still on the handoff record for
/// PN-20, because a report the desktop could not act on still arrived.
fn close_engagement(
    state: &mut AppState,
    decision: gungnir_model::DecisionId,
    endpoint: &str,
    (at, effective, detail): (gungnir_model::MissionTime, bool, &str),
) -> String {
    use gungnir_intercept_service::engagement::{EffectEvidence, EffectSource};
    let evidence = EffectEvidence {
        source: EffectSource::EffectorReport,
        observed_at: at,
        detail: format!("{endpoint}: {detail}"),
    };
    let closed = state
        .engagements
        .iter_mut()
        .find(|e| e.decision == decision)
        .map(|e| {
            if effective {
                e.close_effective(evidence)
            } else {
                e.close_ineffective(evidence)
            }
        });
    match closed {
        Some(Ok(())) => format!(
            "decision {}: {endpoint} reports {}",
            decision.0,
            if effective {
                "effective"
            } else {
                "ineffective"
            }
        ),
        Some(Err(err)) => format!(
            "decision {}: {endpoint} reports completion the engagement could not take: {err}",
            decision.0
        ),
        None => format!(
            "decision {}: {endpoint} reports completion; no open engagement",
            decision.0
        ),
    }
}

/// Issue the handoff for an actionable decision. Called from the engagement path, which
/// has already refused a record that is not actionable.
pub fn issue_for(state: &mut AppState, record: &DecisionRecord) {
    if !record.is_actionable() {
        return;
    }
    let now = record.mission_time;
    let tracks = state.tracking.tracks();
    let track_provenance: Vec<_> = record
        .plan
        .solutions()
        .iter()
        .filter_map(|s| {
            tracks
                .iter()
                .find(|t| t.id == s.track)
                .map(|t| (t.id, t.provenance.clone(), t.quality))
        })
        .collect();
    let releasability = Releasability::combine(
        record
            .plan
            .solutions()
            .iter()
            .filter_map(|s| tracks.iter().find(|t| t.id == s.track))
            .map(|t| t.releasability.clone()),
    );
    // DN-23 §5 rule 1: attribution is never invented. With nobody signed in the record
    // says so in words, and the role is the one that was selected.
    let decided_by = DecisionAttribution {
        operator: record
            .operator_id
            .clone()
            .unwrap_or_else(|| "nobody signed in".to_string()),
        role: format!("{:?}", state.role()),
        at: record.mission_time,
        authority_rule: None,
    };
    let handoff = Handoff::from_decision(
        record.id,
        record.plan.id,
        record.plan.kind.clone(),
        decided_by,
        track_provenance,
        releasability,
        now,
    );
    // The endpoint is the first tasked resource's; a plan tasking resources with
    // different endpoints is two handoffs in DN-07's shape and one here, said in the
    // record rather than silently split.
    let endpoint = record.plan.solutions().iter().find_map(|s| {
        state
            .config
            .resources
            .iter()
            .find(|r| r.id == s.resource.0)
            .and_then(|r| r.handoff_endpoint.clone())
    });
    publish(
        state,
        now,
        Event::Handoff(HandoffEvent::Issued {
            decision: record.id,
            endpoint: endpoint.clone(),
            at: now,
        }),
    );
    let delivery = record_delivery(state, record.id, now, endpoint.as_deref(), &handoff);
    state.handoffs.push(HandoffRecord {
        handoff,
        endpoint,
        delivery,
        reports: Vec::new(),
    });
}

/// The handoff rows PN-06 and PN-20 draw (GAP-040).
///
/// One builder for both panels, borrowed from the record for the frame. Unfiltered: PN-06
/// applies `handoff::stays_visible` itself so the *stays visible until delivered* rule
/// lives in one place, and PN-20 wants every row anyway.
#[must_use]
pub fn rows(state: &AppState) -> Vec<gungnir_ui::panels::handoff::HandoffRow<'_>> {
    state
        .handoffs
        .iter()
        .map(|record| gungnir_ui::panels::handoff::HandoffRow {
            decision: record.handoff.decision.0,
            plan: record.handoff.plan.0,
            endpoint: record.endpoint.as_deref(),
            operator: &record.handoff.decided_by.operator,
            role: &record.handoff.decided_by.role,
            issued: record.handoff.issued,
            delivery: &record.delivery,
            reports: &record.reports,
        })
        .collect()
}

/// Where delivery stands the moment the handoff is issued, on the record and on PN-08.
fn record_delivery(
    state: &mut AppState,
    decision: gungnir_model::DecisionId,
    now: gungnir_model::MissionTime,
    endpoint: Option<&str>,
    handoff: &Handoff,
) -> DeliveryState {
    match endpoint {
        None => {
            publish(
                state,
                now,
                Event::Handoff(HandoffEvent::Manual { decision, at: now }),
            );
            state.alerts.push(format!(
                "decision {}: no handoff endpoint is configured for the tasked resource; \
                 the handoff is manual and must be made by voice",
                decision.0
            ));
            DeliveryState::Manual
        }
        Some(name) => {
            let reason = match crate::deliveries::http_address(state, name) {
                Ok(address) => {
                    let payload = serde_json::to_value(handoff).unwrap_or(serde_json::Value::Null);
                    crate::deliveries::post_handoff(
                        state,
                        decision,
                        name.to_string(),
                        address,
                        payload,
                        now,
                    );
                    state.alerts.push(format!(
                        "decision {}: handoff posted to {name}; awaiting the endpoint",
                        decision.0
                    ));
                    return DeliveryState::Undelivered { since: now };
                }
                Err(reason) => reason,
            };
            publish(
                state,
                now,
                Event::Handoff(HandoffEvent::Undelivered {
                    decision,
                    endpoint: name.to_string(),
                    reason: reason.clone(),
                    at: now,
                }),
            );
            state.alerts.push(format!(
                "decision {}: handoff to {name} undelivered: {reason}",
                decision.0
            ));
            DeliveryState::Undelivered { since: now }
        }
    }
}

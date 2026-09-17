// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The handoff record (GAP-040, DN-07 §5): what a decided assignment carries to an
//! effector, built only from a recorded decision.
//!
//! **The one handoff builder** (`docs/design/DN-31-node-approval-queue.md` §3 point 4;
//! GAP-131, D-57). It is called once per actionable decision, at the moment the engagements
//! open, and `gungnir-app/tests/no_execution_without_decision.rs` pins that
//! `Handoff::from_decision` is constructed in exactly one place in the workspace -- which is
//! why the builder moved here whole rather than being copied for a second binary.
//!
//! **Delivery is claimed only when the endpoint said so**: a resource with no
//! `handoff_endpoint` is manual -- a radio call the operator is told to make; one with an
//! endpoint the host's transport carries is posted and stays undelivered until the endpoint
//! accepts, refused if it refuses, retried and never dropped if it is silent; one with an
//! endpoint of any other kind is undelivered with the reason. DN-07 §5 case 3's rule
//! throughout.

use crate::{ApprovalContext, ApprovalDesk, ApprovalHost};
use gungnir_command::DecisionRecord;
use gungnir_eventing::Event;
use gungnir_model::events::HandoffEvent;
use gungnir_model::handoff::{DecisionAttribution, DeliveryState, Handoff};
use gungnir_model::{DecisionId, MissionTime, Releasability};

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

impl ApprovalDesk {
    /// Issue the handoff for an actionable decision. Called from the engagement path,
    /// which has already refused a record that is not actionable.
    pub fn issue_for(
        &mut self,
        cx: &ApprovalContext<'_>,
        host: &mut dyn ApprovalHost,
        record: &DecisionRecord,
    ) {
        if !record.is_actionable() {
            return;
        }
        let now = record.mission_time;
        let track_provenance: Vec<_> = record
            .plan
            .solutions()
            .iter()
            .filter_map(|s| {
                cx.tracks
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
                .filter_map(|s| cx.tracks.iter().find(|t| t.id == s.track))
                .map(|t| t.releasability.clone()),
        );
        // DN-23 §5 rule 1: attribution is never invented. With nobody signed in the record
        // says so in words, and the role is the one the host is acting in.
        let decided_by = DecisionAttribution {
            operator: record
                .operator_id
                .clone()
                .unwrap_or_else(|| "nobody signed in".to_string()),
            role: cx.role_name(),
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
            cx.config
                .resources
                .iter()
                .find(|r| r.id == s.resource.0)
                .and_then(|r| r.handoff_endpoint.clone())
        });
        host.publish(
            now,
            Event::Handoff(HandoffEvent::Issued {
                decision: record.id,
                endpoint: endpoint.clone(),
                at: now,
            }),
        );
        let delivery = self.record_delivery(host, record.id, now, endpoint.as_deref(), &handoff);
        self.handoffs.push(HandoffRecord {
            handoff,
            endpoint,
            delivery,
            reports: Vec::new(),
        });
        // GAP-065, DN-18 §5 amendment 2: this host's whole current set, for a partner to
        // be served from, if the host has anywhere to publish it.
        host.republish_handoffs(&self.handoffs);
    }

    /// Apply what the effector reported through the node (GAP-040, DN-06).
    ///
    /// A report naming a decision this host did not hand off is rejected and said, as an
    /// untrusted external input is; the node could not know, because it holds no handoffs.
    /// `Executing` moves the engagement; `Completed` closes it with effector-reported
    /// evidence, which the effect measures count apart from what the track's lifecycle
    /// suggested; `Refused` sets the delivery state so PN-06 says the effector will not.
    pub fn apply_report(
        &mut self,
        host: &mut dyn ApprovalHost,
        decision: DecisionId,
        endpoint: &str,
        report: &gungnir_model::handoff::EffectorReport,
        at: MissionTime,
    ) {
        use gungnir_model::handoff::{accept_report, EffectorReport};
        let handoffs: Vec<Handoff> = self.handoffs.iter().map(|r| r.handoff.clone()).collect();
        if accept_report(&handoffs, decision, report).is_err() {
            // By its tag, as every alert names a decision (D-61). The report named a
            // decision this host never handed off; since GAP-130 that is a decision some
            // other machine took, not a second machine's decision under the same number.
            host.alert(format!(
                "effector report from {endpoint} rejected: it names decision {}, and this desktop \
                 issued no such handoff",
                decision.short()
            ));
            return;
        }
        // Kept before it is acted on, so PN-20 shows what came back even when the
        // engagement could not take it: a report the host rejected downstream still
        // happened.
        if let Some(record) = self
            .handoffs
            .iter_mut()
            .find(|h| h.handoff.decision == decision)
        {
            record.reports.push(report.clone());
        }
        // What happened, without the decision: the alert names it by its tag and the audit
        // entry names it whole, because an audit entry is searched for (D-61).
        let outcome = match report {
            EffectorReport::Acknowledged { .. } => {
                format!("{endpoint} acknowledged the handoff")
            }
            EffectorReport::Executing { at } => {
                match self
                    .engagements
                    .iter_mut()
                    .find(|e| e.decision == decision)
                    .map(|e| e.executing(*at))
                {
                    Some(Ok(())) => format!("{endpoint} is executing"),
                    Some(Err(err)) => format!(
                        "{endpoint} reports executing, and the engagement could not take it: {}",
                        refusal(&err)
                    ),
                    None => format!("{endpoint} reports executing; no open engagement"),
                }
            }
            EffectorReport::Completed {
                at,
                effective,
                detail,
            } => self.close_engagement(decision, endpoint, (*at, *effective, detail)),
            EffectorReport::Refused { at, reason } => {
                if let Some(record) = self
                    .handoffs
                    .iter_mut()
                    .find(|h| h.handoff.decision == decision)
                {
                    record.delivery = DeliveryState::Refused {
                        reason: reason.clone(),
                        at: *at,
                    };
                }
                format!("{endpoint} refused the handoff: {reason}")
            }
        };
        let _ = at;
        host.audit(
            gungnir_security::actions::EFFECTOR_REPORT,
            format!("decision {decision}: {outcome}"),
        );
        host.alert(format!("decision {}: {outcome}", decision.short()));
    }

    /// Close the engagement a `Completed` report names, and say what happened (DN-06), in
    /// words that leave the decision for the caller to name.
    ///
    /// The evidence is stamped `EffectSource::EffectorReport` so the effect measures can
    /// count what the effector said apart from what the track's lifecycle suggested; the
    /// two are different grades of evidence and averaging them would flatter the second.
    ///
    /// A report the engagement cannot take -- because it is already closed, or because none
    /// was ever opened -- is said rather than swallowed. It is still on the handoff record
    /// for PN-20, because a report the host could not act on still arrived.
    fn close_engagement(
        &mut self,
        decision: DecisionId,
        endpoint: &str,
        (at, effective, detail): (MissionTime, bool, &str),
    ) -> String {
        use gungnir_intercept_service::engagement::{EffectEvidence, EffectSource};
        let evidence = EffectEvidence {
            source: EffectSource::EffectorReport,
            observed_at: at,
            detail: format!("{endpoint}: {detail}"),
        };
        let closed = self
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
                "{endpoint} reports {}",
                if effective {
                    "effective"
                } else {
                    "ineffective"
                }
            ),
            Some(Err(err)) => format!(
                "{endpoint} reports completion the engagement could not take: {}",
                refusal(&err)
            ),
            None => format!("{endpoint} reports completion; no open engagement"),
        }
    }

    /// Where delivery stands the moment the handoff is issued, on the record and on PN-08.
    fn record_delivery(
        &mut self,
        host: &mut dyn ApprovalHost,
        decision: DecisionId,
        now: MissionTime,
        endpoint: Option<&str>,
        handoff: &Handoff,
    ) -> DeliveryState {
        match endpoint {
            None => {
                host.publish(
                    now,
                    Event::Handoff(HandoffEvent::Manual { decision, at: now }),
                );
                host.alert(format!(
                    "decision {}: no handoff endpoint is configured for the tasked resource; \
                     the handoff is manual and must be made by voice",
                    decision.short()
                ));
                DeliveryState::Manual
            }
            Some(name) => {
                let reason = match host.address_for(name) {
                    Ok(address) => {
                        let payload =
                            serde_json::to_value(handoff).unwrap_or(serde_json::Value::Null);
                        self.post_handoff(host, decision, name.to_string(), address, payload, now);
                        host.alert(format!(
                            "decision {}: handoff posted to {name}; awaiting the endpoint",
                            decision.short()
                        ));
                        return DeliveryState::Undelivered { since: now };
                    }
                    Err(reason) => reason,
                };
                host.publish(
                    now,
                    Event::Handoff(HandoffEvent::Undelivered {
                        decision,
                        endpoint: name.to_string(),
                        reason: reason.clone(),
                        at: now,
                    }),
                );
                host.alert(format!(
                    "decision {}: handoff to {name} undelivered: {reason}",
                    decision.short()
                ));
                DeliveryState::Undelivered { since: now }
            }
        }
    }
}

/// Why an engagement could not take a report, in an alert's words.
///
/// The sentence it ends has already named the decision, by its tag on an alert and whole
/// in the audit entry, and the error's own text names it whole, which is right for a log
/// and wrong on a panel (D-61).
fn refusal(err: &gungnir_intercept_service::engagement::EngagementError) -> &'static str {
    use gungnir_intercept_service::engagement::EngagementError;
    match err {
        EngagementError::AlreadyClosed(_) => "it is already closed",
    }
}

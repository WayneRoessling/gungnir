// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Resilience & disconnected operations, per docs/gungnir-capabilities.md §5.6 and
//! ARCHITECTURE.md §8.4. A connected desktop that loses its node keeps operating
//! on the embedded services and keeps journaling locally; when the link returns,
//! what it recorded is forwarded and the two journals are reconciled.
//!
//! [`reconcile`] merges by mission time, drops exact duplicates, and **reports** the
//! conflicting decisions it finds; it resolves none of them. D-03 locked the rule that
//! does (higher role wins, earlier decision wins a tie), DN-10 §9 put "a decision beats an
//! expiry" ahead of it, and the rule is `gungnir_model::arbitration::arbitrate`. Since the
//! owner's GAP-067 walk (2026-09-16) the desktop applies it to every conflict reported
//! here and leaves to a person only those it cannot rank (`gungnir-app`'s `failover`).
//! Resolving stays out of this crate on purpose: ranking a side means reading a role, and
//! this crate depends on the model, eventing and the store alone.

use gungnir_eventing::{Envelope, Event};
use gungnir_model::arbitration::{ConflictSide, SideOutcome};
use gungnir_model::events::CommandEvent;
use gungnir_model::{MissionTime, PlanId};
use gungnir_store::SessionId;
use std::collections::VecDeque;

/// Bounded queue of envelopes awaiting forwarding. When full, the oldest is
/// dropped and counted; nothing blocks.
#[derive(Debug)]
pub struct StoreAndForwardQueue {
    pending: VecDeque<Envelope>,
    capacity: usize,
    dropped: u64,
}

impl StoreAndForwardQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            pending: VecDeque::new(),
            capacity: capacity.max(1),
            dropped: 0,
        }
    }

    pub fn push(&mut self, envelope: Envelope) {
        if self.pending.len() >= self.capacity {
            self.pending.pop_front();
            self.dropped += 1;
        }
        self.pending.push_back(envelope);
    }

    pub fn drain(&mut self) -> Vec<Envelope> {
        self.pending.drain(..).collect()
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }
}

/// Where a desktop or node was in a session, for recovery after a restart.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Checkpoint {
    pub session: SessionId,
    pub last_seq: u64,
    pub mission_time: MissionTime,
}

/// Two records of the same plan that disagree about how its approval ended.
///
/// Each side carries what the arbitration rule reads -- outcome, operator, role, mission
/// time -- so the rule can be applied to the conflict as reported, without going back to
/// either journal for more.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionConflict {
    pub plan: PlanId,
    /// What this desktop's journal recorded.
    pub local: ConflictSide,
    /// What the node's journal recorded.
    pub remote: ConflictSide,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReconcileReport {
    /// Both journals merged in mission-time order, exact duplicates removed.
    pub merged: Vec<Envelope>,
    pub duplicates_dropped: usize,
    pub conflicts: Vec<DecisionConflict>,
}

/// How an envelope ended a plan's approval, if it did.
///
/// A decision's time is its envelope's, which is when the workflow recorded it; an
/// expiry's is its own `at`, which is when the window closed.
fn ending(env: &Envelope) -> Option<(PlanId, ConflictSide)> {
    match &env.event {
        Event::Command(CommandEvent::Decided {
            plan,
            accepted,
            operator,
            role,
            ..
        }) => Some((
            *plan,
            ConflictSide {
                outcome: if *accepted {
                    SideOutcome::Accepted
                } else {
                    SideOutcome::Rejected
                },
                operator: operator.clone(),
                role: role.clone(),
                at: env.mission_time,
            },
        )),
        Event::Command(CommandEvent::Expired { plan, at }) => Some((
            *plan,
            ConflictSide {
                outcome: SideOutcome::Expired,
                operator: None,
                role: None,
                at: *at,
            },
        )),
        _ => None,
    }
}

/// Merge a desktop's offline journal with the node's journal for the same period.
///
/// **A conflict is two different outcomes for one plan**, one from each journal: an
/// acceptance against a rejection, as before, and since the GAP-067 walk an expiry against
/// either. An expiry facing a decision is a conflict even though neither acted on the
/// plan when the decision was a rejection, because an expiry is not a rejection (DN-10)
/// and the record has to say which of the two stands. Two acceptances, two rejections, and
/// two expiries agree, and are not conflicts.
pub fn reconcile(local: &[Envelope], remote: &[Envelope]) -> ReconcileReport {
    let mut conflicts = Vec::new();
    let remote_endings: Vec<(PlanId, ConflictSide)> = remote.iter().filter_map(ending).collect();
    for (plan, l) in local.iter().filter_map(ending) {
        for (other, r) in &remote_endings {
            if plan == *other && l.outcome != r.outcome {
                conflicts.push(DecisionConflict {
                    plan,
                    local: l.clone(),
                    remote: r.clone(),
                });
            }
        }
    }

    let mut all: Vec<&Envelope> = local.iter().chain(remote.iter()).collect();
    all.sort_by(|a, b| {
        a.mission_time
            .partial_cmp(&b.mission_time)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut merged: Vec<Envelope> = Vec::with_capacity(all.len());
    let mut duplicates_dropped = 0;
    for env in all {
        if merged
            .iter()
            .any(|m| m.mission_time == env.mission_time && m.event == env.event)
        {
            duplicates_dropped += 1;
        } else {
            merged.push(env.clone());
        }
    }
    ReconcileReport {
        merged,
        duplicates_dropped,
        conflicts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_eventing::TrackingEvent;
    use gungnir_model::TrackId;

    fn env(seq: u64, t: f64, event: Event) -> Envelope {
        Envelope {
            seq,
            mission_time: MissionTime(t),
            event,
        }
    }

    fn deleted(id: u64) -> Event {
        Event::Tracking(TrackingEvent::TrackDeleted(TrackId(id)))
    }

    fn decided(plan: u128, accepted: bool) -> Event {
        decided_by(plan, accepted, None, None)
    }

    fn decided_by(plan: u128, accepted: bool, operator: Option<&str>, role: Option<&str>) -> Event {
        Event::Command(CommandEvent::Decided {
            plan: PlanId(plan),
            decision: gungnir_model::DecisionId(plan),
            accepted,
            operator: operator.map(str::to_string),
            role: role.map(str::to_string),
            verdict: gungnir_model::events::VerdictSummary::RequiresHumanApproval,
            rationale: None,
            request: None,
            origin: None,
        })
    }

    fn expired(plan: u128, at: f64) -> Event {
        Event::Command(CommandEvent::Expired {
            plan: PlanId(plan),
            at: MissionTime(at),
        })
    }

    fn side(outcome: SideOutcome, t: f64) -> ConflictSide {
        ConflictSide {
            outcome,
            operator: None,
            role: None,
            at: MissionTime(t),
        }
    }

    #[test]
    fn queue_drops_oldest_when_full_and_counts_it() {
        let mut q = StoreAndForwardQueue::new(2);
        q.push(env(1, 1.0, deleted(1)));
        q.push(env(2, 2.0, deleted(2)));
        q.push(env(3, 3.0, deleted(3)));
        assert_eq!(q.len(), 2);
        assert_eq!(q.dropped(), 1);
        let drained = q.drain();
        assert_eq!(drained[0].seq, 2);
        assert!(q.is_empty());
    }

    #[test]
    fn reconcile_merges_by_time_drops_duplicates_and_reports_conflicts() {
        let local = vec![
            env(1, 1.0, deleted(1)),
            env(2, 3.0, decided(7, true)),
            env(3, 4.0, deleted(4)),
        ];
        let remote = vec![
            env(10, 1.0, deleted(1)),
            env(11, 2.0, deleted(2)),
            env(12, 3.5, decided(7, false)),
        ];
        let report = reconcile(&local, &remote);
        assert_eq!(report.duplicates_dropped, 1);
        let times: Vec<f64> = report.merged.iter().map(|e| e.mission_time.0).collect();
        assert_eq!(times, vec![1.0, 2.0, 3.0, 3.5, 4.0]);
        assert_eq!(
            report.conflicts,
            vec![DecisionConflict {
                plan: PlanId(7),
                local: side(SideOutcome::Accepted, 3.0),
                remote: side(SideOutcome::Rejected, 3.5),
            }]
        );
    }

    /// An expiry on one side against a decision on the other is a conflict, whichever
    /// journal holds which, and whether the decision accepted or rejected: an expiry is
    /// not a rejection (DN-10), so an expiry against a rejection still disagrees about
    /// what ended the approval.
    #[test]
    fn an_expiry_against_a_decision_is_a_conflict_in_either_order() {
        let local_expired = reconcile(
            &[env(1, 5.0, expired(7, 5.0))],
            &[env(10, 6.0, decided(7, true))],
        );
        assert_eq!(
            local_expired.conflicts,
            vec![DecisionConflict {
                plan: PlanId(7),
                local: side(SideOutcome::Expired, 5.0),
                remote: side(SideOutcome::Accepted, 6.0),
            }]
        );

        let remote_expired = reconcile(
            &[env(1, 6.0, decided(7, false))],
            &[env(10, 5.0, expired(7, 5.0))],
        );
        assert_eq!(
            remote_expired.conflicts,
            vec![DecisionConflict {
                plan: PlanId(7),
                local: side(SideOutcome::Rejected, 6.0),
                remote: side(SideOutcome::Expired, 5.0),
            }]
        );
    }

    /// Two decisions that disagree conflict as they always have; records that agree --
    /// two acceptances, two rejections, two expiries -- do not, and neither do endings of
    /// two different plans.
    #[test]
    fn only_different_endings_of_the_same_plan_conflict() {
        let disagree = reconcile(
            &[env(1, 5.0, decided(7, false))],
            &[env(10, 6.0, decided(7, true))],
        );
        assert_eq!(disagree.conflicts.len(), 1);

        for (l, r) in [
            (decided(7, true), decided(7, true)),
            (decided(7, false), decided(7, false)),
            (expired(7, 5.0), expired(7, 6.0)),
            (decided(7, true), decided(8, false)),
            (expired(7, 5.0), decided(8, true)),
        ] {
            let report = reconcile(&[env(1, 5.0, l.clone())], &[env(10, 6.0, r.clone())]);
            assert!(
                report.conflicts.is_empty(),
                "{l:?} against {r:?} was reported as a conflict"
            );
        }
    }

    /// Each side carries exactly what the arbitration rule reads, as its journal recorded
    /// it: an operator and a role where one was recorded, none where none was, and the
    /// decision's envelope time or the expiry's own `at`.
    #[test]
    fn each_side_carries_what_the_rule_reads() {
        let report = reconcile(
            &[
                env(
                    1,
                    110.0,
                    decided_by(1, false, Some("7"), Some("Supervisor")),
                ),
                env(2, 111.0, expired(2, 110.5)),
            ],
            &[
                env(10, 112.0, decided_by(1, true, Some("9"), None)),
                env(11, 113.0, decided_by(2, true, None, None)),
            ],
        );
        assert_eq!(
            report.conflicts,
            vec![
                DecisionConflict {
                    plan: PlanId(1),
                    local: ConflictSide {
                        outcome: SideOutcome::Rejected,
                        operator: Some("7".into()),
                        role: Some("Supervisor".into()),
                        at: MissionTime(110.0),
                    },
                    remote: ConflictSide {
                        outcome: SideOutcome::Accepted,
                        operator: Some("9".into()),
                        role: None,
                        at: MissionTime(112.0),
                    },
                },
                DecisionConflict {
                    plan: PlanId(2),
                    local: ConflictSide {
                        outcome: SideOutcome::Expired,
                        operator: None,
                        role: None,
                        at: MissionTime(110.5),
                    },
                    remote: ConflictSide {
                        outcome: SideOutcome::Accepted,
                        operator: None,
                        role: None,
                        at: MissionTime(113.0),
                    },
                },
            ]
        );
    }

    /// "Merged journal contains every envelope once" (the cross-layer row): the exact
    /// number of duplicates is dropped, the merge is the two journals less exactly those,
    /// and every distinct envelope -- the same event at another time, another event at the
    /// same time -- is in it exactly once.
    #[test]
    fn the_merge_keeps_every_distinct_envelope_exactly_once() {
        let local = vec![
            env(1, 1.0, deleted(1)),
            env(2, 2.0, decided(5, true)),
            env(3, 3.0, deleted(3)),
            env(4, 4.0, expired(6, 4.0)),
        ];
        let remote = vec![
            // Duplicates of local envelopes: same time, same event, another sequence.
            env(10, 1.0, deleted(1)),
            env(11, 2.0, decided(5, true)),
            // Not duplicates: the same event at another time, another event at a time
            // already taken.
            env(12, 5.0, deleted(3)),
            env(13, 3.0, deleted(9)),
        ];
        let report = reconcile(&local, &remote);
        assert_eq!(report.duplicates_dropped, 2);
        assert_eq!(report.merged.len(), local.len() + remote.len() - 2);
        for envelope in local.iter().chain(remote.iter()) {
            let copies = report
                .merged
                .iter()
                .filter(|m| m.mission_time == envelope.mission_time && m.event == envelope.event)
                .count();
            assert_eq!(copies, 1, "{envelope:?} is in the merge {copies} times");
        }
        assert!(
            report
                .merged
                .windows(2)
                .all(|w| w[0].mission_time <= w[1].mission_time),
            "the merge is not in mission-time order"
        );
    }
}

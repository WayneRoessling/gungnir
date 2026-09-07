// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Resilience & disconnected operations, per docs/gungnir-capabilities.md §5.6 and
//! ARCHITECTURE.md §8.4. A connected desktop that loses its node keeps operating
//! on the embedded services and keeps journaling locally; when the link returns,
//! what it recorded is forwarded and the two journals are reconciled.
//!
//! The reconciliation *rule* (who wins on conflicting operator decisions) has a
//! working default in `gungnir-collab` (`RoleRankArbiter`) that is not yet locked
//! (ARCHITECTURE.md §10). [`reconcile`] therefore merges by mission time, drops
//! exact duplicates, and *reports* conflicts rather than resolving them; the
//! arbiter is where the rule plugs in.

use gungnir_eventing::{Envelope, Event};
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

/// Two decisions on the same plan that disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionConflict {
    pub plan: PlanId,
    pub local_accepted: bool,
    pub remote_accepted: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReconcileReport {
    /// Both journals merged in mission-time order, exact duplicates removed.
    pub merged: Vec<Envelope>,
    pub duplicates_dropped: usize,
    pub conflicts: Vec<DecisionConflict>,
}

fn decision(env: &Envelope) -> Option<(PlanId, bool)> {
    match &env.event {
        Event::Command(CommandEvent::Decided { plan, accepted, .. }) => Some((*plan, *accepted)),
        _ => None,
    }
}

/// Merge a desktop's offline journal with the node's journal for the same period.
pub fn reconcile(local: &[Envelope], remote: &[Envelope]) -> ReconcileReport {
    let mut conflicts = Vec::new();
    for l in local.iter().filter_map(decision) {
        for r in remote.iter().filter_map(decision) {
            if l.0 == r.0 && l.1 != r.1 {
                conflicts.push(DecisionConflict {
                    plan: l.0,
                    local_accepted: l.1,
                    remote_accepted: r.1,
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

    fn decided(plan: u64, accepted: bool) -> Event {
        Event::Command(CommandEvent::Decided {
            plan: PlanId(plan),
            decision: gungnir_model::DecisionId(plan),
            accepted,
            operator: None,
            verdict: gungnir_model::events::VerdictSummary::RequiresHumanApproval,
            rationale: None,
        })
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
                local_accepted: true,
                remote_accepted: false
            }]
        );
    }
}

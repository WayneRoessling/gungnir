// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Collaboration & multi-user concurrency, per docs/gungnir-capabilities.md §5.6
//! and ARCHITECTURE.md §8. In the connected profiles several desktops share one
//! mission through a service node that is the system of record; each desktop's
//! `AppState` is a projection kept current by the node's event stream. This crate
//! holds the pieces that are about *people* sharing that picture: applying remote
//! envelopes, draining local ones, and resolving conflicting operator decisions.

use gungnir_command::DecisionRecord;
use gungnir_eventing::Envelope;
use gungnir_security::{OperatorId, Role};

#[derive(Debug, thiserror::Error)]
pub enum CollabError {
    #[error("envelope seq {seq} is older than the last applied seq {last}")]
    StaleEnvelope { seq: u64, last: u64 },
}

/// The two directions of a shared picture: what arrives from the node, and what
/// this desktop produced while acting locally.
pub trait SharedPictureSync: Send + Sync {
    fn apply_remote(&mut self, envelope: &Envelope) -> Result<(), CollabError>;
    fn drain_local(&mut self) -> Vec<Envelope>;
    /// Highest node sequence number applied so far.
    fn last_applied_seq(&self) -> Option<u64>;
}

/// Who made a decision and with what authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthorityContext {
    pub operator: OperatorId,
    pub role: Role,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    KeepFirst,
    KeepSecond,
}

/// Decides between two conflicting decisions on the same plan.
///
/// An expiry is not a decision, and since the DN-10 §3 conformance the arbiter can see
/// that: `DecisionRecord::is_expiry` answers it from the record rather than from whether
/// an operator happens to be named. [`RoleRankArbiter`] uses it, because arbitrating an
/// expiry by role rank would mean inventing a rank for the person who did not decide.
pub trait AuthorityArbiter: Send + Sync {
    fn resolve(
        &self,
        first: (&DecisionRecord, AuthorityContext),
        second: (&DecisionRecord, AuthorityContext),
    ) -> Resolution;
}

/// A real decision beats an expiry; otherwise the higher role wins, and on equal rank
/// the earlier decision wins.
///
/// The expiry rule is a decision-authority rule and was **signed by the owner on
/// 2026-09-05** with DN-10 amendment 1 (§9).
///
/// The expiry rule comes first because it is not a tie-break at all. If one site's
/// window closed and another site's operator decided, there is nothing to arbitrate:
/// one of them chose. Ranking them would require an `AuthorityContext` for the expiry,
/// and whatever role were supplied for a person who did not act would decide the
/// outcome -- a fabricated authority beating a real one whenever the fabrication ranked
/// higher.
#[derive(Debug, Default, Clone, Copy)]
pub struct RoleRankArbiter;

impl AuthorityArbiter for RoleRankArbiter {
    fn resolve(
        &self,
        first: (&DecisionRecord, AuthorityContext),
        second: (&DecisionRecord, AuthorityContext),
    ) -> Resolution {
        let (a, ctx_a) = first;
        let (b, ctx_b) = second;
        // Somebody chose beats nobody chose, whatever authority the expiry was
        // presented with.
        match (a.is_expiry(), b.is_expiry()) {
            (true, false) => return Resolution::KeepSecond,
            (false, true) => return Resolution::KeepFirst,
            // Two expiries, or two decisions: fall through to rank.
            (true, true) | (false, false) => {}
        }
        match ctx_a.role.rank().cmp(&ctx_b.role.rank()) {
            std::cmp::Ordering::Greater => Resolution::KeepFirst,
            std::cmp::Ordering::Less => Resolution::KeepSecond,
            std::cmp::Ordering::Equal => {
                if a.mission_time <= b.mission_time {
                    Resolution::KeepFirst
                } else {
                    Resolution::KeepSecond
                }
            }
        }
    }
}

/// Minimal in-memory sync state: tracks the applied sequence and buffers local
/// envelopes for the node.
#[derive(Debug, Default)]
pub struct InMemorySharedPicture {
    last_seq: Option<u64>,
    local: Vec<Envelope>,
    applied: Vec<Envelope>,
}

impl InMemorySharedPicture {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_local(&mut self, envelope: Envelope) {
        self.local.push(envelope);
    }

    pub fn applied(&self) -> &[Envelope] {
        &self.applied
    }
}

impl SharedPictureSync for InMemorySharedPicture {
    fn apply_remote(&mut self, envelope: &Envelope) -> Result<(), CollabError> {
        if let Some(last) = self.last_seq {
            if envelope.seq <= last {
                return Err(CollabError::StaleEnvelope {
                    seq: envelope.seq,
                    last,
                });
            }
        }
        self.last_seq = Some(envelope.seq);
        self.applied.push(envelope.clone());
        Ok(())
    }

    fn drain_local(&mut self) -> Vec<Envelope> {
        std::mem::take(&mut self.local)
    }

    fn last_applied_seq(&self) -> Option<u64> {
        self.last_seq
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_command::OperatorDecision;
    use gungnir_eventing::{Event, TrackingEvent};
    use gungnir_model::{MissionTime, PlanView, TrackId};
    use gungnir_policy::PolicyVerdict;

    fn record(t: f64, decision: OperatorDecision) -> DecisionRecord {
        DecisionRecord {
            id: gungnir_model::DecisionId(1),
            plan: PlanView::default(),
            verdict: PolicyVerdict::RequiresHumanApproval,
            decision,
            operator_id: None,
            mission_time: MissionTime(t),
        }
    }

    fn ctx(id: u64, role: Role) -> AuthorityContext {
        AuthorityContext {
            operator: OperatorId(id),
            role,
        }
    }

    /// Somebody choosing beats nobody choosing, even when the expiry is presented with
    /// the higher authority. Before `is_expiry` the arbiter could not tell, and a
    /// fabricated rank on the expiry would have won.
    #[test]
    fn a_real_decision_beats_an_expiry_whatever_rank_it_carries() {
        let expiry = record(
            1.0,
            OperatorDecision::Expired {
                at: gungnir_model::MissionTime(1.0),
            },
        );
        let decided = record(2.0, OperatorDecision::Accepted);

        let commander = ctx(1, Role::Commander);
        let operator = ctx(2, Role::Operator);

        // The expiry carries the higher rank and still loses.
        assert_eq!(
            RoleRankArbiter.resolve((&expiry, commander), (&decided, operator)),
            Resolution::KeepSecond
        );
        assert_eq!(
            RoleRankArbiter.resolve((&decided, operator), (&expiry, commander)),
            Resolution::KeepFirst
        );

        // Two expiries fall through to the ordinary rule rather than being refused.
        let other_expiry = record(
            3.0,
            OperatorDecision::Expired {
                at: gungnir_model::MissionTime(3.0),
            },
        );
        assert_eq!(
            RoleRankArbiter.resolve((&expiry, commander), (&other_expiry, operator)),
            Resolution::KeepFirst
        );
    }

    #[test]
    fn supervisor_beats_operator_regardless_of_order() {
        let arb = RoleRankArbiter;
        let op = record(1.0, OperatorDecision::Accepted);
        let sup = record(
            2.0,
            OperatorDecision::Rejected {
                reason: "friendly airliner".into(),
            },
        );
        assert_eq!(
            arb.resolve(
                (&op, ctx(1, Role::Operator)),
                (&sup, ctx(2, Role::Supervisor))
            ),
            Resolution::KeepSecond
        );
        assert_eq!(
            arb.resolve(
                (&sup, ctx(2, Role::Supervisor)),
                (&op, ctx(1, Role::Operator))
            ),
            Resolution::KeepFirst
        );
    }

    #[test]
    fn equal_rank_earlier_wins() {
        let arb = RoleRankArbiter;
        let a = record(1.0, OperatorDecision::Accepted);
        let b = record(
            2.0,
            OperatorDecision::Rejected {
                reason: "friendly airliner".into(),
            },
        );
        assert_eq!(
            arb.resolve((&a, ctx(1, Role::Operator)), (&b, ctx(2, Role::Operator))),
            Resolution::KeepFirst
        );
        assert_eq!(
            arb.resolve((&b, ctx(2, Role::Operator)), (&a, ctx(1, Role::Operator))),
            Resolution::KeepSecond
        );
    }

    #[test]
    fn stale_envelopes_are_rejected() {
        let mut pic = InMemorySharedPicture::new();
        let env = |seq| Envelope {
            seq,
            mission_time: MissionTime(0.0),
            event: Event::Tracking(TrackingEvent::TrackDeleted(TrackId(seq))),
        };
        pic.apply_remote(&env(5)).expect("apply 5");
        assert!(matches!(
            pic.apply_remote(&env(5)),
            Err(CollabError::StaleEnvelope { .. })
        ));
        pic.apply_remote(&env(6)).expect("apply 6");
        assert_eq!(pic.last_applied_seq(), Some(6));
        pic.record_local(env(100));
        assert_eq!(pic.drain_local().len(), 1);
        assert!(pic.drain_local().is_empty());
    }
}

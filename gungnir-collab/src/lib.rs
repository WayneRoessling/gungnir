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

/// D-03's rule and the conflict vocabulary it reads, re-exported so this crate still names
/// them (`agentic-coding-standards.md` §1.2).
///
/// **The rule is written in `gungnir_model::arbitration`, not here**, because its other
/// caller -- the desktop's reconciliation, since the GAP-067 walk (2026-09-16) -- is a
/// crate whose manifest carries no edge to this one. One implementation both reach is what
/// keeps [`RoleRankArbiter`] and the reconciliation from ever disagreeing about who wins.
pub use gungnir_model::arbitration::{
    arbitrate, ArbitrationFacts, ArbitrationGround, ConflictSide, Resolution, SideOutcome, Verdict,
};

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
/// The expiry rule is a decision-authority rule (DN-10 amendment 1, §9).
///
/// The expiry rule comes first because it is not a tie-break at all. If one site's
/// window closed and another site's operator decided, there is nothing to arbitrate:
/// one of them chose. Ranking them would require an `AuthorityContext` for the expiry,
/// and whatever role were supplied for a person who did not act would decide the
/// outcome -- a fabricated authority beating a real one whenever the fabrication ranked
/// higher.
///
/// **The rule itself is [`arbitrate`]**, and this delegates to it, so the reconciliation
/// that applies the rule without this trait cannot drift from it. Every side here arrives
/// with an [`AuthorityContext`], so every rank is known and [`arbitrate`] always has what
/// it needs -- except for two mission times that cannot be ordered at all (a NaN), where
/// it declines and this keeps the second side, which is what the `<=` this used to hold
/// did there. An exact tie in rank and time keeps the first side, also as before.
#[derive(Debug, Default, Clone, Copy)]
pub struct RoleRankArbiter;

impl AuthorityArbiter for RoleRankArbiter {
    fn resolve(
        &self,
        first: (&DecisionRecord, AuthorityContext),
        second: (&DecisionRecord, AuthorityContext),
    ) -> Resolution {
        let facts = |(record, ctx): (&DecisionRecord, AuthorityContext)| ArbitrationFacts {
            expiry: record.is_expiry(),
            rank: Some(ctx.role.rank()),
            mission_time: record.mission_time,
        };
        arbitrate(facts(first), facts(second)).map_or(Resolution::KeepSecond, |v| v.keep)
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
            item: None,
            plan: PlanView::default(),
            verdict: PolicyVerdict::RequiresHumanApproval,
            decision,
            operator_id: None,
            role: None,
            request: None,
            origin: None,
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

    fn decision_at(t: f64, rank: Option<u8>) -> ArbitrationFacts {
        ArbitrationFacts {
            expiry: false,
            rank,
            mission_time: MissionTime(t),
        }
    }

    fn expiry_at(t: f64, rank: Option<u8>) -> ArbitrationFacts {
        ArbitrationFacts {
            expiry: true,
            rank,
            mission_time: MissionTime(t),
        }
    }

    fn kept(keep: Resolution, ground: ArbitrationGround) -> Verdict {
        Verdict { keep, ground }
    }

    /// DN-10 §9 over facts: a decision beats an expiry in either order and **without a
    /// rank on either side**, which is what lets a reconciliation settle an expiry against
    /// a decision whose role was never recorded. A rank on the expiry changes nothing.
    #[test]
    fn the_rule_keeps_a_decision_over_an_expiry_with_no_rank_known() {
        use ArbitrationGround::DecisionOverExpiry;
        assert_eq!(
            arbitrate(expiry_at(1.0, None), decision_at(2.0, None)),
            Some(kept(Resolution::KeepSecond, DecisionOverExpiry))
        );
        assert_eq!(
            arbitrate(decision_at(2.0, None), expiry_at(1.0, None)),
            Some(kept(Resolution::KeepFirst, DecisionOverExpiry))
        );
        let commander = Some(Role::Commander.rank());
        let operator = Some(Role::Operator.rank());
        assert_eq!(
            arbitrate(expiry_at(1.0, commander), decision_at(2.0, operator)),
            Some(kept(Resolution::KeepSecond, DecisionOverExpiry)),
            "the expiry's higher rank decided the outcome"
        );
    }

    /// D-03 over facts: the higher role wins in either order, and it wins even when it
    /// decided later -- time only breaks a tie.
    #[test]
    fn the_rule_keeps_the_higher_rank_whatever_the_order_or_the_times() {
        let supervisor = Some(Role::Supervisor.rank());
        let operator = Some(Role::Operator.rank());
        assert_eq!(
            arbitrate(decision_at(1.0, operator), decision_at(9.0, supervisor)),
            Some(kept(Resolution::KeepSecond, ArbitrationGround::HigherRole))
        );
        assert_eq!(
            arbitrate(decision_at(9.0, supervisor), decision_at(1.0, operator)),
            Some(kept(Resolution::KeepFirst, ArbitrationGround::HigherRole))
        );
    }

    /// D-03's tie-break over facts: on equal rank the earlier decision wins in either
    /// order, and an exact tie keeps the first side under its own ground, so the record
    /// never calls one of two simultaneous decisions the earlier.
    #[test]
    fn the_rule_keeps_the_earlier_decision_on_equal_rank() {
        use ArbitrationGround::{EarlierOnEqualRank, SameTimeOnEqualRank};
        let operator = Some(Role::Operator.rank());
        assert_eq!(
            arbitrate(decision_at(1.0, operator), decision_at(2.0, operator)),
            Some(kept(Resolution::KeepFirst, EarlierOnEqualRank))
        );
        assert_eq!(
            arbitrate(decision_at(2.0, operator), decision_at(1.0, operator)),
            Some(kept(Resolution::KeepSecond, EarlierOnEqualRank))
        );
        assert_eq!(
            arbitrate(decision_at(3.0, operator), decision_at(3.0, operator)),
            Some(kept(Resolution::KeepFirst, SameTimeOnEqualRank))
        );
    }

    /// The owner's condition from the GAP-067 walk: a conflict the rule cannot rank
    /// honestly gets **no** resolution, so it stays with a person. A missing rank on
    /// either side, or both, of two decisions -- or of two expiries -- is that case, and so
    /// are two times that cannot be ordered on equal rank.
    #[test]
    fn a_missing_rank_yields_no_resolution() {
        let supervisor = Some(Role::Supervisor.rank());
        assert_eq!(
            arbitrate(decision_at(1.0, None), decision_at(2.0, supervisor)),
            None
        );
        assert_eq!(
            arbitrate(decision_at(1.0, supervisor), decision_at(2.0, None)),
            None,
            "an unknown role was ranked as the lowest"
        );
        assert_eq!(
            arbitrate(decision_at(1.0, None), decision_at(2.0, None)),
            None
        );
        assert_eq!(
            arbitrate(expiry_at(1.0, None), expiry_at(2.0, supervisor)),
            None
        );
        assert_eq!(
            arbitrate(
                decision_at(f64::NAN, supervisor),
                decision_at(1.0, supervisor)
            ),
            None
        );
    }

    /// `RoleRankArbiter` decides exactly as it did before it delegated. The reference below
    /// is the body `resolve` held until 2026-09-16, kept verbatim, and the comparison runs
    /// over every pair of roles, both expiry flags on each side, and every ordering of the
    /// two times including one that cannot be ordered.
    #[test]
    fn the_arbiter_decides_exactly_as_it_did_before_it_delegated() {
        fn before(
            a: &DecisionRecord,
            ctx_a: AuthorityContext,
            b: &DecisionRecord,
            ctx_b: AuthorityContext,
        ) -> Resolution {
            match (a.is_expiry(), b.is_expiry()) {
                (true, false) => return Resolution::KeepSecond,
                (false, true) => return Resolution::KeepFirst,
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
        let side = |t: f64, expired: bool| {
            if expired {
                record(t, OperatorDecision::Expired { at: MissionTime(t) })
            } else {
                record(t, OperatorDecision::Accepted)
            }
        };
        let times = [(1.0, 2.0), (2.0, 1.0), (3.0, 3.0), (f64::NAN, 1.0)];
        let mut compared = 0;
        for role_a in Role::ALL {
            for role_b in Role::ALL {
                for (t_a, t_b) in times {
                    for (expired_a, expired_b) in
                        [(false, false), (false, true), (true, false), (true, true)]
                    {
                        let (a, b) = (side(t_a, expired_a), side(t_b, expired_b));
                        let (ctx_a, ctx_b) = (ctx(1, *role_a), ctx(2, *role_b));
                        assert_eq!(
                            RoleRankArbiter.resolve((&a, ctx_a), (&b, ctx_b)),
                            before(&a, ctx_a, &b, ctx_b),
                            "{role_a:?} at {t_a} (expired {expired_a}) against {role_b:?} \
                             at {t_b} (expired {expired_b})"
                        );
                        compared += 1;
                    }
                }
            }
        }
        assert_eq!(compared, Role::ALL.len() * Role::ALL.len() * 16);
    }
}

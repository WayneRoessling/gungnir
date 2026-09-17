// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The authority arbitration rule, over the facts it reads (D-03, DN-10 §9, GAP-050).
//!
//! D-03 locked the rule on 2026-09-04: the higher role wins a conflict, and the earlier
//! decision wins a tie. DN-10 §9 put a rule ahead of it: a real decision beats an expiry
//! whatever rank the expiry carries, because nobody chose. The owner's GAP-067 walk
//! (2026-09-16) made the rule resolve the conflicts a reconciliation finds, and kept for a
//! person every conflict it cannot rank honestly.
//!
//! # Why the rule lives here and not in `gungnir-collab`
//!
//! It has two callers. `gungnir_collab::RoleRankArbiter` delegates to [`arbitrate`], and
//! the desktop's reconciliation (`gungnir-app`'s `failover` module) applies it to every
//! conflict an outage leaves. `gungnir-app`'s manifest carries no edge to
//! `gungnir-collab` (the manifests are the graph, `ARCHITECTURE.md` §7.1), so a rule
//! written in collab could be reached by one of its two callers only, and reaching it from
//! the other would have meant a new dependency edge. The lowest crate both already reach is
//! this one, which is `agentic-coding-standards.md` §1.2 applied to a rule: shared, so it
//! moves down and is re-exported (`gungnir_collab` re-exports every item here), never
//! written twice. `rhythm` sits in this crate for the same no-new-edges reason.
//!
//! # What it reads, and what it will not guess
//!
//! One [`ArbitrationFacts`] per side: whether the side is an expiry, the rank of the role
//! the side was decided under **when that role is known**, and its mission time. This
//! crate cannot see `gungnir_security::Role`, so a rank arrives as the number
//! `Role::rank` gives, and a caller that does not know the role passes `None` rather than
//! a rank it chose. [`arbitrate`] answers `None` whenever it would need a rank it was not
//! given: an unknown authority is not a low one, and ranking it as one would let a guess
//! decide whose version of the record stands.

use crate::MissionTime;

/// Which of two conflicting sides stands.
///
/// Owned here since the rule moved down; `gungnir_collab::Resolution` is this type,
/// re-exported, so every path that named it still resolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    KeepFirst,
    KeepSecond,
}

/// What the rule reads from one side of a conflict, and nothing else.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArbitrationFacts {
    /// The side is an expiry: the window closed with nobody deciding (DN-10).
    pub expiry: bool,
    /// `gungnir_security::Role::rank` of the role this side was decided under, when that
    /// role is known. `None` for a decision whose role was never recorded -- one journaled
    /// before decisions carried a role, or taken with nobody signed in -- and for a role
    /// the reader could not parse.
    pub rank: Option<u8>,
    pub mission_time: MissionTime,
}

/// Why the rule kept the side it kept.
///
/// Journaled with the verdict (`LinkEvent::ConflictArbitrated`), because a verdict without
/// its ground cannot be argued with afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ArbitrationGround {
    /// One side decided and the other expired. Somebody chose beats nobody chose, whatever
    /// rank either side carries or lacks (DN-10 §9).
    DecisionOverExpiry,
    /// The kept side's role ranks higher (D-03).
    HigherRole,
    /// Equal rank, and the kept side decided earlier (D-03).
    EarlierOnEqualRank,
    /// Equal rank at the same mission time, so there is no earlier decision for D-03's
    /// tie-break to find.
    ///
    /// The rule keeps the **first** side then, as `RoleRankArbiter`'s `<=` always has. It
    /// is a ground of its own rather than [`ArbitrationGround::EarlierOnEqualRank`] so the
    /// record never claims one decision came before another when neither did; which side
    /// a caller passes first is therefore that caller's exact-tie policy, and each caller
    /// documents its order.
    SameTimeOnEqualRank,
}

/// The rule's answer: the side kept, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verdict {
    pub keep: Resolution,
    pub ground: ArbitrationGround,
}

/// A real decision beats an expiry; otherwise the higher role wins, and on equal rank the
/// earlier decision wins. `None` when the rule would need something it does not have.
///
/// The order of the checks is the design:
///
/// 1. **Expiry first**, and without consulting rank. If one side's window closed and the
///    other side's operator decided, there is nothing to rank: one of them chose. A
///    decision facing an expiry is therefore always resolvable, even when neither side
///    carries a known role.
/// 2. **Rank**, which needs both. A side with no known rank makes the conflict one this
///    rule cannot settle honestly, and it answers `None` so a person does -- rather than
///    treating the unknown role as the lowest, which would let a guess decide.
/// 3. **Mission time** on equal rank. An exact tie keeps the first side
///    ([`ArbitrationGround::SameTimeOnEqualRank`]); two mission times that cannot be
///    ordered at all (a NaN, which no JSON journal can hold) answer `None`.
///
/// Two expiries fall through to rank rather than being refused, which is what
/// `RoleRankArbiter` always did when it was handed an authority for each; a reconciliation
/// never asks, because two expiries agree and so never conflict.
#[must_use]
pub fn arbitrate(first: ArbitrationFacts, second: ArbitrationFacts) -> Option<Verdict> {
    let verdict = |keep, ground| Some(Verdict { keep, ground });
    match (first.expiry, second.expiry) {
        (true, false) => {
            return verdict(
                Resolution::KeepSecond,
                ArbitrationGround::DecisionOverExpiry,
            )
        }
        (false, true) => {
            return verdict(Resolution::KeepFirst, ArbitrationGround::DecisionOverExpiry)
        }
        // Two expiries, or two decisions: rank decides.
        (true, true) | (false, false) => {}
    }
    let (Some(a), Some(b)) = (first.rank, second.rank) else {
        return None;
    };
    match a.cmp(&b) {
        std::cmp::Ordering::Greater => {
            verdict(Resolution::KeepFirst, ArbitrationGround::HigherRole)
        }
        std::cmp::Ordering::Less => verdict(Resolution::KeepSecond, ArbitrationGround::HigherRole),
        std::cmp::Ordering::Equal => match first.mission_time.partial_cmp(&second.mission_time) {
            Some(std::cmp::Ordering::Less) => {
                verdict(Resolution::KeepFirst, ArbitrationGround::EarlierOnEqualRank)
            }
            Some(std::cmp::Ordering::Greater) => verdict(
                Resolution::KeepSecond,
                ArbitrationGround::EarlierOnEqualRank,
            ),
            Some(std::cmp::Ordering::Equal) => verdict(
                Resolution::KeepFirst,
                ArbitrationGround::SameTimeOnEqualRank,
            ),
            None => None,
        },
    }
}

/// How one side of a conflict ended a plan's approval, as far as a journal can tell.
///
/// `CommandEvent::Decided` carries `accepted` and not which of accept and override it was,
/// so an override reads as [`SideOutcome::Accepted`] here: both are actionable, and that
/// is all the record says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SideOutcome {
    Accepted,
    Rejected,
    /// The window closed with nobody deciding. Not a rejection (DN-10).
    Expired,
}

/// One side of a conflicting decision, as its journal recorded it (GAP-050).
///
/// Shared by `gungnir_resilience::DecisionConflict`, which reports the conflict, and
/// `LinkEvent::ConflictArbitrated`, which records how the rule settled it, so the verdict
/// carries exactly the facts the report held rather than a second description of them.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConflictSide {
    pub outcome: SideOutcome,
    /// Who decided, as the decision named them. `None` for an expiry, where nobody did,
    /// and for a decision taken with nobody signed in.
    pub operator: Option<String>,
    /// The role the deciding session carried, as `CommandEvent::Decided::role` recorded
    /// it. `None` for an expiry, and wherever no role was recorded.
    pub role: Option<String>,
    /// When: the decision's envelope time, or the expiry's own `at`.
    pub at: MissionTime,
}

impl ConflictSide {
    /// The facts [`arbitrate`] reads from this side, with the rank of its recorded role
    /// as the caller resolved it.
    ///
    /// The rank is the caller's because only a crate that can see
    /// `gungnir_security::Role` can turn the recorded name into one; a name that does not
    /// parse is an unknown role and arrives here as `None`.
    #[must_use]
    pub fn facts(&self, rank: Option<u8>) -> ArbitrationFacts {
        ArbitrationFacts {
            expiry: self.outcome == SideOutcome::Expired,
            rank,
            mission_time: self.at,
        }
    }
}

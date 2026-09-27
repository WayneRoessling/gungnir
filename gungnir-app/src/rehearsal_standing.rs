// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Whether the laydown in force was rehearsed under what is running, and what a decision
//! on a plan is told about it (GAP-107, `docs/design/DN-26-laydown-options.md` §11).
//!
//! # Advisory, acknowledged, never a gate (D-119)
//!
//! The owner decided that nothing refuses a plan because the laydown under it was never
//! rehearsed. PN-16 says where the laydown in force stands, and PN-07 says it too, in its
//! own section; while the laydown was never rehearsed, was rehearsed under something other
//! than what is running, or cannot be rehearsed under this baseline, accepting or
//! overriding a plan asks the person to tick that they have read it. The sentence they
//! ticked goes onto the decision record, onto `CommandEvent::Decided` and into the
//! decision's audit entry ([`crate::decisions::decide`]).
//!
//! # On the record, not in the session (D-120)
//!
//! A rehearsal journals a `PlanningEvent::LaydownRehearsed` with its stamp, and the desktop
//! folds the stamps from its journal at start ([`crate::planning_record::recover`]), so a
//! restart does not turn a rehearsed laydown into one "never rehearsed". The figures PN-16
//! shows are still the run's own, for the session that ran it.
//!
//! # "Under what is running" (D-119)
//!
//! A stamp carries the five digests [`crate::laydown_rehearsal::basis_of`] takes of what
//! the run took from the deployment. A rehearsal of the laydown in force stands when any
//! stamp of it on the record has the basis the running baseline gives now. A new baseline
//! revision that changed nothing a rehearsal takes does not stale one: the revision is
//! shown, and nothing is asked.

use crate::laydown_rehearsal::basis_of;
use crate::state::AppState;
use gungnir_model::{Acknowledgement, LaydownId, RehearsalStamp, SessionId};

/// What [`Acknowledgement::subject`] says for this module's statement.
pub const SUBJECT: &str = "rehearsal";

/// One stamp on the record, and the session it is in (`None`: not known, which a stamp
/// read from the journal never is).
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedStamp {
    pub stamp: RehearsalStamp,
    pub session: Option<SessionId>,
}

/// Every rehearsal on this desktop's record, oldest first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rehearsals {
    pub stamps: Vec<RecordedStamp>,
}

impl Rehearsals {
    /// Stamps of `laydown`, newest first.
    fn of<'a>(&'a self, laydown: &'a LaydownId) -> impl Iterator<Item = &'a RecordedStamp> {
        self.stamps
            .iter()
            .rev()
            .filter(move |r| &r.stamp.laydown == laydown)
    }

    /// The sessions retention must keep for these (`crate::retention::protected`): each
    /// laydown's latest stamp. Older stamps of a laydown are not needed to say it was
    /// rehearsed, and a stamp that no longer matches anything running is only history.
    #[must_use]
    pub fn sessions_to_keep(&self) -> Vec<SessionId> {
        let mut seen: Vec<&LaydownId> = Vec::new();
        let mut keep = Vec::new();
        for r in self.stamps.iter().rev() {
            if seen.contains(&&r.stamp.laydown) {
                continue;
            }
            seen.push(&r.stamp.laydown);
            keep.extend(r.session);
        }
        keep
    }
}

/// Where the laydown in force stands (DN-26 §11 item 5).
#[derive(Debug, Clone, PartialEq)]
pub enum InForce {
    /// The deployment declares no laydown, so there is nothing to rehearse and nothing is
    /// said or asked (DN-26 §8: empty means none offered).
    NoLaydownDeclared,
    /// No rehearsal of it is on the record.
    NeverRehearsed { laydown: LaydownId },
    /// A rehearsal of it could not run under this baseline, and why.
    CannotBeRehearsed { laydown: LaydownId, reason: String },
    /// Rehearsed, but no rehearsal on the record ran under what is running: `latest` is
    /// the newest, and `parts` the parts of it that differ.
    RehearsedUnderOther {
        laydown: LaydownId,
        latest: RecordedStamp,
        parts: Vec<&'static str>,
        revision_now: u32,
    },
    /// Rehearsed under what is running: `stamp` is the newest such rehearsal.
    Stands {
        laydown: LaydownId,
        stamp: RecordedStamp,
        revision_now: u32,
    },
}

/// When a stamp was run, in words: this session's mission time, or that it was in an
/// earlier one (a mission time from another session means nothing on this clock).
fn when(stamp: &RecordedStamp, this_session: Option<SessionId>) -> String {
    if stamp.session.is_some() && stamp.session == this_session {
        format!("at T+{:.0} s this session", stamp.stamp.ran_at.0)
    } else {
        match stamp.session {
            Some(s) => format!("in session {}", s.0),
            None => "in an earlier session".to_owned(),
        }
    }
}

impl InForce {
    /// The sentence PN-16 and PN-07 draw, and the statement an acknowledgement records.
    ///
    /// `this_session` is the desktop's session, so a rehearsal run now reads as "this
    /// session" and one from before a restart names the session it was in.
    #[must_use]
    pub fn sentence(&self, this_session: Option<SessionId>) -> Option<String> {
        match self {
            InForce::NoLaydownDeclared => None,
            InForce::NeverRehearsed { laydown } => Some(format!(
                "The laydown in force, {laydown}, has never been rehearsed: no rehearsal of \
                 it is on this desktop's record."
            )),
            InForce::CannotBeRehearsed { laydown, reason } => Some(format!(
                "The laydown in force, {laydown}, cannot be rehearsed under this baseline: \
                 {reason}."
            )),
            InForce::RehearsedUnderOther {
                laydown,
                latest,
                parts,
                revision_now,
            } => Some(format!(
                "The laydown in force, {laydown}, was last rehearsed against {} {} under \
                 baseline revision {}, and the {} it ran under are not what is running now \
                 (revision {revision_now}).",
                latest.stamp.scenario.label(),
                when(latest, this_session),
                latest.stamp.revision,
                parts.join(", "),
            )),
            InForce::Stands {
                laydown,
                stamp,
                revision_now,
            } => Some(if stamp.stamp.revision == *revision_now {
                format!(
                    "The laydown in force, {laydown}, was rehearsed against {} {} under what \
                     is running now (baseline revision {revision_now}).",
                    stamp.stamp.scenario.label(),
                    when(stamp, this_session),
                )
            } else {
                format!(
                    "The laydown in force, {laydown}, was rehearsed against {} {} under \
                     baseline revision {}; revision {revision_now} changed nothing a \
                     rehearsal runs under.",
                    stamp.stamp.scenario.label(),
                    when(stamp, this_session),
                    stamp.stamp.revision,
                )
            }),
        }
    }

    /// Whether a decision acting on a plan must acknowledge this standing (DN-26 §11
    /// item 6): never rehearsed, rehearsed under something else, or cannot be rehearsed.
    #[must_use]
    pub fn asks(&self) -> bool {
        matches!(
            self,
            InForce::NeverRehearsed { .. }
                | InForce::CannotBeRehearsed { .. }
                | InForce::RehearsedUnderOther { .. }
        )
    }
}

/// Where the laydown in force stands, as of the running baseline and the record.
#[must_use]
pub fn in_force(state: &AppState) -> InForce {
    let Some(current) = state.config.laydowns.iter().find(|l| l.current) else {
        return InForce::NoLaydownDeclared;
    };
    let laydown = current.id.clone();
    let basis = match basis_of(&state.config, current) {
        Ok(basis) => basis,
        Err(err) => {
            return InForce::CannotBeRehearsed {
                laydown,
                reason: err.to_string(),
            }
        }
    };
    let revision_now = state.config.revision;
    let stamps: Vec<&RecordedStamp> = state.rehearsals.of(&laydown).collect();
    let Some(latest) = stamps.first().map(|r| (*r).clone()) else {
        return InForce::NeverRehearsed { laydown };
    };
    match stamps.into_iter().find(|r| r.stamp.basis == basis) {
        Some(stamp) => InForce::Stands {
            stamp: stamp.clone(),
            laydown,
            revision_now,
        },
        None => InForce::RehearsedUnderOther {
            parts: latest.stamp.basis.differs_in(&basis),
            laydown,
            latest,
            revision_now,
        },
    }
}

/// What a decision acting on a plan must acknowledge now, if anything (DN-26 §11 item 6).
#[must_use]
pub fn advisory(state: &AppState) -> Option<Acknowledgement> {
    let standing = in_force(state);
    if !standing.asks() {
        return None;
    }
    standing
        .sentence(state.session())
        .map(|statement| Acknowledgement {
            subject: SUBJECT.to_owned(),
            statement,
        })
}

/// Record a rehearsal this desktop just ran: journaled, and on the ledger for this
/// session (D-120).
pub fn record(state: &mut AppState, stamp: RehearsalStamp) {
    let now = state.clock.now();
    if let Err(err) = state.events.publish(
        now,
        gungnir_eventing::Event::Planning(gungnir_model::events::PlanningEvent::LaydownRehearsed(
            stamp.clone(),
        )),
    ) {
        tracing::error!(%err, "the rehearsal could not be published to the journal");
        state.alerts.push(format!(
            "The rehearsal of {} is not on the record ({err}); a later session will not \
             know it was run",
            stamp.laydown
        ));
    }
    let session = state.session();
    state
        .rehearsals
        .stamps
        .push(RecordedStamp { stamp, session });
}

// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The planning record, folded from the journal at start (GAP-106, GAP-107): the coverage
//! gap acceptances that still stand, and every laydown rehearsal on the record
//! (`docs/design/DN-33-accepting-a-coverage-gap.md` §8 rule 6,
//! `docs/design/DN-26-laydown-options.md` §11 item 2).
//!
//! One pass over the sessions for both, the way requirements and launch warnings are
//! recovered, and for the same reason: an acceptance made on Monday and a rehearsal run on
//! Monday are still on the record on Tuesday.

use crate::gap_acceptance::GapAcceptances;
use crate::rehearsal_standing::{RecordedStamp, Rehearsals};
use gungnir_eventing::Event;
use gungnir_model::events::PlanningEvent;

/// What recovery found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlanningRecord {
    pub acceptances: GapAcceptances,
    pub rehearsals: Rehearsals,
    /// Set when the journal could not be read whole, with why: what was recovered is then
    /// incomplete, and the desktop says so rather than showing a short list as the whole.
    pub unreadable: Option<String>,
}

/// Fold every session of `journal`, oldest first.
///
/// An acceptance stands when its `GapAccepted` is followed by no `GapAcceptanceReopened`;
/// whether it still holds against the running baseline is the first
/// [`crate::gap_acceptance::reconcile`]'s question, not this one's. The serial continues
/// past the highest identifier seen, re-opened or not.
#[must_use]
pub fn recover(journal: &dyn gungnir_store::EventJournal) -> PlanningRecord {
    let mut record = PlanningRecord::default();
    let mut sessions = match journal.sessions() {
        Ok(sessions) => sessions,
        Err(err) => {
            record.unreadable = Some(err.to_string());
            return record;
        }
    };
    sessions.sort_unstable_by_key(|s| s.0);
    for session in sessions {
        let envelopes = match journal.read_session(session) {
            Ok(envelopes) => envelopes,
            Err(err) => {
                record.unreadable = Some(format!("session {}: {err}", session.0));
                return record;
            }
        };
        for envelope in envelopes {
            let Event::Planning(event) = envelope.event else {
                continue;
            };
            match event {
                PlanningEvent::GapAccepted(acceptance) => {
                    if acceptance.id.0 >= record.acceptances.highest {
                        record.acceptances.highest = acceptance.id.0;
                        record.acceptances.highest_session = Some(session);
                    }
                    record
                        .acceptances
                        .standing
                        .push((acceptance, Some(session)));
                }
                PlanningEvent::GapAcceptanceReopened { acceptance, .. } => record
                    .acceptances
                    .standing
                    .retain(|(a, _)| a.id != acceptance),
                PlanningEvent::LaydownRehearsed(stamp) => {
                    record.rehearsals.stamps.push(RecordedStamp {
                        stamp,
                        session: Some(session),
                    });
                }
            }
        }
    }
    record
}

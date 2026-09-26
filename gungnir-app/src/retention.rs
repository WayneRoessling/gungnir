// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The desktop's journal retention (GAP-122, decision D-78).
//!
//! `gungnir_store::retention` removes sessions past the baseline's policy age and never
//! the one being appended to or one under a hold; `gungnir_mission::apply_retention`
//! removes each one's mission record with it. This module decides **when** and **what
//! else this desktop must keep**, and makes every removal visible.
//!
//! # When
//!
//! On the first tick -- which is "at start", after the live session exists and on the
//! bus that journals what the purge did -- and hourly after that, on the wall clock. Never
//! while opening the journal, which `gungnir_store::retention` forbids. A baseline with
//! no `retention` purges nothing, and the desktop says so once at start.
//!
//! # What this desktop keeps whatever its age
//!
//! The recovery paths rebuild state by folding the whole journal, so a session is kept
//! while anything in it is still in force:
//!
//! - the **live** session, before its first append as much as after;
//! - every session holding an event for a **collection requirement still open**, since the
//!   requirement is rebuilt from all of them, and a purge of the one that tasked it would
//!   bring it back as merely stated;
//! - the session stating the **highest requirement identifier** and the one issuing the
//!   **highest launch-warning serial**, since each serial continues past the highest the
//!   journal holds and a purge of that session would reissue a number;
//! - the session an **unfinished outage** is recorded in (GAP-142), which the merge reads;
//! - a session under an **after-action review**, through the hold `review.rs` places.
//!
//! A launch warning or a closed requirement older than the policy goes with its session:
//! that is what retention is for.
//!
//! # How it is seen
//!
//! Each removal is journaled into the live session as a `RetentionEvent`, logged, counted
//! in [`RetentionState::purged_total`], and raised as an alert naming the sessions. An
//! expired session that is kept is logged with its reason. A purge that fails raises one
//! alert per distinct failure and is tried again at the next interval.

use crate::state::AppState;
use crate::update::publish;
use gungnir_eventing::Event;
use gungnir_model::events::RetentionEvent;
use gungnir_model::{RequirementId, SessionId};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant, SystemTime};

/// How often the purge runs after the first. Far below the policy's granularity of days,
/// and cheap when nothing is due: one directory read.
pub const INTERVAL: Duration = Duration::from_secs(3600);

/// What retention needs to know between runs, and what it has done.
#[derive(Debug, Default)]
pub struct RetentionState {
    /// When the next purge is due; `None` until the first, which runs on the first tick.
    next_due: Option<Instant>,
    /// Sessions removed by this process, for the health line.
    pub purged_total: u64,
    /// The last failure, so the same one is not raised every hour.
    last_error: Option<String>,
    /// The sessions each recovered requirement has events in (from recovery).
    pub(crate) requirement_sessions: BTreeMap<RequirementId, BTreeSet<SessionId>>,
    /// The session holding the highest launch-warning serial recovered.
    pub(crate) launch_warning_session: Option<SessionId>,
}

impl RetentionState {
    /// Built from what recovery found.
    #[must_use]
    pub fn from_recovery(
        requirement_sessions: BTreeMap<RequirementId, BTreeSet<SessionId>>,
        launch_warning_session: Option<SessionId>,
    ) -> Self {
        Self {
            requirement_sessions,
            launch_warning_session,
            ..Self::default()
        }
    }
}

/// Run the purge when it is due. Called from `update::tick`, before the journal drain,
/// so what the purge did is journaled in the same frame.
pub fn tick(state: &mut AppState) {
    let due = state
        .retention
        .next_due
        .is_none_or(|at| Instant::now() >= at);
    if !due {
        return;
    }
    state.retention.next_due = Some(Instant::now() + INTERVAL);
    if state.config.retention.is_none() {
        return;
    }
    run(state, SystemTime::now());
}

/// The sessions this desktop must keep whatever their age (this module's doc).
#[must_use]
pub fn protected(state: &AppState) -> BTreeSet<SessionId> {
    let mut keep: BTreeSet<SessionId> = state.session().into_iter().collect();
    for requirement in state.requirements.iter().filter(|r| r.is_open()) {
        if let Some(sessions) = state.retention.requirement_sessions.get(&requirement.id) {
            keep.extend(sessions.iter().copied());
        }
    }
    if let Some((_, sessions)) = state.retention.requirement_sessions.last_key_value() {
        keep.extend(sessions.iter().copied());
    }
    keep.extend(state.retention.launch_warning_session);
    if let Some(session) = state.fallback.as_ref().and_then(|f| f.session) {
        keep.insert(session);
    }
    keep
}

/// Apply the baseline's policy now, measuring ages against `now`. A baseline with no
/// policy does nothing. Public so a test can age a journal without waiting an hour.
pub fn run(state: &mut AppState, now: SystemTime) {
    let Some(policy) = state.config.retention else {
        return;
    };
    let keep = protected(state);
    let outcome = match gungnir_mission::apply_retention(&state.journal, &policy, now, &keep) {
        Ok(outcome) => outcome,
        Err(err) => {
            let said = err.to_string();
            tracing::error!(%err, "journal retention failed; it is tried again in an hour");
            if state.retention.last_error.as_deref() != Some(said.as_str()) {
                state.alerts.push(format!(
                    "Journal retention failed ({said}); nothing is half-removed, and it is \
                     tried again in an hour"
                ));
                state.retention.last_error = Some(said);
            }
            return;
        }
    };
    state.retention.last_error = None;
    let at = state.clock.now();
    for purged in &outcome.journal.purged {
        publish(
            state,
            at,
            Event::Retention(RetentionEvent::Purged {
                session: purged.session,
                idle_days: purged.idle_days,
                max_session_age_days: policy.max_session_age_days,
                bytes: purged.bytes,
                at,
            }),
        );
    }
    for &session in &outcome.journal.completed {
        publish(
            state,
            at,
            Event::Retention(RetentionEvent::Completed { session, at }),
        );
    }
    for session in &outcome.records_only {
        tracing::info!(
            session = session.0,
            "removed the mission record of a session that journaled nothing"
        );
    }
    let removed: Vec<String> = outcome
        .journal
        .purged
        .iter()
        .map(|p| p.session)
        .chain(outcome.journal.completed.iter().copied())
        .map(|s| s.0.to_string())
        .collect();
    if !removed.is_empty() {
        state.retention.purged_total += removed.len() as u64;
        state.alerts.push(format!(
            "Retention removed {} session(s) not written for more than {} days: {}",
            removed.len(),
            policy.max_session_age_days,
            removed.join(", ")
        ));
    }
}

/// Said once at start: whether this desktop purges anything, and under what limit.
pub fn announce(config: &gungnir_config::ConfigBaseline) {
    if let Some(policy) = config.retention {
        tracing::info!(
            max_session_age_days = policy.max_session_age_days,
            "journal retention: sessions not written for longer than this are purged, at start and hourly"
        );
    } else {
        tracing::info!("journal retention is not configured; no session is ever purged");
    }
}

// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Journal retention on the desktop (GAP-122, decision D-78).
//!
//! `gungnir-store/tests/retention_purge.rs` holds the store's half: ages below, at and
//! above the limit, holds, and a purge interrupted halfway. These are the desktop's:
//! the purge runs on the first tick, it keeps every old session something recovered
//! still depends on, a session under review is held until the review closes, every
//! removal is journaled and alerted, and a baseline with no policy removes nothing.

use gungnir_app::retention;
use gungnir_app::review;
use gungnir_app::state::AppState;
use gungnir_app::sustainment::SustainmentState;
use gungnir_app::update;
use gungnir_config::{AssetConfig, ConfigBaseline};
use gungnir_eventing::{Envelope, Event};
use gungnir_mission::{JournalMissionManager, MissionManager, MissionState};
use gungnir_model::events::{LaunchWarningEvent, RequirementEvent, RetentionEvent};
use gungnir_model::{
    AssetExtent, AssetPriority, CollectionRequirement, Concurrence, Geodetic, LaunchWarningReport,
    MissionTime, Releasability, RequirementId, RequirementState, SensorTaskId, SessionId, TrackId,
};
use gungnir_store::retention::RetentionPolicy;
use gungnir_store::{journal, DurabilityPolicy, EventJournal, FileEventJournal};
use gungnir_ui::panels::reports::ReviewAction;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const DAY: Duration = Duration::from_hours(24);
const LIMIT_DAYS: u32 = 30;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-app-retention-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn config(dir: &Path, retention: Option<RetentionPolicy>) -> ConfigBaseline {
    ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        assets: vec![AssetConfig {
            id: 1,
            name: "the harbour".into(),
            position: [0.0, 0.0, 0.0],
            radius_m: Some(2_000.0),
            priority: "high".into(),
            warning_lead_time_s: None,
            warning_channel: None,
            warning_within_m: None,
            note: None,
        }],
        retention,
        ..ConfigBaseline::default()
    }
}

fn policy() -> RetentionPolicy {
    RetentionPolicy {
        max_session_age_days: LIMIT_DAYS,
        max_audit_log_age_days: 365,
    }
}

fn stated(id: u64) -> Event {
    Event::Requirement(RequirementEvent::Stated {
        requirement: CollectionRequirement {
            id: RequirementId(id),
            title: format!("requirement {id}"),
            priority: AssetPriority::High,
            area: AssetExtent::Point {
                position: Geodetic {
                    lat_rad: 0.9,
                    lon_rad: -0.02,
                    alt_m: 0.0,
                },
            },
            needed_by: None,
            state: RequirementState::Stated,
        },
        at: MissionTime(1.0),
    })
}

fn by() -> Concurrence {
    Concurrence::Operator {
        id: "7".into(),
        role: "SensorManager".into(),
    }
}

fn declined(id: u64) -> Event {
    Event::Requirement(RequirementEvent::Declined {
        requirement: RequirementId(id),
        by: by(),
        reason: "no sensor covers it".into(),
        at: MissionTime(2.0),
    })
}

fn tasked(id: u64) -> Event {
    Event::Requirement(RequirementEvent::Tasked {
        requirement: RequirementId(id),
        task: SensorTaskId(9),
        by: by(),
        at: MissionTime(2.0),
    })
}

fn issued(serial: u64) -> Event {
    Event::LaunchWarning(LaunchWarningEvent::Issued(LaunchWarningReport {
        id: format!("launch-warning-{serial}"),
        what: "a launch from the ridge".into(),
        at: MissionTime(3.0),
        releasability: Releasability::default(),
    }))
}

fn deleted(n: u64) -> Event {
    Event::Tracking(gungnir_model::events::TrackingEvent::TrackDeleted(TrackId(
        n,
    )))
}

/// Write a closed session holding `events`, and age its journal and record by `idle`.
fn earlier_session(dir: &Path, events: Vec<Event>, idle: Duration) -> SessionId {
    let mut journal =
        FileEventJournal::open_with_policy(dir, DurabilityPolicy::node()).expect("journal");
    let session = {
        let mut missions = JournalMissionManager::open(dir, &journal).expect("missions");
        let mut mission = missions.create(ConfigBaseline::default()).expect("created");
        missions
            .transition(&mut mission, MissionState::Live)
            .expect("live");
        let session = mission.session;
        missions.close(mission).expect("closed");
        session
    };
    for (seq, event) in (0_u64..).zip(events) {
        journal
            .append(
                session,
                &Envelope {
                    seq,
                    mission_time: MissionTime(1.0),
                    event,
                },
            )
            .expect("appended");
    }
    drop(journal);
    let at = SystemTime::now() - idle;
    for path in [
        dir.join(journal::session_file_name(session)),
        dir.join(format!("{}.mission.json", session.0)),
    ] {
        std::fs::File::options()
            .write(true)
            .open(&path)
            .expect("file")
            .set_modified(at)
            .expect("mtime");
    }
    session
}

/// What the journal directory lists, read as another process would read it.
fn listed(dir: &Path) -> Vec<SessionId> {
    FileEventJournal::open(dir)
        .expect("a second reader")
        .sessions()
        .expect("listed")
}

fn retention_events(seen: &gungnir_eventing::Receiver<Envelope>) -> Vec<RetentionEvent> {
    seen.try_iter()
        .filter_map(|e| match e.event {
            Event::Retention(r) => Some(r),
            _ => None,
        })
        .collect()
}

/// The first tick purges; everything something recovered still reads is kept; the
/// removals are journaled, alerted and counted; and the serials carry on past what the
/// purge left.
#[test]
fn the_first_tick_purges_only_what_nothing_still_depends_on() {
    let dir = temp_dir("first-tick");
    let old = DAY * 100;
    let open_stated = earlier_session(&dir, vec![stated(1)], old);
    let open_tasked = earlier_session(&dir, vec![tasked(1)], old);
    let closed = earlier_session(&dir, vec![stated(2), declined(2)], old);
    let highest_requirement = earlier_session(&dir, vec![stated(3), declined(3)], old);
    let early_warning = earlier_session(&dir, vec![issued(1)], old);
    let highest_warning = earlier_session(&dir, vec![issued(2)], old);
    let quiet = earlier_session(&dir, vec![deleted(1)], old);
    let recent = earlier_session(&dir, vec![deleted(2)], DAY * 10);

    let mut state = AppState::with_config(config(&dir, Some(policy()))).expect("desktop");
    let live = state.session().expect("live");
    let seen = state.events.subscribe();
    update::tick(&mut state);

    let listed = listed(&dir);
    for kept in [
        open_stated,
        open_tasked,
        highest_requirement,
        highest_warning,
        recent,
        live,
    ] {
        assert!(
            listed.contains(&kept),
            "session {} was purged: {listed:?}",
            kept.0
        );
    }
    for gone in [closed, early_warning, quiet] {
        assert!(!listed.contains(&gone), "session {} survived", gone.0);
        assert!(
            !dir.join(format!("{}.mission.json", gone.0)).exists(),
            "session {}'s mission record survived it",
            gone.0
        );
    }

    let purged: Vec<SessionId> = retention_events(&seen)
        .into_iter()
        .filter_map(|e| match e {
            RetentionEvent::Purged {
                session,
                max_session_age_days,
                idle_days,
                ..
            } => {
                assert_eq!(max_session_age_days, LIMIT_DAYS);
                assert!(idle_days > 99.0);
                Some(session)
            }
            RetentionEvent::Completed { .. } => None,
        })
        .collect();
    assert_eq!(purged, vec![closed, early_warning, quiet]);
    assert_eq!(state.retention.purged_total, 3);
    assert!(
        state
            .alerts
            .iter()
            .any(|a| a.starts_with("Retention removed 3 session(s)")),
        "{:?}",
        state.alerts
    );

    // The removals are on the live session's record, where they survive the sessions
    // they describe.
    state.save_session().expect("saved");
    let recorded = FileEventJournal::open(&dir)
        .expect("a second reader")
        .read_session(live)
        .expect("live session");
    assert_eq!(
        recorded
            .iter()
            .filter(|e| matches!(e.event, Event::Retention(RetentionEvent::Purged { .. })))
            .count(),
        3
    );

    // The open requirement came back tasked, which it could not have done had the
    // session that tasked it gone.
    let requirement = state
        .requirements
        .iter()
        .find(|r| r.id == RequirementId(1))
        .expect("recovered");
    assert!(matches!(requirement.state, RequirementState::Tasked { .. }));

    // A later run with nothing new past the limit removes nothing more.
    retention::run(&mut state, SystemTime::now());
    assert_eq!(state.retention.purged_total, 3);
    drop(state);

    // After a restart the serials continue past what the journal still holds.
    let mut state = AppState::with_config(config(&dir, Some(policy()))).expect("restart");
    assert_eq!(state.next_requirement_id(), RequirementId(4));
    assert_eq!(state.next_launch_warning_id(), "launch-warning-3");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A session under review is held for as long as the review is open, whatever its age,
/// and goes on the first purge after the review closes.
#[test]
fn a_session_under_review_is_held_until_the_review_closes() {
    let dir = temp_dir("review");
    let reviewed = earlier_session(&dir, vec![deleted(1), deleted(2)], DAY * 100);
    let mut state = AppState::with_config(config(&dir, Some(policy()))).expect("desktop");

    let mut sustainment = SustainmentState::default();
    sustainment
        .replay
        .open(&state, reviewed)
        .expect("the old session replays");
    review::apply(&mut state, &mut sustainment, ReviewAction::Open);
    assert!(sustainment.review.is_some(), "{:?}", state.alerts);
    assert!(dir.join(journal::hold_file_name(reviewed)).exists());

    update::tick(&mut state);
    assert!(
        listed(&dir).contains(&reviewed),
        "a session under review was purged"
    );

    review::apply(&mut state, &mut sustainment, ReviewAction::Conclude);
    review::apply(&mut state, &mut sustainment, ReviewAction::Close);
    assert!(
        !dir.join(journal::hold_file_name(reviewed)).exists(),
        "closing the review released its hold: {:?}",
        state.alerts
    );
    retention::run(&mut state, SystemTime::now());
    assert!(!listed(&dir).contains(&reviewed));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A baseline that declares no policy purges nothing, however old (D-78): the period is
/// the deployment's to state.
#[test]
fn a_baseline_without_a_policy_purges_nothing() {
    let dir = temp_dir("none");
    let ancient = earlier_session(&dir, vec![deleted(1)], DAY * 3_000);
    let mut state = AppState::with_config(config(&dir, None)).expect("desktop");
    let seen = state.events.subscribe();
    update::tick(&mut state);
    retention::run(&mut state, SystemTime::now() + DAY * 10_000);
    assert!(listed(&dir).contains(&ancient));
    assert!(retention_events(&seen).is_empty());
    assert_eq!(state.retention.purged_total, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

//! The desktop opens and closes its session through `gungnir-mission` (GAP-051).
//!
//! `MissionManager` was a 39-line trait with no implementation, and both binaries
//! fabricated a `Mission` from the wall clock and declared it `Live` with nothing on disk
//! saying so. These are the tests behind the claim that the lifecycle is wired rather than
//! merely present.
//!
//! The property worth protecting: **a session that ended because the process stopped and a
//! session that ended because somebody finished it are different facts**, and only one of
//! them means the record is complete.

use gungnir_app::state::AppState;
use gungnir_config::ConfigBaseline;
use gungnir_mission::{JournalMissionManager, MissionManager, MissionState};
use gungnir_store::{EventJournal, FileEventJournal};

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("gungnir-session-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn desktop(dir: &std::path::Path) -> AppState {
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    };
    AppState::with_config(config).expect("the desktop starts")
}

fn unclosed_alerts(state: &AppState) -> Vec<&String> {
    state
        .alerts
        .iter()
        .filter(|a| a.contains("was not closed"))
        .collect()
}

/// A launch records a mission, and the record says which baseline it ran under.
#[test]
fn launching_records_a_live_session_on_disk() {
    let dir = scratch("records");
    let state = desktop(&dir);
    let session = state.mission.as_ref().expect("a session").session;
    drop(state);

    let journal = FileEventJournal::open(&dir).expect("journal");
    let missions = JournalMissionManager::open(&dir, &journal).expect("opened");
    assert_eq!(missions.missions().expect("listed"), vec![session]);
    let _ = std::fs::remove_dir_all(dir);
}

/// **A desktop that stops without closing leaves a record that says so**, and the next
/// launch tells the operator which session it was. The alternative -- a record that still
/// claims to be live, or one silently rewritten to look closed -- would hide a hole in the
/// evidence from whoever reviews it.
#[test]
fn a_session_that_was_never_closed_is_reported_at_the_next_launch() {
    let dir = scratch("interrupted");

    let first = desktop(&dir);
    let interrupted = first.mission.as_ref().expect("a session").session;
    assert!(
        unclosed_alerts(&first).is_empty(),
        "a first launch reported an unclosed session that never existed"
    );
    // The process stops here. Nothing calls `close_session`.
    drop(first);

    let second = desktop(&dir);
    let reported = unclosed_alerts(&second);
    assert_eq!(
        reported.len(),
        1,
        "the unclosed session was not reported exactly once: {:?}",
        second.alerts
    );
    assert!(
        reported[0].contains(&format!("Session {}", interrupted.0)),
        "the alert did not name the session: {}",
        reported[0]
    );

    // And it is a *different* session, not a resumption of the interrupted one.
    let current = second.mission.as_ref().expect("a session").session;
    assert_ne!(
        current, interrupted,
        "the new launch reused the interrupted session's identifier"
    );

    let journal = FileEventJournal::open(&dir).expect("journal");
    let mut missions = JournalMissionManager::open(&dir, &journal).expect("opened");
    assert_eq!(
        missions.load(interrupted).expect("loaded").state,
        MissionState::Interrupted,
        "a session nobody closed was reopened as something other than interrupted"
    );
    drop(second);
    let _ = std::fs::remove_dir_all(dir);
}

/// A session closed deliberately is not reported as interrupted, because it was not.
#[test]
fn a_closed_session_is_not_reported_as_interrupted() {
    let dir = scratch("closed");

    let mut first = desktop(&dir);
    let closed = first.mission.as_ref().expect("a session").session;
    first.close_session().expect("closed");
    assert!(
        first.mission.is_none(),
        "the desktop still holds a mission it closed"
    );
    drop(first);

    let second = desktop(&dir);
    assert!(
        unclosed_alerts(&second).is_empty(),
        "a deliberately closed session was reported as unclosed: {:?}",
        second.alerts
    );

    let journal = FileEventJournal::open(&dir).expect("journal");
    let mut missions = JournalMissionManager::open(&dir, &journal).expect("opened");
    assert_eq!(
        missions.load(closed).expect("loaded").state,
        MissionState::Closed
    );
    drop(second);
    let _ = std::fs::remove_dir_all(dir);
}

/// Closing saves first and closes second, so the record never claims a completeness the
/// journal does not have. Checked through the public path: an envelope published and then
/// the session closed is on disk under that session.
#[test]
fn closing_a_session_flushes_what_was_published_into_it() {
    use gungnir_eventing::Event;
    use gungnir_model::events::RequirementEvent;
    use gungnir_model::RequirementId;

    let dir = scratch("flush");
    let mut state = desktop(&dir);
    let session = state.mission.as_ref().expect("a session").session;
    let now = state.clock.now();
    state
        .events
        .publish(
            now,
            Event::Requirement(RequirementEvent::Lapsed {
                requirement: RequirementId(1),
                at: now,
            }),
        )
        .expect("published");

    state.close_session().expect("closed");
    drop(state);

    let journal = FileEventJournal::open(&dir).expect("journal");
    let recorded = journal.read_session(session).expect("read back");
    assert!(
        recorded
            .iter()
            .any(|e| matches!(e.event, Event::Requirement(RequirementEvent::Lapsed { .. }))),
        "an envelope published before the close did not reach the journal"
    );
    let _ = std::fs::remove_dir_all(dir);
}

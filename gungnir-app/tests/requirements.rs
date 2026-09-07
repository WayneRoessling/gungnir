//! Collection requirements through the desktop (GAP-005, DN-11).
//!
//! MT-08 steps 2 and 3 happened outside the system. These are what stands behind the
//! claim that they now happen inside it: an analyst states a requirement, a sensor
//! manager tasks a sensor against it or declines it, and the whole thing lapses on the
//! clock if nobody does either.
//!
//! **The property most worth protecting is that a sensor acknowledging a task is not an
//! answer.** Nothing in this file, and nothing in the desktop, may move a requirement to
//! answered without a person naming the evidence.

use gungnir_app::requirements;
use gungnir_app::state::AppState;
use gungnir_app::update;
use gungnir_config::{AssetConfig, ConfigBaseline, EndpointConfig, SensorConfig};
use gungnir_eventing::Event;
use gungnir_model::events::RequirementEvent;
use gungnir_model::{AssetPriority, MissionTime, RequirementId, RequirementState};
use gungnir_security::Role;
use gungnir_sensor_management::SensorControl;
use gungnir_time::ReplayClockAuthority;

/// A desktop with one defended asset, one commandable sensor, and one that is not.
fn desktop(name: &str) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-requirements-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let sensor = |id: u32, endpoint: Option<&str>| SensorConfig {
        id,
        modality: "radar".into(),
        position: [0.0, 0.0, 0.0],
        max_range_m: 50_000.0,
        control_endpoint: endpoint.map(ToOwned::to_owned),
        maintenance: Vec::new(),
    };
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        sensors: vec![sensor(1, Some("radar-1-control")), sensor(2, None)],
        endpoints: vec![EndpointConfig {
            name: "radar-1-control".into(),
            kind: "sensor-control".into(),
            address: "tcp://127.0.0.1:9100".into(),
        }],
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
        ..ConfigBaseline::default()
    };
    let mut state = AppState::with_config(config).expect("desktop state");
    // A step-controlled clock: a needed-by time cannot be waited out on a wall clock.
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(0.0),
    });
    (state, dir)
}

fn state_one(state: &mut AppState, minutes: Option<f64>) -> RequirementId {
    requirements::state_requirement(
        state,
        "identify the contact in the harbour".into(),
        0,
        AssetPriority::High,
        minutes,
    )
    .expect("the baseline declares an asset to state it over")
}

fn standing(state: &AppState, id: RequirementId) -> RequirementState {
    state
        .requirements
        .iter()
        .find(|r| r.id == id)
        .expect("stated")
        .state
        .clone()
}

/// A stated requirement takes its area from the asset it was stated over, and starts
/// with nothing serving it.
#[test]
fn stating_a_requirement_records_what_was_asked_and_over_where() {
    let (mut state, dir) = desktop("stating");
    let id = state_one(&mut state, Some(10.0));

    let requirement = &state.requirements[0];
    assert_eq!(requirement.id, id);
    assert_eq!(requirement.state, RequirementState::Stated);
    assert_eq!(requirement.priority, AssetPriority::High);
    assert!(
        (requirement.area.radius_m() - 2_000.0).abs() < f64::EPSILON,
        "it did not take the asset's extent: {} m",
        requirement.area.radius_m()
    );
    assert_eq!(requirement.needed_by, Some(MissionTime(600.0)));

    let rows = requirements::rows(&state);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].tasks, 0);
    assert_eq!(rows[0].time_remaining_s, Some(600.0));
    let _ = std::fs::remove_dir_all(dir);
}

/// A requirement with no needed-by time never lapses. That is a decision -- some
/// requirements stand until answered -- and it must not be confused with an overdue one.
#[test]
fn a_requirement_with_no_deadline_never_lapses() {
    let (mut state, dir) = desktop("no-deadline");
    let id = state_one(&mut state, None);

    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(1_000_000.0),
    });
    update::tick(&mut state);

    assert_eq!(standing(&state, id), RequirementState::Stated);
    assert_eq!(requirements::rows(&state)[0].time_remaining_s, None);
    let _ = std::fs::remove_dir_all(dir);
}

/// Tasking is one act: the command is issued and the concurrence recorded together.
/// A requirement is tasked only when a task really exists to serve it.
#[test]
fn tasking_issues_a_command_and_records_the_concurrence_together() {
    let (mut state, dir) = desktop("tasking");
    state.set_role(Role::SensorManager);
    let id = state_one(&mut state, Some(10.0));

    requirements::task(&mut state, id, 1, MissionTime(5.0)).expect("sensor 1 is commandable");

    match standing(&state, id) {
        RequirementState::Tasked { by } => {
            // No operator session exists, so the concurrence carries the role and says
            // nobody was signed in rather than naming a person who did not act.
            assert_eq!(by.operator(), None);
            assert_eq!(by.role(), "SensorManager");
        }
        other => panic!("expected tasked, got {other:?}"),
    }

    // The task names the requirement it serves, which is the link that did not exist
    // before GAP-005: `SensorTask::requirement` was always `None`.
    let serving = state.sensors.tasks_for(id);
    assert_eq!(serving.len(), 1);
    assert!(matches!(
        serving[0].command,
        gungnir_sensor_management::tasking::SensorCommand::Search { .. }
    ));
    assert_eq!(requirements::rows(&state)[0].tasks, 1);
    let _ = std::fs::remove_dir_all(dir);
}

/// The case DN-11 rule 4 and GAP-001 make the common one: the sensor cannot be
/// commanded, so no task exists, so **the requirement must not read as tasked**.
#[test]
fn tasking_an_uncontrollable_sensor_leaves_the_requirement_stated() {
    let (mut state, dir) = desktop("uncontrollable");
    state.set_role(Role::SensorManager);
    let id = state_one(&mut state, Some(10.0));

    let err = requirements::task(&mut state, id, 2, MissionTime(5.0))
        .expect_err("sensor 2 has no control endpoint");
    assert!(
        err.to_string().contains("no control adapter"),
        "the refusal did not say why: {err}"
    );

    assert_eq!(
        standing(&state, id),
        RequirementState::Stated,
        "a requirement nothing is serving was recorded as tasked"
    );
    assert!(
        state.sensors.tasks().is_empty(),
        "a task was recorded anyway"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// The rule DN-11 states in as many words. A sensor acknowledging a command means it
/// took the command, not that it answered the question.
#[test]
fn an_acknowledged_task_never_answers_the_requirement() {
    let (mut state, dir) = desktop("acknowledged");
    state.set_role(Role::SensorManager);
    let id = state_one(&mut state, None);
    requirements::task(&mut state, id, 1, MissionTime(5.0)).expect("commandable");

    let task = state.sensors.tasks()[0].id;
    state
        .sensors
        .acknowledge(task, MissionTime(6.0))
        .expect("the sensor took it");
    update::tick(&mut state);

    assert!(
        matches!(standing(&state, id), RequirementState::Tasked { .. }),
        "an acknowledgement answered the requirement"
    );
    let row = &requirements::rows(&state)[0];
    assert!(
        row.progress.phrase().contains("not yet answered"),
        "the panel would read as answered: {}",
        row.progress.phrase()
    );

    // Only a person naming evidence closes it.
    requirements::satisfy(
        &mut state,
        id,
        "imagery at 01:42 shows the hull number".into(),
    )
    .expect("answered");
    match standing(&state, id) {
        RequirementState::Satisfied { evidence } => {
            assert!(evidence.contains("hull number"));
        }
        other => panic!("expected answered, got {other:?}"),
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// Answering without naming evidence is refused, and a decline without a reason is too.
#[test]
fn closing_a_requirement_always_says_how() {
    let (mut state, dir) = desktop("closing");
    state.set_role(Role::SensorManager);
    let id = state_one(&mut state, None);

    assert!(
        requirements::satisfy(&mut state, id, "   ".into()).is_err(),
        "a requirement was answered with nothing behind it"
    );
    assert!(
        requirements::decline(&mut state, id, "  ".into()).is_err(),
        "a requirement was declined with no reason"
    );
    assert_eq!(standing(&state, id), RequirementState::Stated);

    requirements::decline(&mut state, id, "no sensor can reach that area".into())
        .expect("declines");
    match standing(&state, id) {
        RequirementState::Declined { reason, .. } => {
            assert!(reason.contains("no sensor can reach"));
        }
        other => panic!("expected declined, got {other:?}"),
    }
    // And a closed one is not silently reopened by tasking against it.
    assert!(requirements::task(&mut state, id, 1, MissionTime(9.0)).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

/// A requirement past its needed-by time lapses on the tick rather than remaining open,
/// and a lapse is **not** a decline: nobody refused it.
#[test]
fn an_overdue_requirement_lapses_on_the_tick_and_alerts() {
    let (mut state, dir) = desktop("lapse");
    let id = state_one(&mut state, Some(10.0));
    let alerts_before = state.alerts.len();

    // Inside the window nothing happens.
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(300.0),
    });
    update::tick(&mut state);
    assert_eq!(standing(&state, id), RequirementState::Stated);

    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(601.0),
    });
    update::tick(&mut state);
    assert_eq!(standing(&state, id), RequirementState::Lapsed);

    let new_alerts = &state.alerts[alerts_before..];
    assert!(
        new_alerts.iter().any(|a| a.contains("lapsed")),
        "the lapse raised no alert: {new_alerts:?}"
    );
    assert!(
        new_alerts.iter().all(|a| !a.contains("declined")),
        "a lapse was reported as a decline: {new_alerts:?}"
    );

    // It lapses once, however many frames pass.
    let after = state.alerts.len();
    update::tick(&mut state);
    assert_eq!(state.alerts.len(), after, "the same lapse alerted twice");
    let _ = std::fs::remove_dir_all(dir);
}

/// The lifecycle reaches the bus, which is the only reason an MT-08 replay could ever
/// read it back. A concurrence that named an operator and never left memory could not
/// be reviewed, which is the whole point of recording who concurred.
#[test]
fn the_requirement_lifecycle_is_published() {
    let (mut state, dir) = desktop("events");
    state.set_role(Role::SensorManager);
    let seen = state.events.subscribe();

    let id = state_one(&mut state, Some(10.0));
    requirements::task(&mut state, id, 1, MissionTime(5.0)).expect("commandable");
    requirements::satisfy(&mut state, id, "imagery at 01:42".into()).expect("answered");

    let events: Vec<Event> = seen.try_iter().map(|e| e.event).collect();
    let requirement_events: Vec<&RequirementEvent> = events
        .iter()
        .filter_map(|e| match e {
            Event::Requirement(r) => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(requirement_events.len(), 3, "{requirement_events:?}");
    assert!(matches!(
        requirement_events[0],
        RequirementEvent::Stated { .. }
    ));
    // The tasked event carries the task, so the link between a requirement and what
    // served it survives into the record rather than living only in the panel.
    match requirement_events[1] {
        RequirementEvent::Tasked { task, by, .. } => {
            assert_eq!(*task, state.sensors.tasks()[0].id);
            assert_eq!(by.operator(), None);
        }
        other => panic!("expected tasked, got {other:?}"),
    }
    assert!(matches!(
        requirement_events[2],
        RequirementEvent::Satisfied { .. }
    ));
    assert!(requirement_events.iter().all(|e| e.requirement() == id));
    let _ = std::fs::remove_dir_all(dir);
}

/// The authority split MT-08 turns on. The analyst states and may not concur; the
/// sensor manager concurs. The panel is told which, and the matrix is the authority.
#[test]
fn the_analyst_may_state_but_not_concur() {
    use gungnir_security::{actions::TASK_SENSOR, authz::role_permits};

    assert!(
        !role_permits(Role::IntelligenceAnalyst, TASK_SENSOR),
        "the analyst could concur with their own tasking request"
    );
    assert!(role_permits(Role::SensorManager, TASK_SENSOR));

    // Stating is not an authorized action at all, so an analyst can do it.
    let (mut state, dir) = desktop("authority");
    state.set_role(Role::IntelligenceAnalyst);
    state_one(&mut state, None);
    assert_eq!(state.requirements.len(), 1);
    let _ = std::fs::remove_dir_all(dir);
}

/// **The half GAP-005 was left open on.** An analyst who states a requirement on Monday
/// expects it on Tuesday; until the list was recovered from the journal it died with the
/// process.
#[test]
fn a_requirement_survives_a_restart() {
    let (mut state, dir) = desktop("restart");
    state.set_role(Role::SensorManager);
    let id = state_one(&mut state, Some(10.0));
    requirements::task(&mut state, id, 1, MissionTime(5.0)).expect("tasked");
    state.save_session().expect("saved");
    let config = state.config.clone();
    drop(state);

    // A second desktop over the same data directory: a restart.
    let restarted = AppState::with_config(config).expect("restarted");

    assert_eq!(restarted.requirements.len(), 1, "the requirement was lost");
    let recovered = &restarted.requirements[0];
    assert_eq!(recovered.id, id);
    assert_eq!(recovered.title, "identify the contact in the harbour");
    // The whole requirement came back, not just its title: the area and the deadline
    // are what a stated event carrying only a name would have lost.
    assert_eq!(recovered.priority, AssetPriority::High);
    assert!(recovered.area.radius_m() > 0.0, "the area was lost");
    assert_eq!(recovered.needed_by, Some(MissionTime(600.0)));
    // And the state it had reached, not the state it started in.
    assert!(
        matches!(recovered.state, RequirementState::Tasked { .. }),
        "the requirement came back as {:?} rather than tasked",
        recovered.state
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A new requirement after a restart does not collide with a recovered one. The
/// identifier used to be a per-session serial; recovering a list makes it
/// per-deployment, so the serial has to continue past what came back.
#[test]
fn a_new_requirement_does_not_reuse_a_recovered_identifier() {
    let (mut state, dir) = desktop("ids");
    let first = state_one(&mut state, None);
    let second = state_one(&mut state, None);
    state.save_session().expect("saved");
    let config = state.config.clone();
    drop(state);

    let mut restarted = AppState::with_config(config).expect("restarted");
    let third = state_one(&mut restarted, None);

    assert!(
        third != first && third != second,
        "a new requirement reused a recovered identifier: {third:?}"
    );
    assert_eq!(restarted.requirements.len(), 3);
    let _ = std::fs::remove_dir_all(dir);
}

/// An answered requirement stays answered, so a restart does not reopen work somebody
/// closed.
#[test]
fn a_closed_requirement_comes_back_closed() {
    let (mut state, dir) = desktop("closed");
    let id = state_one(&mut state, None);
    requirements::satisfy(&mut state, id, "imagery at 01:42".into()).expect("answered");
    state.save_session().expect("saved");
    let config = state.config.clone();
    drop(state);

    let restarted = AppState::with_config(config).expect("restarted");
    match &restarted.requirements[0].state {
        RequirementState::Satisfied { evidence } => assert!(evidence.contains("01:42")),
        other => panic!("a closed requirement reopened as {other:?}"),
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// A fresh deployment reports that nothing was asked for, which is a different fact from
/// a record that could not be read.
#[test]
fn a_fresh_deployment_reports_nothing_stated() {
    let (state, dir) = desktop("fresh");
    assert!(state.requirements.is_empty());
    assert_eq!(
        state.recovered,
        gungnir_app::requirements::Recovered::NothingStated
    );
    let _ = std::fs::remove_dir_all(dir);
}

//! The outbound control path through the desktop (GAP-004, DN-11 §5).
//!
//! The unit tests in `gungnir-sensor-management` prove the registry's rules and the
//! render tests in `gungnir-ui` prove PN-10 draws them. Neither proves the desktop
//! wires the two together, and the property that matters most -- a command changes
//! nothing until a sensor acknowledges -- is only true end to end if every piece
//! agrees. These run a real `AppState` through `update::tick`.
//!
//! **Nothing here acknowledges anything**, because nothing can: no adapter exists until
//! GAP-001. That is what the last test is about. It is not a limitation of the test; it
//! is the state of the system, and the test exists to make sure the system says so.

use gungnir_app::state::AppState;
use gungnir_app::sustainment;
use gungnir_app::update;
use gungnir_config::{ConfigBaseline, EndpointConfig, SensorConfig};
use gungnir_eventing::Event;
use gungnir_model::events::SensorTaskEvent;
use gungnir_model::{MissionTime, SensorId, SensorMode};
use gungnir_sensor_management::SensorRegistry;
use gungnir_time::ReplayClockAuthority;

/// A desktop with one commandable sensor and one that is not.
///
/// Sensor 1 names an endpoint; sensor 2 does not, which is what every sensor in the
/// default baseline looks like.
fn desktop(name: &str) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-sensor-control-{name}-{}",
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
        sensor_task_ack_window_s: 10.0,
        ..ConfigBaseline::default()
    };
    (AppState::with_config(config).expect("desktop state"), dir)
}

/// Put the desktop on a step-controlled clock.
///
/// A wall clock cannot be moved eleven seconds forward without waiting eleven seconds,
/// and an acknowledgement window is measured on the mission clock the tick reads -- so
/// stepping it is exactly what a deployment waiting out a window looks like. The caller
/// keeps the value and reinstalls it after each step; `ReplayClockAuthority` is `Copy`.
fn stepped(state: &mut AppState) -> ReplayClockAuthority {
    let clock = ReplayClockAuthority {
        current: MissionTime(0.0),
    };
    state.clock = Box::new(clock);
    clock
}

fn mode_of(state: &AppState, id: u32) -> SensorMode {
    state
        .sensors
        .sensors()
        .iter()
        .find(|s| s.id == SensorId(id))
        .expect("the sensor is in the baseline")
        .mode
}

/// The rule the whole note turns on. A command puts a mode in the *asked* column and
/// leaves the confirmed mode alone, all the way through the desktop.
#[test]
fn a_command_does_not_change_the_confirmed_mode() {
    let (mut state, dir) = desktop("not-confirmed");
    sustainment::command_sensor_mode(&mut state, 1, SensorMode::Search).expect("issued");

    assert_eq!(
        mode_of(&state, 1),
        SensorMode::Standby,
        "the confirmed mode moved on a command alone"
    );
    let rows = sustainment::sensor_rows(&state);
    let row = rows.iter().find(|r| r.id == 1).expect("row");
    assert_eq!(row.mode, SensorMode::Standby);
    assert_eq!(row.requested, Some(SensorMode::Search));
    assert!(
        !row.contributing,
        "a merely requested Search was credited to coverage"
    );
    // And the coverage layer agrees, which is the consequence that reaches the map.
    assert!(
        state.sensors.coverage().is_empty(),
        "coverage counted a sensor nobody has confirmed is searching"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// Rule 4: a sensor with no configured endpoint is refused and **nothing is recorded**.
/// A task in the record for a command no adapter could carry would read, later, as a
/// command that was sent and ignored.
#[test]
fn an_uncontrollable_sensor_is_refused_and_records_nothing() {
    use gungnir_sensor_management::SensorControl;

    let (mut state, dir) = desktop("uncontrollable");
    let err = sustainment::command_sensor_mode(&mut state, 2, SensorMode::Search)
        .expect_err("sensor 2 has no endpoint");

    assert!(
        err.to_string().contains("no control adapter"),
        "the refusal did not say why: {err}"
    );
    assert!(
        state.sensors.tasks().is_empty(),
        "a refused command left a task in the record"
    );
    assert_eq!(mode_of(&state, 2), SensorMode::Standby);
    let rows = sustainment::sensor_rows(&state);
    let row = rows.iter().find(|r| r.id == 2).expect("row");
    assert!(!row.controllable, "PN-10 was told sensor 2 is commandable");
    assert!(row.task.is_none() && row.requested.is_none());
    let _ = std::fs::remove_dir_all(dir);
}

/// Recording an observed mode is the other act, and it does change the confirmed mode --
/// with no task and no command. It is the only one of the two that does anything today.
#[test]
fn recording_an_observed_mode_changes_the_mode_and_commands_nothing() {
    use gungnir_sensor_management::SensorControl;

    let (mut state, dir) = desktop("observed");
    sustainment::record_observed_mode(&mut state, 2, SensorMode::Search).expect("recorded");

    assert_eq!(mode_of(&state, 2), SensorMode::Search);
    assert!(
        state.sensors.tasks().is_empty(),
        "recording what an operator knows issued a command"
    );
    assert_eq!(state.sensors.coverage().len(), 1);
    let _ = std::fs::remove_dir_all(dir);
}

/// Rule 2, through the tick: a command nobody answers becomes unacknowledged inside the
/// window, alerts, and is never retried.
///
/// This is the state every command reaches today, since no adapter exists to answer.
#[test]
fn an_unanswered_command_times_out_alerts_and_is_not_retried() {
    use gungnir_sensor_management::SensorControl;

    let (mut state, dir) = desktop("timeout");
    let mut clock = stepped(&mut state);
    let alerts_before = state.alerts.len();
    sustainment::command_sensor_mode(&mut state, 1, SensorMode::Search).expect("issued");

    // Inside the window, nothing has happened yet.
    update::tick(&mut state);
    assert!(
        state.sensors.tasks()[0].state.is_open(),
        "a command timed out before its window closed"
    );

    // Past it.
    clock.advance(state.config.sensor_task_ack_window_s + 1.0);
    state.clock = Box::new(clock);
    update::tick(&mut state);

    let task = &state.sensors.tasks()[0];
    assert!(
        !task.state.is_open(),
        "the window closed and the task is still open"
    );
    assert!(
        matches!(
            task.state,
            gungnir_sensor_management::tasking::TaskState::Unacknowledged
        ),
        "an unanswered command was recorded as something other than unacknowledged: {:?}",
        task.state
    );
    assert_eq!(
        state.sensors.tasks().len(),
        1,
        "the command was retried; retry is an operator action (DN-11 §5 rule 3)"
    );
    assert_eq!(
        mode_of(&state, 1),
        SensorMode::Standby,
        "a command nobody answered still moved the confirmed mode"
    );
    assert!(
        state
            .sensors
            .mode_status(SensorId(1))
            .is_some_and(|m| m.requested.is_none()),
        "a request nobody answered is still shown as outstanding"
    );

    let new_alerts = &state.alerts[alerts_before..];
    assert!(
        new_alerts.iter().any(|a| a.contains("did not acknowledge")),
        "the timeout raised no alert: {new_alerts:?}"
    );
    assert!(
        new_alerts.iter().any(|a| a.contains("not been retried")),
        "the alert did not say the command was left alone: {new_alerts:?}"
    );

    // A second sweep must not alert again: the task is closed, and an alert per frame
    // for one stale command would bury everything else on PN-08.
    let after_first = state.alerts.len();
    update::tick(&mut state);
    assert_eq!(
        state.alerts.len(),
        after_first,
        "the same stale command alerted twice"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// The asking and the fact are different events. An after-action review that could not
/// tell "we told the radar to search" from "the radar searched" would be unable to
/// answer the question MT-07 turns on.
#[test]
fn issuing_and_timing_out_are_journalled_as_distinct_events() {
    let (mut state, dir) = desktop("events");
    let mut clock = stepped(&mut state);
    let seen = state.events.subscribe();

    sustainment::command_sensor_mode(&mut state, 1, SensorMode::Search).expect("issued");
    clock.advance(state.config.sensor_task_ack_window_s + 1.0);
    state.clock = Box::new(clock);
    update::tick(&mut state);

    let events: Vec<Event> = seen.try_iter().map(|e| e.event).collect();
    let issued = events
        .iter()
        .filter(|e| matches!(e, Event::SensorTask(SensorTaskEvent::Issued { .. })))
        .count();
    let unacknowledged = events
        .iter()
        .filter(|e| matches!(e, Event::SensorTask(SensorTaskEvent::Unacknowledged { .. })))
        .count();
    assert_eq!(issued, 1, "the command was not published as asked-for");
    assert_eq!(unacknowledged, 1, "the timeout was not published");
    assert!(
        !events.iter().any(|e| matches!(e, Event::Sensor(_))),
        "a command published a mode change; nothing changed mode"
    );
    let _ = std::fs::remove_dir_all(dir);
}

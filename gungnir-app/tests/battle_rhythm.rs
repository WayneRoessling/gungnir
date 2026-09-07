//! Battle rhythm on the desktop (GAP-054, DN-21).
//!
//! `gungnir-reporting`'s rhythm types shipped with their own tests and **nothing
//! constructed one**: no schedule was read from a baseline, no maintenance window reached
//! a sensor record, and no handover was ever assembled. These are the tests behind the
//! claim that the rhythm runs.
//!
//! DN-21 §8's four criteria are here, and the first of them is the one that matters most:
//! **a wall-clock scheduler passes every other check**, so the determinism of the
//! mission-time scheduler is tested directly rather than inferred.

use gungnir_app::state::AppState;
use gungnir_config::{
    ConfigBaseline, MaintenanceWindowConfig, ReportingConfig, ScheduledProductConfig, SensorConfig,
};
use gungnir_eventing::{Envelope, Event, Receiver};
use gungnir_model::events::RhythmEvent;
use gungnir_model::{MissionTime, ProductKind, SensorId, SensorMode};
use gungnir_sensor_management::SensorRegistry;
use gungnir_time::ReplayClockAuthority;

fn product(
    name: &str,
    kind: &str,
    period: f64,
    deliver_to: Option<&str>,
) -> ScheduledProductConfig {
    ScheduledProductConfig {
        name: name.into(),
        kind: kind.into(),
        period_s: period,
        offset_s: 0.0,
        deliver_to: deliver_to.map(ToOwned::to_owned),
    }
}

fn sensor(windows: Vec<MaintenanceWindowConfig>) -> SensorConfig {
    SensorConfig {
        id: 1,
        modality: "radar".into(),
        position: [0.0, 0.0, 10.0],
        max_range_m: 20_000.0,
        control_endpoint: None,
        maintenance: windows,
    }
}

fn maintenance(from: f64, to: f64) -> MaintenanceWindowConfig {
    MaintenanceWindowConfig {
        from_s: from,
        to_s: to,
        reason: "antenna swap".into(),
    }
}

/// A desktop on a **replay clock**, so the rhythm can be driven deterministically.
fn desktop(
    name: &str,
    products: Vec<ScheduledProductConfig>,
    sensors: Vec<SensorConfig>,
) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-rhythm-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        reporting: ReportingConfig {
            scheduled: products,
            retention_sessions: 10,
        },
        sensors,
        ..ConfigBaseline::default()
    };
    let mut state = AppState::with_config(config).expect("the desktop starts");
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(0.0),
    });
    (state, dir)
}

fn advance(state: &mut AppState, to: f64) {
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(to),
    });
}

fn rhythm_events(events: &Receiver<Envelope>) -> Vec<RhythmEvent> {
    events
        .try_iter()
        .filter_map(|e| match e.event {
            Event::Rhythm(r) => Some(r),
            _ => None,
        })
        .collect()
}

/// **DN-21 §8's first criterion, and the one a wall-clock implementation would pass every
/// other check while failing.** The same session driven twice produces the same products
/// at the same mission times -- and those times are the schedule's, not the moments the
/// tick happened to run.
#[test]
fn the_same_run_produces_the_same_products_at_the_same_mission_times() {
    let mut due_times = Vec::new();
    for pass in 0..2 {
        let (mut state, dir) = desktop(
            &format!("determinism-{pass}"),
            vec![product("watch handover", "handover-summary", 100.0, None)],
            Vec::new(),
        );
        let events = state.events.subscribe();

        // Deliberately uneven steps: the tick rate must not decide what the schedule
        // produces. A wall-clock scheduler would fire once per tick that noticed.
        for to in [0.0, 37.0, 101.0, 150.0, 205.0, 410.0] {
            advance(&mut state, to);
            gungnir_app::rhythm::tick(&mut state);
        }

        let times: Vec<f64> = rhythm_events(&events)
            .into_iter()
            .filter_map(|e| match e {
                RhythmEvent::ProductDue { due, .. } => Some(due.0),
                _ => None,
            })
            .collect();
        due_times.push(times);
        let _ = std::fs::remove_dir_all(dir);
    }

    assert_eq!(
        due_times[0], due_times[1],
        "two runs of the same session produced different products"
    );
    assert_eq!(
        due_times[0],
        vec![100.0, 200.0, 300.0, 400.0],
        "products were published at the tick's times rather than the schedule's"
    );
}

/// A product with no endpoint is produced and held for a person. **A real configuration,
/// not a failure**, which is why it is not the same event as an undelivered one.
#[test]
fn a_product_with_no_endpoint_is_held_rather_than_reported_as_undelivered() {
    let (mut state, dir) = desktop(
        "held",
        vec![product("sitrep", "situation-report", 50.0, None)],
        Vec::new(),
    );
    let events = state.events.subscribe();
    gungnir_app::rhythm::tick(&mut state);
    advance(&mut state, 60.0);
    gungnir_app::rhythm::tick(&mut state);

    let published = rhythm_events(&events);
    assert!(
        published
            .iter()
            .any(|e| matches!(e, RhythmEvent::ProductHeld { .. })),
        "a product with no endpoint left no record that it was produced: {published:?}"
    );
    assert!(
        !published
            .iter()
            .any(|e| matches!(e, RhythmEvent::ProductUndelivered { .. })),
        "a product nobody was supposed to deliver was reported as undelivered"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// **A deployment that believed it was reporting to higher command and was not** is the
/// failure DN-21 §5's delivery rule exists to prevent. There is no delivery path yet
/// (GAP-040), so a product with an endpoint is recorded as owed, never silently dropped.
#[test]
fn a_product_with_an_endpoint_is_recorded_as_undelivered_not_dropped() {
    let dir =
        std::env::temp_dir().join(format!("gungnir-rhythm-undelivered-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        endpoints: vec![gungnir_config::EndpointConfig {
            name: "higher-command".into(),
            kind: "handoff".into(),
            address: "https://higher.example/reports".into(),
        }],
        reporting: ReportingConfig {
            scheduled: vec![product(
                "sitrep",
                "situation-report",
                50.0,
                Some("higher-command"),
            )],
            retention_sessions: 10,
        },
        ..ConfigBaseline::default()
    };
    let mut state = AppState::with_config(config).expect("the desktop starts");
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(0.0),
    });
    let events = state.events.subscribe();
    gungnir_app::rhythm::tick(&mut state);
    advance(&mut state, 60.0);
    gungnir_app::rhythm::tick(&mut state);

    let published = rhythm_events(&events);
    let undelivered = published
        .iter()
        .find(|e| matches!(e, RhythmEvent::ProductUndelivered { .. }))
        .expect("a product owed to an endpoint left no record");
    match undelivered {
        RhythmEvent::ProductUndelivered {
            endpoint, reason, ..
        } => {
            assert_eq!(endpoint, "higher-command");
            assert!(reason.contains("GAP-040"), "{reason}");
        }
        other => panic!("unexpected event: {other:?}"),
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// **DN-21 §8's second criterion.** A sensor offline inside its window raises no failure
/// alert -- and the coverage it is not providing is missing all the same, because a
/// maintenance window changes the alert and never the picture.
#[test]
fn a_sensor_down_in_its_window_is_not_a_fault_and_still_leaves_the_gap() {
    let (mut state, dir) = desktop(
        "in-window",
        Vec::new(),
        vec![sensor(vec![maintenance(10.0, 100.0)])],
    );
    // Radiating, so there is a coverage answer to lose.
    state
        .sensors
        .set_mode(SensorId(1), SensorMode::Search)
        .expect("search");
    let covering_before = state.sensors.coverage().len();
    assert_eq!(
        covering_before, 1,
        "the fixture provides no coverage to lose"
    );

    advance(&mut state, 20.0);
    state
        .sensors
        .set_mode(SensorId(1), SensorMode::Offline)
        .expect("go offline");
    let alerts_before = state.alerts.len();
    gungnir_app::rhythm::tick(&mut state);

    let record = state.sensors.get(SensorId(1)).expect("sensor 1");
    assert!(record.is_in_maintenance(MissionTime(20.0)));
    assert!(
        !record.absence_is_a_fault(MissionTime(20.0)),
        "an expected absence was reported as a fault"
    );
    assert_eq!(
        state.alerts.len(),
        alerts_before,
        "a planned outage raised an alert: {:?}",
        state.alerts
    );

    // And the picture is honest about it: the gap is there, planned or not.
    assert!(
        state.sensors.coverage().is_empty(),
        "a sensor that is off the air was still counted as covering because its outage \
         was planned"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// **DN-21 §8's third criterion, and the case the whole feature exists to surface.**
#[test]
fn a_sensor_that_does_not_return_overruns_and_alerts() {
    let (mut state, dir) = desktop(
        "overrun",
        Vec::new(),
        vec![sensor(vec![maintenance(10.0, 100.0)])],
    );
    let events = state.events.subscribe();
    state
        .sensors
        .set_mode(SensorId(1), SensorMode::Offline)
        .expect("go offline");

    advance(&mut state, 20.0);
    gungnir_app::rhythm::tick(&mut state);
    advance(&mut state, 200.0);
    gungnir_app::rhythm::tick(&mut state);

    let published = rhythm_events(&events);
    assert!(
        published
            .iter()
            .any(|e| matches!(e, RhythmEvent::MaintenanceOverrun { .. })),
        "a sensor that never came back left no overrun in the record: {published:?}"
    );
    assert!(
        state
            .alerts
            .iter()
            .any(|a| a.contains("did not return from planned maintenance")),
        "nobody was told: {:?}",
        state.alerts
    );

    // Reported once, not every frame. An alert repeated every tick is an alert nobody
    // reads.
    let alerts = state.alerts.len();
    gungnir_app::rhythm::tick(&mut state);
    assert_eq!(state.alerts.len(), alerts, "the overrun alerted twice");
    let _ = std::fs::remove_dir_all(dir);
}

/// **DN-21 §8's fourth criterion.** A handover is incomplete until somebody takes it by
/// name, and taking it is what MOE-13 counts -- so it reaches the record.
#[test]
fn a_handover_is_incomplete_until_acknowledged_by_name() {
    let (mut state, dir) = desktop(
        "handover",
        vec![product("watch handover", "handover-summary", 100.0, None)],
        Vec::new(),
    );
    let events = state.events.subscribe();

    gungnir_app::rhythm::tick(&mut state);
    assert!(
        state.rhythm.handover().is_none(),
        "a handover was open before one came due"
    );

    advance(&mut state, 150.0);
    gungnir_app::rhythm::tick(&mut state);
    let summary = state.rhythm.handover().expect("a handover came due");
    assert!(
        !summary.is_complete(),
        "a handover was complete before anyone took it"
    );
    assert_eq!(summary.baseline_version, state.config.version);

    // An empty name is refused: a handover taken by nobody is not a handover.
    assert!(gungnir_app::rhythm::acknowledge_handover(&mut state, "   ").is_err());
    assert!(!state.rhythm.handover().expect("still open").is_complete());

    gungnir_app::rhythm::set_handover_notes(&mut state, "radar 2 intermittent")
        .expect("notes accepted");
    gungnir_app::rhythm::acknowledge_handover(&mut state, "Supervisor").expect("taken");

    let summary = state.rhythm.handover().expect("still held");
    assert!(summary.is_complete());
    assert_eq!(summary.notes.as_deref(), Some("radar 2 intermittent"));

    // Taken twice is refused: the record of who has the watch is not rewritten.
    assert!(gungnir_app::rhythm::acknowledge_handover(&mut state, "Commander").is_err());
    // And notes are not rewritten after the incoming watch accepted them.
    assert!(gungnir_app::rhythm::set_handover_notes(&mut state, "actually fine").is_err());

    let published = rhythm_events(&events);
    let taken = published
        .iter()
        .find(|e| matches!(e, RhythmEvent::HandoverAcknowledged { .. }))
        .expect("the acknowledgement MOE-13 counts left no event");
    match taken {
        RhythmEvent::HandoverAcknowledged { by, .. } => assert_eq!(by, "Supervisor"),
        other => panic!("unexpected event: {other:?}"),
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// The default deployment configures no rhythm, and nothing fires. A schedule nobody asked
/// for would put products in the journal of every session ever recorded.
#[test]
fn a_deployment_with_no_schedule_produces_nothing() {
    let (mut state, dir) = desktop("none", Vec::new(), Vec::new());
    let events = state.events.subscribe();
    for to in [0.0, 1000.0, 100_000.0] {
        advance(&mut state, to);
        gungnir_app::rhythm::tick(&mut state);
    }
    assert!(rhythm_events(&events).is_empty());
    assert!(state.rhythm.handover().is_none());
    let _ = std::fs::remove_dir_all(dir);
}

/// The first tick establishes the mark rather than firing every product whose schedule
/// ever passed. A session whose clock starts at Unix seconds would otherwise produce
/// decades of handover summaries in one frame.
#[test]
fn the_first_tick_does_not_fire_a_lifetime_of_backlog() {
    let (mut state, dir) = desktop(
        "backlog",
        vec![product("watch handover", "handover-summary", 100.0, None)],
        Vec::new(),
    );
    let events = state.events.subscribe();
    advance(&mut state, 1_700_000_000.0);
    gungnir_app::rhythm::tick(&mut state);
    assert!(
        rhythm_events(&events).is_empty(),
        "the first tick fired the whole backlog"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// The kinds are kept apart in the record, because a report and a handover are different
/// products and an after-action review counts them separately.
#[test]
fn the_product_kind_reaches_the_record() {
    let (mut state, dir) = desktop(
        "kinds",
        vec![
            product("watch handover", "handover-summary", 100.0, None),
            product("sitrep", "situation-report", 100.0, None),
        ],
        Vec::new(),
    );
    let events = state.events.subscribe();
    gungnir_app::rhythm::tick(&mut state);
    advance(&mut state, 150.0);
    gungnir_app::rhythm::tick(&mut state);

    let kinds: Vec<ProductKind> = rhythm_events(&events)
        .into_iter()
        .filter_map(|e| match e {
            RhythmEvent::ProductDue { kind, .. } => Some(kind),
            _ => None,
        })
        .collect();
    assert!(kinds.contains(&ProductKind::HandoverSummary));
    assert!(kinds.contains(&ProductKind::SituationReport));
    let _ = std::fs::remove_dir_all(dir);
}

//! Engagements open on a decision and close on evidence (GAP-043, DN-06 §8).
//!
//! The state machine has its own tests in the facade. These are the wiring's: that an
//! actionable decision opens exactly one engagement per solution, that no engagement
//! closes without an evidence record, that an unobserved window closes `Indeterminate`
//! and never as success or failure, and that the two evidence sources reach the report
//! apart.

use gungnir_app::engagements;
use gungnir_app::state::AppState;
use gungnir_app::update;
use gungnir_command::{DecisionRecord, OperatorDecision};
use gungnir_config::{ConfigBaseline, ResourceConfig};
use gungnir_eventing::Event;
use gungnir_intercept_service::engagement::{EffectSource, EngagementState};
use gungnir_model::events::{engagement_outcome as outcome, EngagementEvent};
use gungnir_model::{
    Classification, DecisionId, DetectionView, EffectorLayer, InterceptSolutionView, MissionTime,
    PlanId, PlanKind, PlanView, Provenance, Quality, Releasability, ResourceId, TrackId,
    TrackStatus, TrackView,
};
use gungnir_policy::PolicyVerdict;
use gungnir_time::ReplayClockAuthority;
use gungnir_tracking_service::{SubmitError, TrackingService};

/// A picture the test controls: the desktop's real tracking service has no pipeline
/// behind it (GAP-011), so the tracks come from here.
struct ScriptedPicture {
    tracks: Vec<TrackView>,
}

impl TrackingService for ScriptedPicture {
    fn submit_detection(&mut self, _: DetectionView) -> Result<(), SubmitError> {
        Ok(())
    }
    fn poll(&mut self, _: MissionTime) {}
    fn tracks(&self) -> &[TrackView] {
        &self.tracks
    }
    fn is_healthy(&self) -> bool {
        true
    }
}

fn track(id: u64) -> TrackView {
    TrackView {
        id: TrackId(id),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::zeros(),
        covariance: nalgebra::SMatrix::identity(),
        classification: Classification::Unknown,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(0.0),
        releasability: Releasability::default(),
    }
}

fn desktop(name: &str, window: Option<f64>) -> (AppState, std::path::PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("gungnir-engagements-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        resources: vec![ResourceConfig {
            id: 1,
            position: [0.0, 0.0, 0.0],
            capacity: 4,
            layer: "point".into(),
            cost: None,
            rounds_available: None,
            reserve: None,
            handoff_endpoint: None,
            intercept_speed_mps: None,
        }],
        ..ConfigBaseline::default()
    };
    if let Some(w) = window {
        config
            .assessment
            .effect_window_s
            .insert(EffectorLayer::Point, w);
    }
    gungnir_config::validate(&config).expect("valid");
    let mut state = AppState::with_config(config).expect("the desktop starts");
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(0.0),
    });
    (state, dir)
}

fn picture(state: &mut AppState, ids: &[u64]) {
    state.tracking = Box::new(ScriptedPicture {
        tracks: ids.iter().map(|id| track(*id)).collect(),
    });
}

fn at(state: &mut AppState, t: f64) {
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(t),
    });
}

fn accepted(decision: u64, tracks: &[u64]) -> DecisionRecord {
    DecisionRecord {
        id: DecisionId(decision),
        plan: PlanView {
            id: PlanId(decision),
            mission_time: MissionTime(0.0),
            kind: PlanKind::Intercept {
                solutions: tracks
                    .iter()
                    .map(|t| InterceptSolutionView {
                        resource: ResourceId(1),
                        track: TrackId(*t),
                        intercept_point: None,
                        time_to_intercept_s: Some(10.0),
                    })
                    .collect(),
            },
            ..PlanView::default()
        },
        verdict: PolicyVerdict::RequiresHumanApproval,
        decision: OperatorDecision::Accepted,
        operator_id: None,
        mission_time: MissionTime(0.0),
    }
}

fn closed_outcomes(events: &gungnir_eventing::Receiver<gungnir_eventing::Envelope>) -> Vec<String> {
    events
        .try_iter()
        .filter_map(|env| match env.event {
            Event::Engagement(EngagementEvent::Closed { outcome, .. }) => Some(outcome),
            _ => None,
        })
        .collect()
}

/// **Every accepted decision opens exactly one engagement** per solution (DN-06 §8), on
/// the bus as well as in state; a rejected one opens none.
#[test]
fn an_actionable_decision_opens_one_engagement_per_solution() {
    let (mut state, dir) = desktop("opens", Some(30.0));
    let events = state.events.subscribe();
    assert_eq!(engagements::open_for(&mut state, &accepted(1, &[7, 8])), 2);
    assert_eq!(state.engagements.len(), 2);
    let opened = events
        .try_iter()
        .filter(|e| matches!(e.event, Event::Engagement(EngagementEvent::Opened { .. })))
        .count();
    assert_eq!(opened, 2);

    let mut rejected = accepted(2, &[9]);
    rejected.decision = OperatorDecision::Rejected {
        reason: "not this one".into(),
    };
    assert_eq!(engagements::open_for(&mut state, &rejected), 0);
    assert_eq!(state.engagements.len(), 2);
    let _ = std::fs::remove_dir_all(dir);
}

/// **No window, no engagement, and an alert** rather than a guessed timeout: an engagement
/// with an invented window would close on the invention.
#[test]
fn a_layer_without_an_effect_window_opens_nothing_and_says_so() {
    let (mut state, dir) = desktop("no-window", None);
    assert_eq!(engagements::open_for(&mut state, &accepted(1, &[7])), 0);
    assert!(state.engagements.is_empty());
    assert!(
        state
            .alerts
            .iter()
            .any(|a| a.contains("effect_window_s") && a.contains("Point")),
        "{:?}",
        state.alerts
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// The engaged track leaves the picture inside the window: effective, **on track-lifecycle
/// evidence, which the record names as weak**, and no alert -- success is not an
/// interruption (DN-06 §7).
#[test]
fn a_track_that_leaves_the_picture_inside_the_window_is_effective_on_weak_evidence() {
    let (mut state, dir) = desktop("effective", Some(30.0));
    picture(&mut state, &[7]);
    engagements::open_for(&mut state, &accepted(1, &[7]));
    let events = state.events.subscribe();
    let alerts_before = state.alerts.len();

    at(&mut state, 5.0);
    update::tick(&mut state); // seen
    picture(&mut state, &[]);
    at(&mut state, 10.0);
    update::tick(&mut state); // gone, inside the window

    let e = &state.engagements[0];
    match &e.state {
        EngagementState::Effective { evidence } => {
            assert_eq!(evidence.source, EffectSource::TrackLifecycle);
            assert!(
                evidence.detail.contains("dropped by the tracker"),
                "{}",
                evidence.detail
            );
        }
        other => panic!("expected effective, got {other:?}"),
    }
    assert_eq!(
        closed_outcomes(&events),
        vec![outcome::EFFECTIVE_TRACK_INFERRED]
    );
    assert_eq!(
        state.alerts.len(),
        alerts_before,
        "a success interrupted: {:?}",
        &state.alerts[alerts_before..]
    );
    let counts = engagements::outcome_counts(&state);
    assert_eq!(counts.effective_track_inferred, 1);
    assert_eq!(
        counts.effective_corroborated, 0,
        "track evidence was counted as corroborated"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// The track is still there when the window closes: ineffective, alerted.
#[test]
fn a_track_that_persists_past_the_window_is_ineffective_and_alerted() {
    let (mut state, dir) = desktop("ineffective", Some(30.0));
    picture(&mut state, &[7]);
    engagements::open_for(&mut state, &accepted(1, &[7]));
    let events = state.events.subscribe();
    at(&mut state, 31.0);
    update::tick(&mut state);
    assert!(matches!(
        state.engagements[0].state,
        EngagementState::Ineffective { .. }
    ));
    assert_eq!(
        closed_outcomes(&events),
        vec![outcome::INEFFECTIVE_TRACK_INFERRED]
    );
    assert!(
        state.alerts.iter().any(|a| a.contains("ineffective")),
        "{:?}",
        state.alerts
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// **An unobserved outcome closes indeterminate and never as success or failure** (DN-06
/// §8, the criterion the note calls the point of the design): the picture never showed
/// the engaged track, so its absence proves nothing.
#[test]
fn an_unobserved_window_closes_indeterminate() {
    let (mut state, dir) = desktop("indeterminate", Some(30.0));
    picture(&mut state, &[]);
    engagements::open_for(&mut state, &accepted(1, &[7]));
    let events = state.events.subscribe();
    at(&mut state, 10.0);
    update::tick(&mut state);
    assert!(
        state.engagements[0].state.is_open(),
        "closed with nothing observed"
    );
    at(&mut state, 31.0);
    update::tick(&mut state);
    assert!(matches!(
        state.engagements[0].state,
        EngagementState::Indeterminate { .. }
    ));
    assert_eq!(closed_outcomes(&events), vec![outcome::INDETERMINATE]);
    let counts = engagements::outcome_counts(&state);
    assert_eq!(
        (
            counts.effective_track_inferred,
            counts.ineffective_track_inferred,
            counts.indeterminate
        ),
        (0, 0, 1)
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A superseded plan's open engagements are aborted; another plan's are not.
#[test]
fn supersession_aborts_only_that_plans_engagements() {
    let (mut state, dir) = desktop("superseded", Some(30.0));
    picture(&mut state, &[7, 8]);
    engagements::open_for(&mut state, &accepted(1, &[7]));
    engagements::open_for(&mut state, &accepted(2, &[8]));
    engagements::observe_superseded(&mut state, PlanId(1), MissionTime(3.0));
    assert!(matches!(
        state.engagements[0].state,
        EngagementState::Aborted { .. }
    ));
    assert!(state.engagements[1].state.is_open());
    let _ = std::fs::remove_dir_all(dir);
}

/// **The report counts the two evidence sources apart** (DN-06 §8, fourth criterion),
/// from the journal alone.
#[test]
fn the_session_report_keeps_the_evidence_sources_apart() {
    let (mut state, dir) = desktop("report", Some(30.0));
    picture(&mut state, &[7, 8]);
    engagements::open_for(&mut state, &accepted(1, &[7, 8]));
    at(&mut state, 5.0);
    update::tick(&mut state);
    picture(&mut state, &[8]);
    at(&mut state, 10.0);
    update::tick(&mut state); // 7 left: effective, track-inferred
    at(&mut state, 31.0);
    update::tick(&mut state); // 8 stayed: ineffective, track-inferred

    let mut reports = gungnir_app::sustainment::ReportState::default();
    reports.generate(&state).expect("the journal folds");
    let view = gungnir_app::sustainment::reports_view(&state, &reports);
    let counts = view.counts.expect("counts after generating");
    let value = |label: &str| {
        counts
            .iter()
            .find(|c| c.label == label)
            .unwrap_or_else(|| panic!("no count line {label:?}: {counts:?}"))
            .value
    };
    assert_eq!(value("Engagements opened"), 2);
    assert_eq!(value("Effective (track-inferred)"), 1);
    assert_eq!(value("Ineffective (track-inferred)"), 1);
    assert_eq!(value("Effective (corroborated)"), 0);
    assert_eq!(value("Indeterminate"), 0);
    assert!(
        counts
            .iter()
            .find(|c| c.label == "Effective (track-inferred)")
            .and_then(|c| c.note)
            .is_some_and(|n| n.contains("never added")),
        "the track-inferred line must say it is not to be summed with the corroborated one"
    );
    let _ = std::fs::remove_dir_all(dir);
}

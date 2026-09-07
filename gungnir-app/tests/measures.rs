//! The measures catalogue reaches PN-13 from the journal (GAP-047).
//!
//! The folds have their own tests in `gungnir-reporting`. These are the wiring's, and the
//! traceability row's: every figure is recomputed from the journal, so two folds of one
//! journal agree exactly, and MOE-02 reads a later reclassification of an engaged track
//! from the picture the journal kept.

use gungnir_app::engagements;
use gungnir_app::state::AppState;
use gungnir_app::sustainment::{self, ReportState};
use gungnir_app::update;
use gungnir_command::{DecisionRecord, OperatorDecision};
use gungnir_config::{ConfigBaseline, ResourceConfig};
use gungnir_eventing::Event;
use gungnir_model::events::TrackingEvent;
use gungnir_model::{
    Classification, DecisionId, EffectorLayer, InterceptSolutionView, MissionTime, PlanId,
    PlanKind, PlanView, Provenance, Quality, Releasability, ResourceId, TrackId, TrackStatus,
    TrackView,
};
use gungnir_policy::PolicyVerdict;
use gungnir_ui::panels::reports::MeasureLineValue;

fn desktop(name: &str) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-measures-{name}-{}", std::process::id()));
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
    config
        .assessment
        .effect_window_s
        .insert(EffectorLayer::Point, 30.0);
    let state = AppState::with_config(config).expect("the desktop starts");
    (state, dir)
}

fn track(id: u64, classification: Classification) -> TrackView {
    TrackView {
        id: TrackId(id),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::zeros(),
        covariance: nalgebra::SMatrix::identity(),
        classification,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(0.0),
        releasability: Releasability::default(),
    }
}

fn accepted(decision: u64, track: u64) -> DecisionRecord {
    DecisionRecord {
        id: DecisionId(decision),
        plan: PlanView {
            id: PlanId(decision),
            kind: PlanKind::Intercept {
                solutions: vec![InterceptSolutionView {
                    resource: ResourceId(1),
                    track: TrackId(track),
                    intercept_point: None,
                    time_to_intercept_s: Some(10.0),
                }],
            },
            ..PlanView::default()
        },
        verdict: PolicyVerdict::RequiresHumanApproval,
        decision: OperatorDecision::Accepted,
        operator_id: None,
        mission_time: MissionTime(0.0),
    }
}

fn line<'a>(
    view: &'a gungnir_ui::panels::reports::ReportsView<'a>,
    id: &str,
) -> &'a gungnir_ui::panels::reports::MeasureLine {
    view.measures
        .expect("generated")
        .iter()
        .find(|m| m.id == id)
        .unwrap_or_else(|| panic!("no {id}"))
}

/// **An engagement of a track later carried as friendly is counted** (MOE-02), from the
/// journal alone; and the whole catalogue is on the panel, refused rows included.
#[test]
fn moe_02_counts_an_engaged_track_later_shown_friendly() {
    let (mut state, dir) = desktop("moe02");
    engagements::open_for(&mut state, &accepted(1, 7));
    engagements::open_for(&mut state, &accepted(2, 8));
    let now = state.clock.now();
    state
        .events
        .publish(
            now,
            Event::Tracking(TrackingEvent::TrackUpdated(track(
                7,
                Classification::Friendly,
            ))),
        )
        .expect("publish");
    update::tick(&mut state); // journals the frame

    let mut reports = ReportState::default();
    reports.generate(&state).expect("the journal folds");
    let view = sustainment::reports_view(&state, &reports);
    assert_eq!(line(&view, "MOE-02").value, MeasureLineValue::Count(1));
    // No `Decided` was journaled for these engagements (opened straight from records),
    // so MOE-05 counts them as lacking a record and says so.
    assert_eq!(
        line(&view, "MOE-05").value,
        MeasureLineValue::Fraction {
            value: 0.0,
            numerator: 0,
            denominator: 2
        }
    );
    assert!(line(&view, "MOE-05")
        .note
        .as_deref()
        .is_some_and(|n| n.contains("GAP-032")));
    // Health went on the record at the first tick, before anything was decided.
    assert!(matches!(
        &line(&view, "MOE-06").value,
        MeasureLineValue::Count(0)
    ));
    assert!(matches!(
        &line(&view, "MOE-01").value,
        MeasureLineValue::NotComputable { reason } if reason.contains("GAP-045")
    ));
    assert_eq!(
        view.measures.expect("generated").len(),
        13,
        "a row went missing"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// **Every figure is recomputed from the journal** (the traceability row): a second
/// generation over the same journal is identical, figure for figure and reason for
/// reason.
#[test]
fn two_folds_of_one_journal_agree_exactly() {
    let (mut state, dir) = desktop("determinism");
    engagements::open_for(&mut state, &accepted(1, 7));
    update::tick(&mut state);

    let mut first = ReportState::default();
    first.generate(&state).expect("generate");
    let mut second = ReportState::default();
    second.generate(&state).expect("generate");
    let a = sustainment::reports_view(&state, &first);
    let b = sustainment::reports_view(&state, &second);
    assert_eq!(a.measures, b.measures);
    assert_eq!(a.counts, b.counts);
    let _ = std::fs::remove_dir_all(dir);
}

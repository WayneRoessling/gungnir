//! The fires deconfliction engine runs in the desktop's chain (GAP-036, DN-05 §5, §8).
//!
//! It existed with its tests and the chain never ran it. These are the wiring's: a fires
//! task is judged by every check, a check without data fails with a stated reason and
//! never passes, the failed task is denied and never queued, and the checks reach PN-05.

use gungnir_app::decisions::{self, Submitted};
use gungnir_app::state::AppState;
use gungnir_command::ApprovalWorkflow;
use gungnir_config::{ConfigBaseline, ResourceConfig};
use gungnir_model::{
    Classification, DeconflictionKind, DeconflictionResult, FiresPlan, Geodetic, MissionTime,
    PlanId, PlanKind, PlanView, Provenance, Quality, Releasability, ResourceId, TrackId,
    TrackStatus, TrackView,
};
use gungnir_policy::{DenialReason, PolicyVerdict};
use gungnir_tracking_service::{SubmitError, TrackingService};

const ORIGIN: [f64; 3] = [0.959_931, 0.209_440, 0.0];

struct Picture(Vec<TrackView>);
impl TrackingService for Picture {
    fn submit_detection(&mut self, _: gungnir_model::DetectionView) -> Result<(), SubmitError> {
        Ok(())
    }
    fn poll(&mut self, _: MissionTime) {}
    fn tracks(&self) -> &[TrackView] {
        &self.0
    }
    fn is_healthy(&self) -> bool {
        true
    }
}

fn desktop(name: &str) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-fires-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some(ORIGIN),
        resources: vec![ResourceConfig {
            id: 3,
            position: ORIGIN,
            capacity: 4,
            layer: "area".into(),
            cost: None,
            rounds_available: None,
            reserve: None,
            handoff_endpoint: None,
            intercept_speed_mps: None,
        }],
        ..ConfigBaseline::default()
    };
    // The two engines ahead of fires deconfliction are satisfied on purpose, so what
    // the test sees is the fourth engine's verdict: the area layer is weapons free and
    // the operator holds decision authority on it (DN-09).
    config.policy.control_status.by_layer.insert(
        gungnir_model::EffectorLayer::Area,
        gungnir_model::policy_settings::WeaponsControlStatus::Free,
    );
    config
        .policy
        .authority
        .rules
        .push(gungnir_model::policy_settings::AuthorityRule {
            action: gungnir_app::decisions::DECISION_ACTION.into(),
            role: "Operator".into(),
            layer: Some(gungnir_model::EffectorLayer::Area),
            class: None,
            pre_delegated: false,
        });
    gungnir_config::validate(&config).expect("valid");
    let state = AppState::with_config(config).expect("the desktop starts");
    (state, dir)
}

fn fires_task(error_m: f64) -> PlanView {
    PlanView {
        id: PlanId(1),
        kind: PlanKind::Fires(Box::new(FiresPlan {
            target: TrackId(7),
            target_position: Geodetic {
                lat_rad: ORIGIN[0],
                lon_rad: ORIGIN[1] + 0.000_5, // ~1.83 km east at 55° N
                alt_m: 0.0,
            },
            location_error_m: error_m,
            firing_unit: ResourceId(3),
            time_on_target: None,
            deconfliction: DeconflictionResult::default(),
        })),
        ..PlanView::default()
    }
}

fn friendly_at_enu(e: f64, n: f64) -> TrackView {
    let mut t = TrackView {
        id: TrackId(9),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::zeros(),
        covariance: nalgebra::SMatrix::identity(),
        classification: Classification::Friendly,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(0.0),
        releasability: Releasability::default(),
    };
    t.state[0] = e;
    t.state[1] = n;
    t
}

/// **A check with missing data fails and never passes** (DN-05 §5 rule 2): three checks
/// have no source today, so a fires task is denied, never queued, and every check is on
/// PN-05 with its reason.
#[test]
fn a_fires_task_is_denied_while_three_checks_have_no_source() {
    let (mut state, dir) = desktop("no-sources");
    state.tracking = Box::new(Picture(vec![]));
    let outcome = decisions::submit(&mut state, fires_task(40.0));
    assert_eq!(
        outcome,
        Submitted::Evaluated(PolicyVerdict::Denied {
            reason_code: DenialReason::FiresDeconfliction
        })
    );
    assert!(
        state.approvals.pending().is_empty(),
        "a denied fires task was queued"
    );

    let checks = &state.fires_checks;
    assert_eq!(checks.len(), 5, "{checks:?}");
    let by = |k: DeconflictionKind| checks.iter().find(|c| c.kind == k).expect("check");
    assert!(by(DeconflictionKind::LocationAccuracy).passed);
    assert!(
        by(DeconflictionKind::FriendlyPosition).passed,
        "no friendly track is near"
    );
    for k in [
        DeconflictionKind::NoFireArea,
        DeconflictionKind::AirspaceMeasure,
        DeconflictionKind::InterceptorTrajectory,
    ] {
        let c = by(k);
        assert!(!c.passed, "{k:?} passed with no data");
        assert!(c.detail.contains("could not be evaluated"), "{}", c.detail);
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// Friendly positions come from the picture: a friendly track inside the keep-out
/// radius fails that check with the distance stated.
#[test]
fn a_friendly_track_inside_the_keep_out_fails_the_friendly_check() {
    let (mut state, dir) = desktop("friendly");
    // 0.0005 rad of longitude at 55° N is about 1.83 km east; a friendly 1.85 km east
    // is a few tens of metres from the target, inside the 40 m + 500 m keep-out.
    state.tracking = Box::new(Picture(vec![friendly_at_enu(1_850.0, 0.0)]));
    let _ = decisions::submit(&mut state, fires_task(40.0));
    let friendly = state
        .fires_checks
        .iter()
        .find(|c| c.kind == DeconflictionKind::FriendlyPosition)
        .expect("check");
    assert!(!friendly.passed, "{}", friendly.detail);
    assert!(
        friendly.detail.contains("inside the"),
        "{}",
        friendly.detail
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A plan that is not a fires task carries no fires checks on PN-05.
#[test]
fn an_intercept_plan_has_no_fires_checks() {
    let (mut state, dir) = desktop("intercept");
    let _ = decisions::submit(
        &mut state,
        PlanView {
            id: PlanId(2),
            ..PlanView::default()
        },
    );
    assert!(state.fires_checks.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

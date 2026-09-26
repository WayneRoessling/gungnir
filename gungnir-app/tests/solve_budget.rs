// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The solve budget, through the desktop (GAP-119, D-81, D-82; DN-04 §10).
//!
//! `gungnir-intercept-service`'s own tests hold the planner to its budget. What they
//! cannot hold is what an operator sees when the planner cannot answer a tick: that PN-05
//! says the plan is stale, how old it is and why, above the plan itself; that PN-07 names
//! it among the conditions a decision is taken under, so accept waits on an
//! acknowledgement of exactly that; and that all of it clears on the next tick the
//! planner answers. These run against a real `AppState` and the tick the binary runs,
//! with the planner's clock stepped rather than slept on.

use std::sync::Arc;
use std::time::Duration;

use gungnir_app::state::{AppState, PlanStanding};
use gungnir_app::{decisions, update, workspace};
use gungnir_config::{ConfigBaseline, ResourceConfig};
use gungnir_intercept_service::SteppedClock;
use gungnir_model::policy_settings::{AuthorityRule, WeaponsControlStatus};
use gungnir_model::{
    Classification, EffectorLayer, MissionTime, Provenance, Quality, Releasability, TrackId,
    TrackStatus, TrackView,
};
use gungnir_tracking_service::{SubmitError, TrackingService};
use gungnir_ui::harness::RenderProbe;
use gungnir_workflow::PanelId;

const ORIGIN: [f64; 3] = [0.959_931, 0.209_440, 0.0];

/// A picture the test sets, shared with the state so it can grow between ticks.
#[derive(Clone, Default)]
struct Picture(Arc<std::sync::Mutex<Vec<TrackView>>>);

struct PictureService {
    picture: Picture,
    snapshot: Vec<TrackView>,
}

impl TrackingService for PictureService {
    fn submit_detection(&mut self, _: gungnir_model::DetectionView) -> Result<(), SubmitError> {
        Ok(())
    }
    fn poll(&mut self, _: MissionTime) {
        if let Ok(tracks) = self.picture.0.lock() {
            self.snapshot.clone_from(&tracks);
        }
    }
    fn tracks(&self) -> &[TrackView] {
        &self.snapshot
    }
    fn is_healthy(&self) -> bool {
        true
    }
}

fn track(id: u64, east_m: f64) -> TrackView {
    let mut t = TrackView {
        id: TrackId(id),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::zeros(),
        covariance: nalgebra::SMatrix::identity(),
        classification: Classification::Hostile,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(0.0),
        releasability: Releasability::default(),
    };
    t.state[0] = east_m;
    t.state[3] = -120.0;
    t
}

fn effector(id: u32) -> ResourceConfig {
    ResourceConfig {
        id,
        position: ORIGIN,
        capacity: 1,
        layer: "point".into(),
        cost: None,
        rounds_available: None,
        reserve: None,
        handoff_endpoint: None,
        intercept_speed_mps: Some(400.0),
    }
}

/// Three closing tracks and three point effectors, a chain that clears a point plan for
/// an Operator, and a planner whose clock the test steps.
fn desktop(name: &str) -> (AppState, Picture, Arc<SteppedClock>, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-solve-budget-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let mut config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some(ORIGIN),
        resources: vec![effector(40), effector(41), effector(42)],
        ..ConfigBaseline::default()
    };
    config
        .policy
        .control_status
        .by_layer
        .insert(EffectorLayer::Point, WeaponsControlStatus::Free);
    config.policy.authority.rules.push(AuthorityRule {
        action: decisions::DECISION_ACTION.into(),
        role: "Operator".into(),
        layer: Some(EffectorLayer::Point),
        class: None,
        pre_delegated: false,
    });
    gungnir_config::validate(&config).expect("the baseline is valid");
    let mut state = AppState::with_config(config).expect("the desktop starts");
    let picture = Picture::default();
    if let Ok(mut tracks) = picture.0.lock() {
        *tracks = vec![
            track(70, 30_000.0),
            track(71, 20_000.0),
            track(72, 10_000.0),
        ];
    }
    state.tracking = Box::new(PictureService {
        picture: picture.clone(),
        snapshot: Vec::new(),
    });
    let clock = Arc::new(SteppedClock::new(Duration::ZERO));
    state.intercept =
        Box::new(gungnir_app::state::embedded_planner(&state.config).with_clock(clock.clone()));
    (state, picture, clock, dir)
}

fn pn05(state: &AppState) -> gungnir_ui::harness::DrawnFrame {
    RenderProbe::new()
        .draw(|ui| {
            let _ = workspace::render_panel(ui, PanelId::InterceptPanel, state);
        })
        .1
}

fn pn07(state: &mut AppState) -> gungnir_ui::harness::DrawnFrame {
    let mut dialog = std::mem::take(&mut state.dialog);
    let frame = RenderProbe::new()
        .draw(|ui| {
            let _ = workspace::render_decision_dialog(ui, state, &mut dialog);
        })
        .1;
    state.dialog = dialog;
    frame
}

/// **The deployment's budget and the planner's default are one figure**, MOP-06's 4 ms
/// (D-81). Two crates hold it because `gungnir-config` cannot depend on the planner; this
/// is what keeps them from parting.
#[test]
fn the_baseline_default_is_the_planners_default() {
    let config = ConfigBaseline::default();
    assert_eq!(
        config.plan_solve_budget().expect("the default is valid"),
        gungnir_intercept_service::DEFAULT_SOLVE_BUDGET
    );
    assert_eq!(
        gungnir_app::state::embedded_planner(&config).solve_budget(),
        Duration::from_millis(4)
    );
}

/// A baseline's own budget reaches the planner the desktop builds, and the planner it
/// falls back to builds the same way (D-81).
#[test]
fn the_baselines_budget_reaches_the_planner() {
    let config = ConfigBaseline {
        plan_solve_budget_ms: 12.5,
        ..ConfigBaseline::default()
    };
    assert_eq!(
        gungnir_app::state::embedded_planner(&config).solve_budget(),
        Duration::from_micros(12_500)
    );
}

/// **The operator sees a stale plan as stale, everywhere it matters, and then sees it
/// clear** (GAP-119, D-82).
#[test]
fn a_plan_the_planner_cannot_refresh_is_shown_stale_and_clears_when_it_can() {
    let (mut state, picture, clock, dir) = desktop("stale");

    // t1: the planner answers inside its budget. The plan is proposed and queued, and
    // nothing on PN-05 or PN-07 calls it stale.
    update::tick(&mut state);
    assert_eq!(state.plan_standing, PlanStanding::Current);
    assert!(state.health().intercept_healthy);
    let first = state.last_plan.clone();
    assert!(
        !first.is_empty(),
        "three effectors and three tracks planned nothing"
    );
    let rows = decisions::queue_rows(&state);
    let item = rows
        .iter()
        .find(|r| r.plan_id == first.id)
        .map(|r| r.id)
        .expect("the plan was queued for a decision");
    drop(rows);
    state.select_approval(item);
    assert!(!pn05(&state).says("STALE"));
    assert!(!pn07(&mut state).says("STALE PLAN"));

    // t2: a fourth track arrives, and the solve for the new picture does not finish
    // inside its budget. The plan in force is kept, unchanged, and labelled.
    if let Ok(mut tracks) = picture.0.lock() {
        tracks.push(track(73, 40_000.0));
    }
    clock.set_step(Duration::from_millis(10));
    update::tick(&mut state);
    match &state.plan_standing {
        PlanStanding::Stale {
            computed_at,
            asked_at,
            reason,
        } => {
            assert_eq!(*computed_at, first.mission_time);
            assert!(asked_at.0 >= computed_at.0);
            assert!(reason.contains("4 ms budget"), "{reason}");
        }
        other => panic!("the tick that could not be answered left {other:?}"),
    }
    assert_eq!(
        state.last_plan, first,
        "the plan in force changed while stale"
    );
    assert!(
        !state.health().intercept_healthy,
        "a stale planner reads healthy"
    );

    let panel = pn05(&state);
    assert!(
        panel.says("STALE: this plan was computed at t ="),
        "{}",
        panel.joined()
    );
    assert!(
        panel.says("does not answer the picture on screen"),
        "{}",
        panel.joined()
    );
    assert!(panel.says("4 ms budget"), "{}", panel.joined());

    // PN-07 names it among the degraded conditions, with its age and its reason -- which
    // is what the acknowledgement that gates accept is given against (D-82).
    let dialog = pn07(&mut state);
    assert!(
        dialog.says("STALE PLAN: computed at t ="),
        "{}",
        dialog.joined()
    );
    assert!(dialog.says("4 ms budget"), "{}", dialog.joined());
    assert!(!state.dialog.degraded_acknowledged);

    // t3: the planner finishes the new picture inside its budget. Fresh, healthy, and
    // nothing says stale any more.
    clock.set_step(Duration::ZERO);
    update::tick(&mut state);
    assert_eq!(state.plan_standing, PlanStanding::Current);
    assert!(state.health().intercept_healthy);
    assert!(!pn05(&state).says("STALE"));
    assert!(!pn07(&mut state).says("STALE PLAN"));

    drop(state);
    let _ = std::fs::remove_dir_all(dir);
}

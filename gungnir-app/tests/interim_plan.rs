// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! An interim plan, through the desktop (GAP-156, D-93; DN-04 §11).
//!
//! `gungnir-intercept-service`'s own tests hold the planner to when it stands a one-step
//! answer in and what it says about it. What they cannot hold is what an operator sees:
//! that PN-05 says INTERIM above the plan, with how much of the optimum it is known to
//! reach and why the optimum is not here; that the plan is proposed and queued, and its
//! PN-06 row says INTERIM; that PN-07 names it among the conditions a decision is taken
//! under, so accept waits on an acknowledgement of exactly that; and that when the full
//! solve reaches the same assignment the plan stands and nothing more is asked about it.
//! These run against a real
//! `AppState` and the tick the binary runs, with the planner's clock stepped and the
//! mission clock replayed, so nothing depends on how fast this machine is.

use std::sync::Arc;
use std::time::Duration;

use gungnir_app::state::{AppState, PlanStanding};
use gungnir_app::{decisions, update, workspace};
use gungnir_config::{ConfigBaseline, ResourceConfig};
use gungnir_intercept_service::SteppedClock;
use gungnir_model::policy_settings::{AuthorityRule, WeaponsControlStatus};
use gungnir_model::{
    Classification, EffectorLayer, MissionTime, PlanBasis, Provenance, Quality, Releasability,
    TrackId, TrackStatus, TrackView,
};
use gungnir_time::ReplayClockAuthority;
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
/// an Operator, a planner whose clock the test steps and a mission clock it sets.
fn desktop(name: &str) -> (AppState, Picture, Arc<SteppedClock>, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-interim-plan-{name}-{}",
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
    // Two tracks, so the first plan pairs the later effectors (the tie rule) and the
    // four-track picture's interim answer, the diagonal, is a different recommendation.
    if let Ok(mut tracks) = picture.0.lock() {
        *tracks = vec![track(71, 20_000.0), track(72, 10_000.0)];
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

fn tick_at(state: &mut AppState, t: f64) {
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(t),
    });
    update::tick(state);
}

fn pn05(state: &AppState) -> gungnir_ui::harness::DrawnFrame {
    RenderProbe::new()
        .draw(|ui| {
            let _ = workspace::render_panel(ui, PanelId::InterceptPanel, state);
        })
        .1
}

fn pn06(state: &AppState) -> gungnir_ui::harness::DrawnFrame {
    RenderProbe::new()
        .draw(|ui| {
            let _ = workspace::render_panel(ui, PanelId::ApprovalQueue, state);
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

/// **The deployment's wait and the planner's default are one figure**, MOP-07's 500 ms
/// (D-93), and a baseline's own wait reaches the planner the desktop builds.
#[test]
fn the_baseline_default_wait_is_the_planners_default() {
    let config = ConfigBaseline::default();
    assert_eq!(
        config.plan_stand_in_after().expect("the default is valid"),
        gungnir_intercept_service::DEFAULT_STAND_IN_AFTER
    );
    assert_eq!(
        gungnir_app::state::embedded_planner(&config).stand_in_after(),
        Duration::from_millis(500)
    );
    let config = ConfigBaseline {
        plan_stand_in_after_ms: 1_250.0,
        ..ConfigBaseline::default()
    };
    assert_eq!(
        gungnir_app::state::embedded_planner(&config).stand_in_after(),
        Duration::from_millis(1_250)
    );
}

/// **An operator mid-raid gets an answer to the picture in front of them, labelled as
/// not the optimum everywhere it is shown, and then the optimum** (GAP-156, D-93).
#[test]
#[allow(clippy::too_many_lines)] // one operator's view told in order
fn a_picture_the_planner_cannot_finish_gets_a_labelled_interim_plan() {
    let (mut state, picture, clock, dir) = desktop("interim");

    // t = 1: answered inside the budget.
    tick_at(&mut state, 1.0);
    assert_eq!(state.plan_standing, PlanStanding::Current);
    let first = state.last_plan.clone();
    assert_eq!(first.basis, PlanBasis::Exact);

    // t = 2: a fourth track, and the exact solve cannot advance. Inside the wait the plan
    // in force is the last good one, stale.
    if let Ok(mut tracks) = picture.0.lock() {
        tracks.insert(0, track(70, 30_000.0));
        tracks.push(track(73, 40_000.0));
    }
    clock.set_step(Duration::from_millis(10));
    tick_at(&mut state, 2.0);
    assert!(
        matches!(state.plan_standing, PlanStanding::Stale { .. }),
        "{:?}",
        state.plan_standing
    );
    tick_at(&mut state, 2.3);
    assert!(matches!(state.plan_standing, PlanStanding::Stale { .. }));
    assert_eq!(state.last_plan, first);

    // t = 2.5: half a second behind, and the stand-in answers the picture on screen.
    tick_at(&mut state, 2.5);
    match &state.plan_standing {
        PlanStanding::Interim { share, reason } => {
            assert_eq!(share, "worth at least 100% of the best plan's value");
            assert!(reason.contains("has not finished 500 ms after"), "{reason}");
            assert!(
                reason.contains("% done after"),
                "the desktop's own progress: {reason}"
            );
        }
        other => panic!("half a second behind, the standing was {other:?}"),
    }
    let interim = state.last_plan.clone();
    assert_eq!(interim.basis, PlanBasis::OneStep);
    assert_ne!(
        interim.id, first.id,
        "the interim plan was not proposed as a new plan"
    );
    assert!(
        !state.health().intercept_healthy,
        "an interim answer is not the planner's own, and the strip must say so"
    );

    // PN-05: INTERIM above the plan, with the bound and the reason, and the plan drawn.
    let panel = pn05(&state);
    assert!(
        panel.says("INTERIM: this plan is the best assignment"),
        "{}",
        panel.joined()
    );
    assert!(panel.says("at least 100%"), "{}", panel.joined());
    assert!(
        panel.says("has not finished 500 ms after"),
        "{}",
        panel.joined()
    );
    assert!(!panel.says("STALE"), "{}", panel.joined());

    // PN-06: proposed and queued, and its row says what it is.
    let rows = decisions::queue_rows(&state);
    let item = rows
        .iter()
        .find(|r| r.plan_id == interim.id)
        .map(|r| (r.id, r.basis))
        .expect("the interim plan was queued for a decision");
    drop(rows);
    assert_eq!(item.1, PlanBasis::OneStep);
    let queue = pn06(&state);
    assert!(queue.says("INTERIM"), "{}", queue.joined());

    // PN-07: named among the conditions, twice over -- the planner's and the item's -- and
    // accept waits on the acknowledgement.
    state.select_approval(item.0);
    let dialog = pn07(&mut state);
    assert!(dialog.says("INTERIM PLAN"), "{}", dialog.joined());
    assert!(
        dialog.says("this item's plan is an interim one-step answer"),
        "{}",
        dialog.joined()
    );
    assert!(!state.dialog.degraded_acknowledged);

    // t = 3: the exact solve can run and reaches the same assignment. The interim plan
    // stands (GAP-097: one pairing, one item), the planner is current, PN-05 says the full
    // solve has since reached it, and PN-07 asks nothing more about how it was reached.
    clock.set_step(Duration::ZERO);
    tick_at(&mut state, 3.0);
    assert_eq!(state.plan_standing, PlanStanding::Current);
    assert!(state.health().intercept_healthy);
    assert_eq!(state.last_plan, interim, "the confirmed plan was not kept");
    assert_eq!(
        decisions::queue_rows(&state)
            .iter()
            .filter(|r| r.plan_id == interim.id)
            .count(),
        1,
        "the confirmation queued the same pairing again"
    );
    let panel = pn05(&state);
    assert!(!panel.says("INTERIM: this plan is"), "{}", panel.joined());
    assert!(
        panel.says("full solve has since reached the same assignment"),
        "{}",
        panel.joined()
    );
    let dialog = pn07(&mut state);
    assert!(
        !dialog.says("this item's plan is an interim one-step answer"),
        "{}",
        dialog.joined()
    );
    assert!(
        !dialog.says("INTERIM PLAN: the planner"),
        "{}",
        dialog.joined()
    );

    drop(state);
    let _ = std::fs::remove_dir_all(dir);
}

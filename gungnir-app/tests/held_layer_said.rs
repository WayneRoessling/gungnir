// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! **A layer at hold that is refusing every plan is said** (GAP-183, D-114;
//! `docs/design/DN-09-authority-and-control-status.md` §9).
//!
//! DN-09 refuses a plan whole if any of its solutions is refused, and the allocator tasks
//! every adequate resource, so a deployment with its area layer at hold and its point
//! layer weapons free is offered nothing at all: every plan tasks the area battery. The
//! owner kept that rule (D-114) and asked for it to be said instead -- on PN-06 under the
//! empty queue, and on PN-05 beside the plan -- naming the layer, how many plans its hold
//! has refused, and that lifting the hold is what would let the other layers' engagements
//! through, so an operator never reads the empty queue as a quiet sector.
//!
//! This runs a real `AppState` and the tick the binary runs, with a picture the test sets,
//! the planner's clock stepped and the mission clock replayed. The linked desktop's half
//! is `linked_plan_standing.rs`'s `a_linked_desktop_is_told_a_held_layer_refuses_every_plan`,
//! which has the node harness.

use std::sync::Arc;
use std::time::Duration;

use gungnir_app::state::AppState;
use gungnir_app::{decisions, update, workspace};
use gungnir_command::ApprovalWorkflow;
use gungnir_config::{ConfigBaseline, ResourceConfig};
use gungnir_intercept_service::SteppedClock;
use gungnir_model::policy_settings::{AuthorityRule, WeaponsControlStatus};
use gungnir_model::{
    Classification, EffectorLayer, HeldLayerView, MissionTime, Provenance, Quality, Releasability,
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

fn effector(id: u32, layer: &str) -> ResourceConfig {
    ResourceConfig {
        id,
        position: ORIGIN,
        capacity: 1,
        layer: layer.into(),
        cost: None,
        rounds_available: None,
        reserve: None,
        handoff_endpoint: None,
        intercept_speed_mps: Some(400.0),
    }
}

/// A point effector and an area battery, in `resources` order; the point layer weapons
/// free, the area layer at `area`; an Operator holding the authority to decide at both, so
/// a refusal here is the control status's and nobody's authority.
fn desktop(
    name: &str,
    area: WeaponsControlStatus,
    resources: Vec<ResourceConfig>,
) -> (AppState, Picture, std::path::PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("gungnir-held-layer-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some(ORIGIN),
        resources,
        ..ConfigBaseline::default()
    };
    let control = &mut config.policy.control_status.by_layer;
    control.insert(EffectorLayer::Point, WeaponsControlStatus::Free);
    control.insert(EffectorLayer::Area, area);
    for layer in [EffectorLayer::Point, EffectorLayer::Area] {
        config.policy.authority.rules.push(AuthorityRule {
            action: decisions::DECISION_ACTION.into(),
            role: "Operator".into(),
            layer: Some(layer),
            class: None,
            pre_delegated: false,
        });
    }
    gungnir_config::validate(&config).expect("the baseline is valid");
    let mut state = AppState::with_config(config).expect("the desktop starts");
    let picture = Picture::default();
    state.tracking = Box::new(PictureService {
        picture: picture.clone(),
        snapshot: Vec::new(),
    });
    state.intercept = Box::new(
        gungnir_app::state::embedded_planner(&state.config)
            .with_clock(Arc::new(SteppedClock::new(Duration::ZERO))),
    );
    (state, picture, dir)
}

fn point_then_area() -> Vec<ResourceConfig> {
    vec![effector(40, "point"), effector(50, "area")]
}

fn set(picture: &Picture, tracks: Vec<TrackView>) {
    if let Ok(mut held) = picture.0.lock() {
        *held = tracks;
    }
}

fn tick_at(state: &mut AppState, t: f64) {
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(t),
    });
    update::tick(state);
}

fn panel(state: &AppState, id: PanelId) -> gungnir_ui::harness::DrawnFrame {
    RenderProbe::new()
        .draw(|ui| {
            let _ = workspace::render_panel(ui, id, state);
        })
        .1
}

/// **The raid, the held area layer, and lifting the hold.** Two plans are proposed as
/// the raid grows; the area layer's hold refuses both, so nothing is queued, and PN-06
/// names the area layer as blocking with the count while PN-05 says the plan on screen
/// will not reach the queue. Lifted -- a new baseline in force, which is how a hold is
/// lifted today -- the same raid's plan is queued and neither line is drawn.
#[test]
fn a_held_area_layer_is_named_as_blocking_until_its_hold_is_lifted() {
    let (mut state, picture, dir) = desktop("held", WeaponsControlStatus::Hold, point_then_area());
    set(&picture, vec![track(71, 20_000.0), track(72, 10_000.0)]);
    tick_at(&mut state, 1.0);
    let first = state.last_plan.id;
    set(
        &picture,
        vec![
            track(70, 30_000.0),
            track(71, 20_000.0),
            track(72, 10_000.0),
        ],
    );
    tick_at(&mut state, 2.0);
    assert_ne!(state.last_plan.id, first, "the growing raid is re-planned");
    assert!(
        state.last_plan.assignments().iter().any(|(r, _)| r.0 == 50),
        "the planner tasks the area battery: {:?}",
        state.last_plan.assignments()
    );

    // Nothing reached a person, and the desk says which layer and how often.
    assert!(state.desk.approvals.queue().is_empty());
    let held = decisions::held_layers(&state);
    assert_eq!(
        held,
        vec![HeldLayerView {
            layer: EffectorLayer::Area,
            refused: 2,
            evaluated: 2,
            since: MissionTime(1.0),
        }]
    );
    let pn06 = panel(&state, PanelId::ApprovalQueue);
    assert!(
        pn06.says(
            "The area layer is at HOLD and is refusing every plan that tasks it: 2 of the 2 \
             plan(s) evaluated since its first refusal were refused for it"
        ),
        "{}",
        pn06.joined()
    );
    assert!(
        pn06.says("Lifting the hold on the area layer -- a supervisor's or commander's act"),
        "{}",
        pn06.joined()
    );
    assert!(pn06.says("not a quiet sector"), "{}", pn06.joined());
    let pn05 = panel(&state, PanelId::InterceptPanel);
    assert!(
        pn05.says("This plan will not reach the approval queue: it tasks the area layer."),
        "{}",
        pn05.joined()
    );

    // Lifted: the area layer weapons free under a new baseline in force. The same raid's
    // plan clears the chain and waits for a person, and neither line is drawn.
    drop(state);
    let _ = std::fs::remove_dir_all(&dir);
    let (mut state, picture, dir) =
        desktop("lifted", WeaponsControlStatus::Free, point_then_area());
    set(
        &picture,
        vec![
            track(70, 30_000.0),
            track(71, 20_000.0),
            track(72, 10_000.0),
        ],
    );
    tick_at(&mut state, 1.0);
    assert_eq!(state.desk.approvals.queue().len(), 1, "{:?}", state.alerts);
    assert!(decisions::held_layers(&state).is_empty());
    let pn06 = panel(&state, PanelId::ApprovalQueue);
    assert!(!pn06.says("is at HOLD"), "{}", pn06.joined());
    assert!(pn06.says("1 waiting"), "{}", pn06.joined());
    let pn05 = panel(&state, PanelId::InterceptPanel);
    assert!(
        !pn05.says("will not reach the approval queue"),
        "{}",
        pn05.joined()
    );

    drop(state);
    let _ = std::fs::remove_dir_all(dir);
}

/// **While it is true.** A plan offered for decision ends the window: once one is, the
/// hold is no longer refusing *every* plan, and the line goes -- here because the raid
/// thins to one track, which the allocator's tie rule pairs with the later-declared
/// resource, the point effector, alone.
#[test]
fn the_line_goes_the_moment_a_plan_is_offered() {
    let (mut state, picture, dir) = desktop(
        "offered",
        WeaponsControlStatus::Hold,
        vec![effector(50, "area"), effector(40, "point")],
    );
    set(&picture, vec![track(71, 20_000.0), track(72, 10_000.0)]);
    tick_at(&mut state, 1.0);
    assert_eq!(
        decisions::held_layers(&state).len(),
        1,
        "{:?}",
        state.alerts
    );
    assert!(panel(&state, PanelId::ApprovalQueue).says("is at HOLD"));

    set(&picture, vec![track(72, 10_000.0)]);
    tick_at(&mut state, 2.0);
    let tasked: Vec<u32> = state
        .last_plan
        .assignments()
        .iter()
        .map(|(r, _)| r.0)
        .collect();
    assert_eq!(
        tasked,
        vec![40],
        "the lone track goes to the point effector"
    );
    assert_eq!(state.desk.approvals.queue().len(), 1, "{:?}", state.alerts);
    assert!(decisions::held_layers(&state).is_empty());
    let pn06 = panel(&state, PanelId::ApprovalQueue);
    assert!(!pn06.says("is at HOLD"), "{}", pn06.joined());
    assert!(pn06.says("1 waiting"), "{}", pn06.joined());

    drop(state);
    let _ = std::fs::remove_dir_all(dir);
}

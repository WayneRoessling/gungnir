// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Alternatives and what-if, wired to the desktop (GAP-032).
//!
//! `gungnir_decision::DecisionSupport` had no implementor anywhere in the workspace, and
//! a trait nobody implements is the least discoverable stub there is: nothing panics,
//! nothing errors, and no health flag is false. These are the tests behind the claim that
//! the desktop now answers the question -- that every option it offers carries a verdict
//! the *same four-engine chain* returned, and that a what-if changes nothing.
//!
//! They run against a real `AppState` over a scratch data directory, so the chain, the
//! allocator, the approval queue and the journal are the ones `update::tick` uses.

use gungnir_app::decisions;
use gungnir_app::state::AppState;
use gungnir_app::update;
use gungnir_command::ApprovalWorkflow;
use gungnir_config::{ConfigBaseline, ResourceConfig};
use gungnir_model::policy_settings::{AuthorityRule, WeaponsControlStatus};
use gungnir_model::{
    Classification, EffectorLayer, MissionTime, Provenance, Quality, Releasability, TrackId,
    TrackStatus, TrackView,
};
use gungnir_policy::PolicyVerdict;
use gungnir_tracking_service::{SubmitError, TrackingService};

const ORIGIN: [f64; 3] = [0.959_931, 0.209_440, 0.0]; // 55 N, 12 E in radians

/// A fixed picture, so the test exercises the decision path rather than the tracker.
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

/// One hostile track closing on the origin from `east_m`.
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

/// A desktop with three closing tracks and three point effectors.
///
/// **The resources and the tracks are numbered from zero on purpose.**
/// `gungnir_allocation::BellmanDpAllocator` returns the reward matrix's row and column
/// *indices* wrapped in `ResourceId` and `TrackId`, and `DpInterceptService` passes them
/// into the plan without mapping them back to the identifiers of the resources and tracks
/// it solved over. With any other numbering the plan names resources that do not exist --
/// the policy chain denies it `UnknownResource` -- and attributes each engagement to the
/// wrong track. Numbering from zero makes the indices and the identifiers coincide, so
/// these tests exercise decision support rather than that defect. It is reported with this
/// change and is not fixed here: it belongs to `gungnir-intercept-service`.
///
/// The two engines ahead of the fires check are satisfied deliberately -- the point layer
/// is weapons free and the operator holds decision authority on it -- so that what a test
/// reads is a verdict about *this plan* rather than a standing refusal that would be the
/// same whatever was proposed. `desktop_at` takes the control status so one test can hold
/// everything else fixed and change only the chain.
fn desktop_at(name: &str, status: WeaponsControlStatus) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-alternatives-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let mut config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        origin: Some(ORIGIN),
        resources: vec![effector(0), effector(1), effector(2)],
        // A one-step horizon, because the point of this fixture is that a plan exists to
        // have alternatives to. At the default horizon of ten the dynamic program values
        // deferring exactly as highly as acting -- there is no time preference in the
        // reward matrix -- and its tie-break keeps the first matching enumerated, which is
        // the empty one, so the plan is empty however many tracks are closing. That is
        // `gungnir-allocation`'s behaviour and not this module's; see the report filed
        // with this change.
        allocation_horizon: 1,
        ..ConfigBaseline::default()
    };
    config
        .policy
        .control_status
        .by_layer
        .insert(EffectorLayer::Point, status);
    config.policy.authority.rules.push(AuthorityRule {
        action: decisions::DECISION_ACTION.into(),
        role: "Operator".into(),
        layer: Some(EffectorLayer::Point),
        class: None,
        pre_delegated: false,
    });
    gungnir_config::validate(&config).expect("the baseline is valid");
    let mut state = AppState::with_config(config).expect("the desktop starts");
    state.tracking = Box::new(Picture(vec![
        track(0, 30_000.0),
        track(1, 20_000.0),
        track(2, 10_000.0),
    ]));
    (state, dir)
}

fn desktop(name: &str) -> (AppState, std::path::PathBuf) {
    desktop_at(name, WeaponsControlStatus::Free)
}

/// What a course of action says, apart from when it was computed: who is tasked against
/// what, the verdict it carries, and the explanation given for it.
type Substance = (
    Vec<(gungnir_model::ResourceId, TrackId)>,
    PolicyVerdict,
    String,
);

/// Everything about a set of courses of action except when it was computed.
///
/// The mission clock runs between two calls, so comparing the plans whole would compare
/// wall-clock timestamps and fail for a reason that has nothing to do with what the test
/// is asking. What has to be identical is the substance: who is tasked against what, the
/// verdict each option carries, and the explanation given for it.
fn summarise(courses: &[gungnir_decision::CourseOfAction]) -> Vec<Substance> {
    courses
        .iter()
        .map(|c| (c.plan.assignments(), c.policy_verdict, c.rationale.clone()))
        .collect()
}

/// **Every course of action carries a verdict the desktop's own chain returned**, which is
/// the first half of the `gungnir-decision` verification row.
///
/// The proof that the verdict is the chain's and not a constant is that changing the chain
/// changes every one of them: with the point layer weapons free the plans need a person,
/// and with it at hold the same plans against the same tracks are denied for that reason.
/// A default or an assumed pass would not move.
#[test]
fn every_alternative_carries_a_verdict_from_the_desktops_own_chain() {
    let (free, free_dir) = desktop("free");
    let free_courses = decisions::alternatives(&free, decisions::MAX_ALTERNATIVES);
    assert!(
        free_courses.len() > 1,
        "no alternatives were offered at all: {free_courses:#?}"
    );
    for course in &free_courses {
        assert_eq!(
            course.policy_verdict,
            PolicyVerdict::RequiresHumanApproval,
            "a course of action carries a verdict the free chain did not return: {}",
            course.rationale
        );
    }

    let (held, held_dir) = desktop_at("held", WeaponsControlStatus::Hold);
    let held_courses = decisions::alternatives(&held, decisions::MAX_ALTERNATIVES);
    assert_eq!(
        held_courses.len(),
        free_courses.len(),
        "the same picture offered a different number of options under a different chain"
    );
    for course in &held_courses {
        assert!(
            matches!(course.policy_verdict, PolicyVerdict::Denied { .. }),
            "a course of action was not denied by a chain that denies everything: {}",
            course.rationale
        );
    }
    let _ = std::fs::remove_dir_all(free_dir);
    let _ = std::fs::remove_dir_all(held_dir);
}

/// Each alternative is a genuinely different plan, and each says which resource it does
/// without.
#[test]
fn the_recommendation_leads_and_each_alternative_is_a_different_plan() {
    let (state, dir) = desktop("distinct");
    let courses = decisions::alternatives(&state, decisions::MAX_ALTERNATIVES);
    assert!(courses[0].rationale.starts_with("Recommended"));
    let primary = courses[0].plan.assignments();
    let mut seen = vec![primary.clone()];
    for alternative in &courses[1..] {
        assert!(
            alternative.rationale.contains("were unavailable"),
            "an alternative does not say what it does without: {}",
            alternative.rationale
        );
        let assignments = alternative.plan.assignments();
        assert!(
            !seen.contains(&assignments),
            "two courses of action are the same plan: {assignments:?}"
        );
        seen.push(assignments);
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// **The live state is unchanged after `what_if`**, which is the second half of the row.
///
/// Snapshotted before and compared after: the tracks, the resource pool, the plan in
/// force, the held alternatives, the approval queue, the decision record, the denial
/// history and the alerts. The last comparison is the one a field-by-field check would
/// miss -- re-running `alternatives` puts the whole chain and the allocator over the live
/// picture again, so anything the rehearsal moved would show up as a different answer.
#[test]
fn what_if_leaves_the_live_desktop_exactly_as_it_was() {
    let (mut state, dir) = desktop("unchanged");
    for _ in 0..3 {
        update::tick(&mut state);
    }

    let tracks_before: Vec<TrackView> = state.tracking.tracks().to_vec();
    let resources_before = state.resources.clone();
    let plan_before = state.last_plan.clone();
    let alternatives_before = state.alternatives.clone();
    let queued_before = state.approvals.queue().len();
    let records_before = state.approvals.records().len();
    let denials_before = state.denials.count;
    let alerts_before = state.alerts.clone();
    let recomputed_before = summarise(&decisions::alternatives(
        &state,
        decisions::MAX_ALTERNATIVES,
    ));

    // The hypothesis: the nearest track was never there.
    let hypothetical: Vec<TrackView> = state
        .tracking
        .tracks()
        .iter()
        .filter(|t| t.id != TrackId(2))
        .cloned()
        .collect();
    let course = decisions::what_if(&state, &hypothetical);
    assert!(
        course.rationale.contains("What if"),
        "a what-if that does not say so reads as a proposal: {}",
        course.rationale
    );

    assert_eq!(
        state.tracking.tracks(),
        tracks_before.as_slice(),
        "what_if changed the live picture"
    );
    assert_eq!(
        state.resources, resources_before,
        "what_if changed the pool"
    );
    assert_eq!(state.last_plan, plan_before, "what_if changed the plan");
    assert_eq!(
        state.alternatives, alternatives_before,
        "what_if changed the held alternatives"
    );
    assert_eq!(
        state.approvals.queue().len(),
        queued_before,
        "what_if queued something for a person to decide"
    );
    assert_eq!(
        state.approvals.records().len(),
        records_before,
        "what_if left a decision in the record"
    );
    assert_eq!(state.denials.count, denials_before);
    assert_eq!(state.alerts, alerts_before, "what_if raised an alert");
    assert_eq!(
        summarise(&decisions::alternatives(
            &state,
            decisions::MAX_ALTERNATIVES,
        )),
        recomputed_before,
        "the recommendation changed after a what-if, so something live moved"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// The tick holds the alternatives to the plan it proposed, so PN-05 cannot draw options
/// for one plan beside a different one.
#[test]
fn the_tick_holds_alternatives_for_the_plan_in_force() {
    let (mut state, dir) = desktop("tick");
    for _ in 0..3 {
        update::tick(&mut state);
    }
    assert!(
        !state.alternatives.is_empty(),
        "the tick proposed a plan and held no recommendation for it"
    );
    assert_eq!(
        state.alternatives[0].plan.assignments(),
        state.last_plan.assignments(),
        "the head of the list is not the plan in force"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// The rehearsal is drawn only for a hypothesis the operator posed.
///
/// Nothing is selected at start, so there is no what-if; selecting a track poses the
/// question "what if we lost this one" without needing an input surface this build does
/// not have, and the answer plans against the picture minus that track.
#[test]
fn a_rehearsal_appears_only_once_a_track_is_selected() {
    let (mut state, dir) = desktop("selection");
    update::tick(&mut state);
    assert!(
        state.what_if.is_none(),
        "a hypothesis appeared that nobody posed"
    );

    state.select_track(TrackId(2));
    update::tick(&mut state);
    let course = state
        .what_if
        .as_ref()
        .expect("a selected track poses the question");
    assert!(
        course.rationale.contains("2 hypothetical track"),
        "the rehearsal did not drop the selected track: {}",
        course.rationale
    );
    assert!(
        matches!(
            course.policy_verdict,
            PolicyVerdict::RequiresHumanApproval | PolicyVerdict::Denied { .. }
        ),
        "the rehearsal carries no verdict at all"
    );
    // Still committed to nothing.
    assert!(
        state.approvals.queue().is_empty() || state.approvals.records().is_empty(),
        "the rehearsal reached the queue"
    );
    let _ = std::fs::remove_dir_all(dir);
}

// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Plan validity: a plan produced under a baseline outside its window is superseded
//! rather than applied (GAP-052, DN-08 §5, the CAP-5.6 verification row).
//!
//! `ValidityWindow` and `ConfigBaseline::is_promotable_at` shipped with DN-08 and
//! **nothing ever called them**: the window was displayed in PN-14 and enforced nowhere,
//! so an expired baseline stayed in force and went on producing plans that were queued
//! for approval like any other. These are the tests behind the claim that it is enforced.
//!
//! The property worth protecting is the one the design states outright: expiry never
//! changes a picture retroactively, so this is a gate on what happens *now*, not a
//! rewriting of what was already recorded.

use gungnir_app::decisions::{self, Submitted};
use gungnir_app::state::AppState;
use gungnir_command::ApprovalWorkflow;
use gungnir_config::ConfigBaseline;
use gungnir_eventing::{Envelope, Event};
use gungnir_model::events::InterceptEvent;
use gungnir_model::{MissionTime, PlanId, PlanView, ValidityWindow};
use gungnir_ui::panels::status_strip::BaselineValidity;

/// A window, in Unix seconds, relative to now: the desktop runs on a wall clock.
fn window(from_offset_s: f64, until_offset_s: Option<f64>) -> ValidityWindow {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or_default();
    ValidityWindow {
        valid_from: MissionTime(now + from_offset_s),
        valid_until: until_offset_s.map(|o| MissionTime(now + o)),
    }
}

fn desktop(name: &str, validity: Option<ValidityWindow>) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-validity-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        validity,
        ..ConfigBaseline::default()
    };
    (
        AppState::with_config(config).expect("the desktop starts"),
        dir,
    )
}

fn a_plan() -> PlanView {
    PlanView {
        id: PlanId(1),
        ..PlanView::default()
    }
}

/// **The criterion.** A plan produced under an expired baseline is superseded, never
/// queued, and the supersession is on the bus rather than being a gap in the record.
#[test]
fn a_plan_under_an_expired_baseline_is_superseded_and_never_queued() {
    let (mut state, dir) = desktop("expired", Some(window(-1000.0, Some(-10.0))));
    let events = state.events.subscribe();

    let outcome = decisions::submit(&mut state, a_plan());

    assert_eq!(outcome, Submitted::Superseded);
    assert!(
        state.approvals.pending().is_empty(),
        "a superseded plan reached the approval queue"
    );

    let published: Vec<Envelope> = events.try_iter().collect();
    assert!(
        published
            .iter()
            .any(|e| matches!(e.event, Event::Intercept(InterceptEvent::PlanSuperseded(_)))),
        "the supersession left no trace on the bus: {published:?}"
    );
    assert!(
        state.alerts.iter().any(|a| a.contains("superseded")),
        "nothing told the operator why the queue stopped filling: {:?}",
        state.alerts
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// **Superseded is not denied.** An empty plan is exactly what the policy chain refuses,
/// and under an expired baseline it is not evaluated at all -- so no refusal nobody made
/// goes into the denial record.
#[test]
fn a_superseded_plan_is_not_evaluated_and_records_no_denial() {
    let (mut state, dir) = desktop("not-denied", Some(window(-1000.0, Some(-10.0))));

    // The same plan under a baseline with no window *is* denied, which is the control.
    let (mut ordinary, ordinary_dir) = desktop("control", None);
    assert!(
        matches!(
            decisions::submit(&mut ordinary, a_plan()),
            Submitted::Evaluated(_)
        ),
        "the control plan was not evaluated, so this test proves nothing"
    );
    assert_eq!(ordinary.denials.count, 1, "the control plan was not denied");

    assert_eq!(
        decisions::submit(&mut state, a_plan()),
        Submitted::Superseded
    );
    assert_eq!(
        state.denials.count, 0,
        "a superseded plan was recorded as denied by a policy that never ran"
    );
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(ordinary_dir);
}

/// A baseline whose window has not opened supersedes too, and for the same reason.
#[test]
fn a_plan_under_a_baseline_not_yet_in_force_is_superseded() {
    let (mut state, dir) = desktop("not-yet", Some(window(3600.0, None)));
    assert_eq!(
        decisions::submit(&mut state, a_plan()),
        Submitted::Superseded
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// Inside its window nothing changes: the plan is evaluated exactly as before.
#[test]
fn a_plan_inside_the_window_is_evaluated_normally() {
    let (mut state, dir) = desktop("inside", Some(window(-10.0, Some(3600.0))));
    assert!(matches!(
        decisions::submit(&mut state, a_plan()),
        Submitted::Evaluated(_)
    ));
    let _ = std::fs::remove_dir_all(dir);
}

/// **Four states, not a boolean.** "No window configured" is not the same claim as
/// "valid", and "not yet" is not "expired" -- an operator whose plans have started being
/// superseded has to be able to tell which happened.
#[test]
fn the_four_validity_states_are_kept_apart() {
    let cases = [
        ("none", None, "NoWindowConfigured"),
        ("open", Some(window(-10.0, None)), "InForce"),
        ("bounded", Some(window(-10.0, Some(3600.0))), "InForce"),
        ("early", Some(window(3600.0, None)), "NotYet"),
        ("late", Some(window(-1000.0, Some(-10.0))), "Expired"),
    ];
    for (name, validity, expected) in cases {
        let (state, dir) = desktop(name, validity);
        let actual = gungnir_app::status::baseline_validity(&state);
        let matched = matches!(
            (expected, actual),
            ("NoWindowConfigured", BaselineValidity::NoWindowConfigured)
                | ("InForce", BaselineValidity::InForce { .. })
                | ("NotYet", BaselineValidity::NotYet { .. })
                | ("Expired", BaselineValidity::Expired { .. })
        );
        assert!(matched, "{name}: expected {expected}, got {actual:?}");
        assert_eq!(
            actual.supersedes_plans(),
            matches!(expected, "NotYet" | "Expired"),
            "{name}: the wrong states supersede plans"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}

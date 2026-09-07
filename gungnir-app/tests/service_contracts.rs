// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The two service contracts say what they are returning (GAP-066).
//!
//! `submit_detection` returned `()`, so the ingest gateway counted a detection as accepted
//! and published `IngestEvent::Accepted` **before knowing whether the pipeline took it**.
//! `plan` returned a bare `PlanView`, so the last good plan came back looking exactly like
//! one computed for the picture in front of the operator.
//!
//! Both were contracts an adapter written under GAP-001 would have been built against.

use gungnir_intercept_service::{DpInterceptService, InterceptService, PlanOutcome};
use gungnir_model::MissionTime;

/// **A stale plan is not a fresh one**, and the type says which without a health flag on a
/// different panel.
#[test]
fn a_plan_says_whether_it_was_computed_for_this_snapshot() {
    let mut svc = DpInterceptService::new(10);
    // Nothing to solve: a correct, fresh answer for this snapshot.
    let outcome = svc.plan(MissionTime(1.0), &[], &[]);
    assert!(outcome.is_fresh(), "{outcome:?}");
    assert!(
        outcome.plan().expect("a plan").is_empty(),
        "an empty plan for an empty sector is the right answer"
    );
}

/// **The distinction that matters most**: no plan at all is not an empty plan. One says
/// nobody could compute a recommendation; the other says the recommendation is to do
/// nothing.
#[test]
fn no_plan_and_an_empty_plan_are_different_answers() {
    let empty = PlanOutcome::Fresh(gungnir_model::PlanView::default());
    let none = PlanOutcome::NoPlan {
        reason: "the allocator reported itself unimplemented".to_owned(),
    };
    assert!(empty.plan().is_some());
    assert!(none.plan().is_none());
    assert!(empty.is_fresh());
    assert!(!none.is_fresh());
}

/// The desktop does not propose a plan it was not given fresh. Publishing `PlanProposed`
/// for a stale plan would put a recommendation in the journal that nothing recommended at
/// that moment.
#[test]
fn the_desktop_proposes_nothing_while_the_allocator_is_unimplemented() {
    use gungnir_eventing::Event;
    use gungnir_model::events::InterceptEvent;

    let dir =
        std::env::temp_dir().join(format!("gungnir-contracts-propose-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = gungnir_config::ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        ..gungnir_config::ConfigBaseline::default()
    };
    let mut state = gungnir_app::state::AppState::with_config(config).expect("desktop");
    let events = state.events.subscribe();

    for _ in 0..3 {
        gungnir_app::update::tick(&mut state);
    }

    let proposed = events
        .try_iter()
        .filter(|e| matches!(e.event, Event::Intercept(InterceptEvent::PlanProposed(_))))
        .count();
    assert_eq!(
        proposed, 0,
        "a plan was proposed while the allocator cannot compute one"
    );
    let _ = std::fs::remove_dir_all(dir);
}

// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The policy chain's verdict is on the record (GAP-028).
//!
//! Every plan the desktop submits leaves `PlanEvaluated` on the bus, naming the engines
//! that ran, whatever the verdict was. A denial that reached only a counter would be a
//! decision nobody could review.

use gungnir_app::decisions::{self, DESKTOP_ENGINES};
use gungnir_app::state::AppState;
use gungnir_config::ConfigBaseline;
use gungnir_eventing::Event;
use gungnir_model::events::InterceptEvent;
use gungnir_model::{PlanId, PlanView};

#[test]
fn every_submitted_plan_leaves_its_verdict_and_engines_on_the_record() {
    let dir = std::env::temp_dir().join(format!("gungnir-policy-record-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    };
    let mut state = AppState::with_config(config).expect("the desktop starts");
    let events = state.events.subscribe();

    // An empty plan: the chain denies it (nothing is tasked), and the denial is recorded.
    let _ = decisions::submit(
        &mut state,
        PlanView {
            id: PlanId(9),
            ..PlanView::default()
        },
    );
    let evaluated: Vec<(PlanId, Vec<String>)> = events
        .try_iter()
        .filter_map(|e| match e.event {
            Event::Intercept(InterceptEvent::PlanEvaluated { plan, engines, .. }) => {
                Some((plan, engines))
            }
            _ => None,
        })
        .collect();
    assert_eq!(evaluated.len(), 1, "one evaluation per submission");
    assert_eq!(evaluated[0].0, PlanId(9));
    assert_eq!(
        evaluated[0].1,
        DESKTOP_ENGINES
            .iter()
            .map(|e| (*e).to_string())
            .collect::<Vec<_>>()
    );
    let _ = std::fs::remove_dir_all(dir);
}

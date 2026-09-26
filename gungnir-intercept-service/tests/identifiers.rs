// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! DN-31 §9 row 1, the planner's half (GAP-130; D-56): "No two identifiers equal across
//! machines and restarts".
//!
//! `DpInterceptService` numbered its plans from 1, and it is built in more places than one
//! might expect: by each binary at start, by a desktop falling back from its node, and once
//! per call for every alternative and what-if `gungnir-decision` solves. Each of those
//! started again at plan 1. Separate planners in one process stand in for the machines and
//! the restarts here, as separate workflows do for the queue in `gungnir-command`.

use gungnir_intercept_service::{
    DpInterceptService, InterceptService, MissionTime, PlanId, PlanOutcome, ResourceId,
    ResourceView, SteppedClock, TrackId, TrackView,
};
use gungnir_model::{Classification, Geodetic, Provenance, Quality, Releasability, TrackStatus};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

/// A planner whose clock stands still, so every solve fits its budget whatever the
/// machine's load: this test is about identifiers, not about time (GAP-164).
fn planner(horizon: usize) -> DpInterceptService {
    DpInterceptService::new(horizon).with_clock(Arc::new(SteppedClock::new(Duration::ZERO)))
}

fn track(id: u64, east_m: f64) -> TrackView {
    TrackView {
        id: TrackId(id),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::<f64, 6>::new(east_m, 1_000.0, 100.0, -30.0, 0.0, 0.0),
        covariance: nalgebra::SMatrix::<f64, 6, 6>::identity(),
        classification: Classification::Hostile,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(0.0),
        releasability: Releasability::default(),
    }
}

fn resource() -> ResourceView {
    ResourceView {
        id: ResourceId(1),
        position: Geodetic {
            lat_rad: 0.96,
            lon_rad: 0.21,
            alt_m: 0.0,
        },
        capacity: 1,
        ready: true,
        layer: gungnir_model::EffectorLayer::Point,
        cost: gungnir_model::RelativeCost::default(),
        magazine: None,
        intercept_speed_mps: None,
    }
}

/// Plan once against the picture `tracks`, which is a new plan whenever the assignment
/// changes.
fn propose(planner: &mut DpInterceptService, at: f64, tracks: &[TrackView]) -> PlanId {
    match planner.plan(MissionTime(at), tracks, &[resource()]) {
        PlanOutcome::Fresh(plan) => plan.id,
        other => panic!("no fresh plan: {other:?}"),
    }
}

/// DN-31 §9 row 1: a node's planner, two desktops', a desktop's after a restart, and the
/// planner built per question for an alternative mint plans in turn, and **no two plans
/// share an identifier**, each is a UUID v7, and in minting order each is greater than the
/// last. `PlanId(0)` is minted by none of them: it stays the identifier of
/// `PlanView::default()` alone.
#[test]
fn separate_planners_standing_in_for_machines_and_restarts_never_mint_the_same_plan_identifier() {
    let mut node = planner(4);
    let mut desktop_a = planner(4);
    let mut desktop_b = planner(4);
    let mut minted: Vec<PlanId> = Vec::new();

    for round in 0..30_u32 {
        let at = f64::from(round);
        // The picture alternates between two tracks, so every planner's assignment changes
        // every round and every round mints a plan on every planner.
        let picture = [track(u64::from(round % 2) + 1, 8_000.0)];
        if round == 15 {
            desktop_a = planner(4); // a restart
        }
        for planner in [&mut node, &mut desktop_a, &mut desktop_b] {
            minted.push(propose(planner, at, &picture));
        }
        // An alternative is solved on a planner built for the one question.
        minted.push(propose(&mut planner(4), at, &picture));
    }

    let values: Vec<u128> = minted.iter().map(|p| p.0).collect();
    let distinct: HashSet<u128> = values.iter().copied().collect();
    assert_eq!(
        distinct.len(),
        values.len(),
        "two plans shared an identifier"
    );
    assert!(
        !distinct.contains(&0),
        "a planner minted PlanId(0), the no-plan identifier"
    );
    assert!(
        values
            .iter()
            .all(|v| uuid::Uuid::from_u128(*v).get_version() == Some(uuid::Version::SortRand)),
        "a plan identifier is not a UUID v7"
    );
    assert!(
        values.windows(2).all(|w| w[0] < w[1]),
        "plan identifiers do not sort in the order they were minted"
    );
}

// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Differential test for the `docs/verification-capability-table.md` §1 row
//! "`filters` | RTS smoother / fixed-lag smoothing".
//!
//! Oracle `filterpy.kalman.rts_smoother` 1.4.5. Pass criterion: **relative error < 1e-6**.
//!
//! The forward pass in the fixture is the already-gated linear filter on a position
//! measurement, so this compares the backward recursion alone rather than a filter and a
//! smoother at once. Every step of the smoothed trajectory is checked, not only the
//! endpoints: a smoother that agreed at both ends and not in the middle would be exactly
//! the failure a fixed-lag consumer would hit and never see.
//!
//! The forward states in the fixture are also fed straight back in, so a disagreement
//! cannot come from this test re-running a filter differently from the oracle's.

use gungnir_core::ConstantVelocity;
use gungnir_filters::{rts_smooth, Smoothed};
use nalgebra::{SMatrix, SVector};

/// The row's tolerance, verbatim.
const TOL: f64 = 1e-6;

#[derive(serde::Deserialize)]
struct Snapshot {
    x: Vec<f64>,
    p: Vec<Vec<f64>>,
}

#[derive(serde::Deserialize)]
struct Case {
    name: String,
    dt: f64,
    sigma_a_sq: f64,
    forward: Vec<Snapshot>,
    smoothed: Vec<Snapshot>,
}

#[derive(serde::Deserialize)]
struct Fixture {
    oracle: String,
    filterpy: String,
    cases: Vec<Case>,
}

fn snapshot_to_smoothed(s: &Snapshot) -> Smoothed<6> {
    Smoothed {
        state: SVector::<f64, 6>::from_column_slice(&s.x),
        covariance: SMatrix::<f64, 6, 6>::from_fn(|r, c| s.p[r][c]),
    }
}

#[test]
fn the_smoother_matches_filterpy_at_every_step() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/oracles/filters/rts.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let fixture: Fixture = serde_json::from_str(&text).expect("the fixture parses");
    assert!(
        fixture.oracle.contains("rts_smoother"),
        "the fixture names its oracle: {}",
        fixture.oracle
    );
    assert_eq!(fixture.filterpy, "1.4.5", "the pinned oracle version");
    assert!(!fixture.cases.is_empty());

    for case in &fixture.cases {
        let forward: Vec<Smoothed<6>> = case.forward.iter().map(snapshot_to_smoothed).collect();
        let ours = rts_smooth(
            &forward,
            &ConstantVelocity {
                sigma_a_sq: case.sigma_a_sq,
            },
            case.dt,
        )
        .unwrap_or_else(|e| panic!("{}: {e}", case.name));

        assert_eq!(ours.len(), case.smoothed.len(), "{}: length", case.name);
        for (k, (mine, theirs)) in ours.iter().zip(&case.smoothed).enumerate() {
            let their_x = SVector::<f64, 6>::from_column_slice(&theirs.x);
            let their_p = SMatrix::<f64, 6, 6>::from_fn(|r, c| theirs.p[r][c]);
            let dx = (mine.state - their_x).norm() / their_x.norm().max(1.0);
            assert!(
                dx < TOL,
                "{} step {k}: state relative error {dx} over {TOL}\nours   {}\noracle {}",
                case.name,
                mine.state.transpose(),
                their_x.transpose()
            );
            let dp = (mine.covariance - their_p).norm() / their_p.norm().max(1.0);
            assert!(
                dp < TOL,
                "{} step {k}: covariance relative error {dp} over {TOL}",
                case.name
            );
        }

        // The smoother must actually do something: a pass that returned its input would
        // agree with nothing and this test would still need to catch it.
        let moved = ours
            .iter()
            .zip(&forward)
            .any(|(a, b)| (a.state - b.state).norm() > 1.0);
        assert!(
            moved,
            "{}: the smoothed trajectory is the forward one, so nothing was smoothed",
            case.name
        );
        // And the last estimate is the forward one, because there is nothing after it.
        let last = ours.len() - 1;
        assert!((ours[last].state - forward[last].state).norm() < 1e-12);
    }
}

#[test]
fn an_empty_trajectory_smooths_to_an_empty_one() {
    let out = rts_smooth::<ConstantVelocity, 6>(&[], &ConstantVelocity { sigma_a_sq: 1.0 }, 1.0)
        .expect("no error");
    assert!(out.is_empty());
}

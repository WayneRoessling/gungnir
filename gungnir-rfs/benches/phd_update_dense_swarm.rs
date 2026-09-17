// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Benchmark group `phd_update_dense_swarm` (agentic-coding-standards.md §2.6).
//!
//! **Real as of 2026-09-16.** Until then this built a 200-element `Vec<f64>` and
//! returned its length: a placeholder from before the GM-PHD filter existed, left in
//! place after it was implemented. It measured about 0.75 ns -- a length the optimiser
//! could see through -- and Gate 6 compared that between runs as if it were a filter; six runs of unchanged benchmark code on 2026-09-16 put its ratio
//! anywhere from 1.07 to 3.00.
//!
//! What it measures now is one scan of a Scenario 4-shaped swarm: `PhdFilter::predict`
//! then `update`, including its prune and merge, against 200 targets on a 20 x 10 grid
//! 50 m apart, flying together at 15 m/s, observed with 5 m position noise and 20
//! clutter detections. The filter is first run for five scans from a birth at every
//! target, so the measured scan starts from a settled mixture of about 200 components
//! rather than from an empty one, and the setup refuses to run if it has not settled
//! there -- a mixture that had collapsed would make this a benchmark of pruning.
//!
//! `max_components` is 400, not the default 100: the default would truncate a
//! 200-target swarm to half of itself on every scan, and this would measure the
//! truncation. The other settings are the defaults `tests/phd_diff.rs` uses.
//!
//! Inputs come from fixed arithmetic rather than an `Rng`, for the reason
//! `gungnir-association`'s benchmark gives, and not from `gungnir-scenario`, which
//! depends on this crate through `gungnir-fusion-async`.
//!
//! Per `../../benches/README.md` no benchmark asserts against an absolute number; the
//! regression gate compares against the previous baseline.

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use gungnir_rfs::{GaussianComponent, PhdFilter, PhdSettings};
use gungnir_track::ConstantVelocity;
use nalgebra::{SMatrix, SVector};

const COLUMNS: u32 = 20;
const ROWS: u32 = 10;
const SPACING_M: f64 = 50.0;
const DT: f64 = 1.0;
const WARM_UP_SCANS: u32 = 5;
const CLUTTER: u32 = 20;

/// A deterministic value in `[-1, 1)`, with no structure a filter could lock onto.
fn jitter(scan: u32, target: u32, axis: u32) -> f64 {
    let mixed = scan.wrapping_mul(2_654_435_761)
        ^ target.wrapping_mul(2_246_822_519)
        ^ axis.wrapping_mul(3_266_489_917);
    f64::from(mixed % 20_001) / 10_000.0 - 1.0
}

/// Every target's true position at `scan`: a grid 2 km out, moving east together.
fn truth(scan: u32) -> Vec<SVector<f64, 3>> {
    let mut positions = Vec::new();
    for row in 0..ROWS {
        for column in 0..COLUMNS {
            positions.push(SVector::<f64, 3>::new(
                2_000.0 + f64::from(column) * SPACING_M + 15.0 * f64::from(scan) * DT,
                3_000.0 + f64::from(row) * SPACING_M,
                500.0,
            ));
        }
    }
    positions
}

/// What the sensor reports at `scan`: every target with 5 m of noise, then the clutter
/// scattered across the swarm's extent.
fn detections(scan: u32) -> Vec<SVector<f64, 3>> {
    let mut out: Vec<SVector<f64, 3>> = truth(scan)
        .into_iter()
        .zip(0_u32..)
        .map(|(p, target)| {
            p + SVector::<f64, 3>::new(
                5.0 * jitter(scan, target, 0),
                5.0 * jitter(scan, target, 1),
                5.0 * jitter(scan, target, 2),
            )
        })
        .collect();
    for k in 0..CLUTTER {
        let id = 10_000 + k;
        out.push(SVector::<f64, 3>::new(
            2_500.0 + 600.0 * jitter(scan, id, 0),
            3_250.0 + 400.0 * jitter(scan, id, 1),
            500.0 + 50.0 * jitter(scan, id, 2),
        ));
    }
    out
}

fn diagonal<const N: usize>(d: [f64; N]) -> SMatrix<f64, N, N> {
    SMatrix::<f64, N, N>::from_diagonal(&SVector::<f64, N>::from(d))
}

fn position_h() -> SMatrix<f64, 3, 6> {
    let mut h = SMatrix::<f64, 3, 6>::zeros();
    for axis in 0..3 {
        h[(axis, axis)] = 1.0;
    }
    h
}

fn phd_update_dense_swarm(c: &mut Criterion) {
    let motion = ConstantVelocity { sigma_a_sq: 1.0 };
    let mut settled = PhdFilter::new(
        PhdSettings {
            max_components: 400,
            ..PhdSettings::default()
        },
        position_h(),
        diagonal([25.0, 25.0, 25.0]),
    )
    .expect("the benchmark describes a valid scene");

    let births: Vec<GaussianComponent> = truth(0)
        .into_iter()
        .map(|p| GaussianComponent {
            weight: 1.0,
            mean: SVector::<f64, 6>::new(p[0], p[1], p[2], 0.0, 0.0, 0.0),
            cov: diagonal([25.0, 25.0, 25.0, 400.0, 400.0, 400.0]),
        })
        .collect();
    for scan in 0..WARM_UP_SCANS {
        let born: &[GaussianComponent] = if scan == 0 { &births } else { &[] };
        settled
            .predict(&motion, DT, born)
            .expect("a warm-up scan predicts");
        settled
            .update(&detections(scan))
            .expect("a warm-up scan updates");
    }
    let cardinality = settled.cardinality();
    assert!(
        (180.0..=220.0).contains(&cardinality) && settled.component_count() >= 180,
        "the warm-up did not settle on the swarm (cardinality {cardinality:.1}, {} \
         components), so this would not measure a dense-swarm scan",
        settled.component_count()
    );

    let scan = detections(WARM_UP_SCANS);
    c.bench_function("phd_update_dense_swarm", |b| {
        b.iter_batched(
            || settled.clone(),
            |mut filter| {
                filter
                    .predict(&motion, DT, &[])
                    .expect("the measured scan predicts");
                filter
                    .update(black_box(&scan))
                    .expect("the measured scan updates");
                black_box(filter.cardinality())
            },
            BatchSize::LargeInput,
        );
    });
}

criterion_group!(benches, phd_update_dense_swarm);
criterion_main!(benches);

// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Benchmark group `hungarian_solve_100x100` (agentic-coding-standards.md §2.6),
//! named after the verification-capability-table.md §1 row
//! "Hungarian / Jonker-Volgenant".
//!
//! Real as of 2026-09-05: `solve_assignment` is implemented, so this measures the
//! solver rather than the cost-matrix construction that stood in for it.
//!
//! Three shapes are measured because the algorithm's cost is `O(n²m)` and association
//! rarely hands it a square matrix. The dense-swarm case in
//! `docs/scenario-crate-narrative.md` Scenario 4 is the one that decides whether the
//! DP solve has to move off the frame thread, so the 200-track shape is here too.
//!
//! Per `../../benches/README.md` no benchmark asserts against an absolute number; the
//! regression gate compares against the previous release's baseline.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use gungnir_association::solve_assignment;
use nalgebra::DMatrix;

/// A deterministic cost matrix with no accidental structure.
///
/// The multiplicative-hash pattern the placeholder used produced a matrix with a
/// strong diagonal regularity, which the solver finishes far too easily; this mixes
/// the row and column harder so the augmenting paths are of realistic length. Fixed
/// arithmetic rather than an `Rng` keeps the input identical between runs, which is
/// what a regression comparison needs.
#[allow(clippy::cast_precision_loss)]
fn cost_matrix(rows: usize, cols: usize) -> DMatrix<f64> {
    DMatrix::<f64>::from_fn(rows, cols, |r, c| {
        let mixed = (r.wrapping_mul(2_654_435_761) ^ c.wrapping_mul(2_246_822_519))
            .wrapping_add(r.wrapping_mul(c));
        ((mixed % 100_003) as f64) / 100.0
    })
}

fn hungarian_solve(c: &mut Criterion) {
    let mut group = c.benchmark_group("hungarian_solve_100x100");
    for (rows, cols) in [(100, 100), (100, 400), (200, 200)] {
        let cost = cost_matrix(rows, cols);
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{rows}x{cols}")),
            &cost,
            |b, cost| {
                b.iter(|| {
                    let assignment =
                        solve_assignment(black_box(cost)).expect("well-formed cost matrix");
                    black_box(assignment.total_cost)
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, hungarian_solve);
criterion_main!(benches);

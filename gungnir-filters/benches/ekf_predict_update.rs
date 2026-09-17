// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Benchmark group `ekf_predict_update`, named after the capability-table row it
//! measures (agentic-coding-standards.md §2.6).
//!
//! **Real as of 2026-09-16.** Until then this timed `black_box(0_u64)`: a placeholder
//! written while the EKF was a trait surface, left in place after
//! `ExtendedKalmanFilter` was implemented. It measured about 0.4 ns, which is the
//! timer and nothing else, and Gate 6 compared that number between runs as if it were
//! a filter; six runs of unchanged benchmark code on 2026-09-16 put its ratio anywhere
//! from 0.91 to 1.91.
//!
//! What it measures now is one track's worth of filtering: a six-state
//! constant-velocity EKF with a range/azimuth/elevation radar at the ENU origin, run
//! through 100 predict-and-update cycles of a Scenario 1-shaped flight -- 40 s straight,
//! a 3 deg/s turn for 30 s, 30 s straight, at 250 m/s about 30 km out. The model is
//! the one `tests/nonlinear_diff.rs` checks against filterpy.
//!
//! The measurements are built once, from fixed arithmetic rather than an `Rng`, for the
//! reason `gungnir-association`'s benchmark gives: a regression comparison needs the
//! input identical between runs. They are the noiseless measurement of the truth plus a
//! fixed perturbation (about 10 m in range, 1 mrad in each angle), so every update has
//! a real innovation to work with. `gungnir-scenario` is not used because it would be
//! a new edge into this crate from a crate that already depends on it.
//!
//! Per `../../benches/README.md` no benchmark asserts against an absolute number; the
//! regression gate compares against the previous baseline.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use gungnir_core::ConstantVelocity;
use gungnir_filters::{ExtendedKalmanFilter, Filter, MeasurementModel, RangeAzimuthElevation};
use nalgebra::{SMatrix, SVector};

const SCANS: u32 = 100;
const DT: f64 = 1.0;

/// A deterministic value in `[-1, 1)` for scan `i` and channel `k`, with no structure a
/// filter could lock onto.
fn jitter(i: u32, k: u32) -> f64 {
    let mixed = i.wrapping_mul(2_654_435_761) ^ k.wrapping_mul(2_246_822_519);
    f64::from(mixed % 20_001) / 10_000.0 - 1.0
}

/// The truth state for every scan, and the measurement a sensor at the origin reports.
fn flight(sensor: &RangeAzimuthElevation) -> (SVector<f64, 6>, Vec<SVector<f64, 3>>) {
    let start =
        SVector::<f64, 6>::from_column_slice(&[20_000.0, 22_000.0, 9_000.0, -250.0, 0.0, 0.0]);
    let turn_rate = 3.0_f64.to_radians() * DT;
    let mut truth = start;
    let mut measurements = Vec::new();
    for i in 0..SCANS {
        if (40..70).contains(&i) {
            let (ve, vn) = (truth[3], truth[4]);
            truth[3] = ve * turn_rate.cos() - vn * turn_rate.sin();
            truth[4] = ve * turn_rate.sin() + vn * turn_rate.cos();
        }
        for axis in 0..3 {
            truth[axis] += truth[3 + axis] * DT;
        }
        let mut z = sensor.predict_measurement(&truth);
        z[0] += 10.0 * jitter(i, 0);
        z[1] += 1e-3 * jitter(i, 1);
        z[2] += 1e-3 * jitter(i, 2);
        measurements.push(z);
    }
    (start, measurements)
}

fn diagonal<const N: usize>(d: [f64; N]) -> SMatrix<f64, N, N> {
    SMatrix::<f64, N, N>::from_diagonal(&SVector::<f64, N>::from(d))
}

fn ekf_predict_update(c: &mut Criterion) {
    let sensor = RangeAzimuthElevation::at([0.0, 0.0, 0.0]);
    let (start, measurements) = flight(&sensor);
    let initial = ExtendedKalmanFilter::new(
        start,
        diagonal([2_500.0, 2_500.0, 2_500.0, 400.0, 400.0, 400.0]),
        ConstantVelocity { sigma_a_sq: 25.0 },
        sensor,
        diagonal([100.0, 1e-6, 1e-6]),
    );

    c.bench_function("ekf_predict_update", |b| {
        b.iter(|| {
            let mut filter = initial.clone();
            for z in black_box(&measurements) {
                filter.predict(DT);
                filter.update(z);
            }
            black_box(*filter.state())
        });
    });
}

criterion_group!(benches, ekf_predict_update);
criterion_main!(benches);

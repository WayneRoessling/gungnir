//! Benchmark group `ekf_predict_update`, named after the capability-table row it
//! measures (agentic-coding-standards.md §2.6). Inputs come from `gungnir-scenario`
//! Scenario 1 once the generator exists; until the EKF has a constructor this
//! measures only the harness overhead so `bench-regression.yml` has a baseline.

use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn ekf_predict_update(c: &mut Criterion) {
    c.bench_function("ekf_predict_update", |b| {
        b.iter(|| {
            // Placeholder: replace with ExtendedKalmanFilter::predict + update over
            // a Scenario 1 measurement sequence.
            black_box(0_u64)
        });
    });
}

criterion_group!(benches, ekf_predict_update);
criterion_main!(benches);

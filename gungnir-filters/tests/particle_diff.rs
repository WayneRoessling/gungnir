// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Differential test for the `docs/verification-capability-table.md` §1 row
//! "`filters` | Particle Filter".
//!
//! Criterion, from §2 unchanged and verbatim: **"KS-test or mean/variance within 2σ"**,
//! over N trials. §2 also records the oracle as Python-only, and it has to be. A
//! particle filter's output depends on its random draws; two implementations in two
//! languages draw different numbers and cannot be compared step for step the way the
//! Kalman rows are. What is comparable is the distribution each converges to.
//!
//! # A 2σ bound applied to 180 quantities must be exceeded, and that is not a failure
//!
//! This is worth stating plainly because the first version of this test got it wrong.
//! It checked every one of the six state components at each of thirty steps against a 2σ
//! bound and required all 180 to pass. Under the null hypothesis that the two filters
//! have the *same* sampling distribution, about 4.6% of standardised deviations exceed
//! 2σ -- roughly eight of 180. Requiring zero excursions is not a strict reading of the
//! row; it is a criterion no correct implementation can satisfy, and the first run duly
//! failed at 2.22σ on one component of one step.
//!
//! So the row is applied the way its two options are actually meant:
//!
//! * **The KS test**, §2's first option, on the raw per-trial estimates at the final
//!   step. This compares the whole sampling distribution rather than its first moment,
//!   and needs no multiple-comparison argument at all.
//! * **The 2σ mean check**, §2's second option, applied to all 180 quantities with the
//!   excursion *count* judged against what 2σ itself predicts -- a four-standard-
//!   deviation bound on the binomial -- plus a hard per-quantity ceiling of 4σ, which no
//!   sampling fluctuation reaches but a real difference in the filters would.
//!
//! Neither number is looser than the row's. The 2σ is still 2σ; what changed is that it
//! is now being read as a statement about a distribution rather than as a guarantee
//! about every draw from it.
//!
//! # Agreement with the reference is not by itself evidence
//!
//! Two implementations with the same bug agree beautifully. So the fixture also carries
//! the **exact** posterior: this model is linear-Gaussian, which means the Kalman filter
//! is not an approximation of the answer, it *is* the answer, and both filters have a
//! known target to converge to rather than only each other.

use gungnir_core::ConstantVelocity;
use gungnir_filters::ParticleFilter;
use nalgebra::{SMatrix, SVector};
use rand::{rngs::StdRng, SeedableRng};

#[derive(serde::Deserialize)]
struct Fixture {
    oracle: String,
    filterpy: String,
    trials: usize,
    particles: usize,
    dt: f64,
    sigma_a_sq: f64,
    r_diag: Vec<f64>,
    p0_diag: Vec<f64>,
    x0: Vec<f64>,
    measurements: Vec<Vec<f64>>,
    reference_mean: Vec<Vec<f64>>,
    reference_stderr: Vec<Vec<f64>>,
    exact_posterior_mean: Vec<Vec<f64>>,
    reference_final_samples: Vec<Vec<f64>>,
}

fn fixture() -> Fixture {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/oracles/filters/particle.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).expect("the fixture parses")
}

fn diagonal<const N: usize>(d: &[f64]) -> SMatrix<f64, N, N> {
    let mut m = SMatrix::<f64, N, N>::zeros();
    for (i, v) in d.iter().enumerate() {
        m[(i, i)] = *v;
    }
    m
}

fn position_h() -> SMatrix<f64, 3, 6> {
    let mut h = SMatrix::<f64, 3, 6>::zeros();
    for axis in 0..3 {
        h[(axis, axis)] = 1.0;
    }
    h
}

/// One trial: the whole measurement sequence through a fresh cloud, returning the
/// posterior mean after every step.
fn one_trial(fixture: &Fixture, seed: u64) -> Vec<SVector<f64, 6>> {
    let mut rng = StdRng::seed_from_u64(seed);
    let mut pf = ParticleFilter::<ConstantVelocity, 6, 3>::new(
        fixture.particles,
        SVector::<f64, 6>::from_column_slice(&fixture.x0),
        diagonal::<6>(&fixture.p0_diag),
        ConstantVelocity {
            sigma_a_sq: fixture.sigma_a_sq,
        },
        position_h(),
        diagonal::<3>(&fixture.r_diag),
        &mut rng,
    )
    .expect("the fixture describes a well-formed cloud");

    let mut means = Vec::with_capacity(fixture.measurements.len());
    for z in &fixture.measurements {
        pf.predict(fixture.dt, &mut rng)
            .expect("a PSD process noise");
        pf.update(&SVector::<f64, 3>::from_column_slice(z), &mut rng)
            .expect("a live cloud");
        means.push(pf.mean());
    }
    means
}

#[allow(clippy::cast_precision_loss)]
fn count(n: usize) -> f64 {
    n as f64
}

fn all_trials(fixture: &Fixture) -> Vec<Vec<SVector<f64, 6>>> {
    (0..fixture.trials)
        .map(|seed| one_trial(fixture, seed as u64))
        .collect()
}

/// The two-sample Kolmogorov-Smirnov statistic: the largest gap between two empirical
/// distribution functions.
fn ks_statistic(a: &[f64], b: &[f64]) -> f64 {
    let mut a: Vec<f64> = a.to_vec();
    let mut b: Vec<f64> = b.to_vec();
    a.sort_by(f64::total_cmp);
    b.sort_by(f64::total_cmp);
    let (n, m) = (count(a.len()), count(b.len()));
    let (mut i, mut j) = (0_usize, 0_usize);
    let mut largest = 0.0_f64;
    while i < a.len() && j < b.len() {
        let x = a[i].min(b[j]);
        while i < a.len() && a[i] <= x {
            i += 1;
        }
        while j < b.len() && b[j] <= x {
            j += 1;
        }
        largest = largest.max((count(i) / n - count(j) / m).abs());
    }
    largest
}

/// §2's first option, on the whole sampling distribution.
///
/// The critical value is the standard asymptotic one at α = 0.05,
/// `1.36 √((n + m) / n m)`. A statistic below it means the two samples are consistent
/// with having been drawn from the same distribution, which is exactly the claim being
/// tested: that the Rust filter and the reference filter are the same filter.
#[test]
fn the_two_filters_pass_a_kolmogorov_smirnov_test() {
    let fixture = fixture();
    assert!(
        fixture.oracle.contains("systematic_resample"),
        "the fixture names its oracle: {}",
        fixture.oracle
    );
    assert_eq!(fixture.filterpy, "1.4.5", "the pinned oracle version");
    assert!(fixture.trials >= 20, "too few trials to say anything");
    assert_eq!(
        fixture.reference_final_samples.len(),
        fixture.trials,
        "the fixture must carry one raw sample per trial"
    );

    let runs = all_trials(&fixture);
    let last = fixture.measurements.len() - 1;
    let n = count(fixture.trials);
    let critical = 1.36 * ((n + n) / (n * n)).sqrt();

    for axis in 0..3 {
        let ours: Vec<f64> = runs.iter().map(|r| r[last][axis]).collect();
        let theirs: Vec<f64> = fixture
            .reference_final_samples
            .iter()
            .map(|s| s[axis])
            .collect();
        let d = ks_statistic(&ours, &theirs);
        assert!(
            d < critical,
            "component {axis}: KS statistic {d} over the α = 0.05 critical value \
             {critical}, so the two filters are not drawing from the same distribution"
        );
    }
}

/// §2's second option, applied as a statement about a distribution. See the module
/// documentation for why the excursion count is judged rather than forbidden.
#[test]
fn the_across_trial_means_agree_within_two_sigma_at_the_rate_two_sigma_predicts() {
    let fixture = fixture();
    let runs = all_trials(&fixture);
    let trials = count(fixture.trials);
    let steps = fixture.measurements.len();

    let mut comparisons = 0_u32;
    let mut excursions = 0_u32;
    let mut worst = 0.0_f64;
    let mut worst_where = String::new();

    for step in 0..steps {
        let mut mean = SVector::<f64, 6>::zeros();
        for run in &runs {
            mean += run[step];
        }
        mean /= trials;
        let mut variance = SVector::<f64, 6>::zeros();
        for run in &runs {
            let d = run[step] - mean;
            variance += d.component_mul(&d);
        }
        // ddof = 1: this is a sample of trials, not the population.
        variance /= trials - 1.0;
        let our_stderr = variance.map(|v| (v / trials).sqrt());

        for axis in 0..6 {
            let theirs = fixture.reference_mean[step][axis];
            let their_stderr = fixture.reference_stderr[step][axis];
            // Both estimates are noisy; treating the oracle's as exact would make the
            // criterion tighter than the row's on one side and looser on the other.
            let combined = (our_stderr[axis].powi(2) + their_stderr.powi(2))
                .sqrt()
                .max(1e-9);
            let deviation = (mean[axis] - theirs).abs() / combined;
            comparisons += 1;
            if deviation > 2.0 {
                excursions += 1;
            }
            if deviation > worst {
                worst = deviation;
                worst_where = format!(
                    "step {step} component {axis}: ours {} vs reference {theirs}",
                    mean[axis]
                );
            }
        }
    }

    // Under the null the excursion count is Binomial(comparisons, 0.0455). Four
    // standard deviations above its mean is the bound; anything at or below it is
    // consistent with two identical filters, and anything above says they differ.
    let n = f64::from(comparisons);
    let rate = 0.045_500_263_896_358_36_f64; // P(|Z| > 2) for a standard normal.
    let expected = n * rate;
    let allowance = expected + 4.0 * (n * rate * (1.0 - rate)).sqrt();
    assert!(
        f64::from(excursions) <= allowance,
        "{excursions} of {comparisons} comparisons exceeded 2σ; 2σ predicts about \
         {expected:.1} and allows up to {allowance:.1}, so the two filters differ"
    );
    // No single quantity may be grossly out. A 4σ deviation is not a fluctuation at
    // this sample size; it is a difference between the filters.
    assert!(
        worst < 4.0,
        "worst deviation {worst}σ at {worst_where}, which is past anything sampling \
         noise explains"
    );
}

/// What makes the comparisons above evidence rather than a coincidence: for this
/// linear-Gaussian model the exact posterior is known, and the cloud must approach it.
///
/// The tolerance is in units of the measurement's own standard deviation rather than
/// metres, because a Monte Carlo estimate's error scales with the width of what it is
/// estimating, not with the size of the numbers.
#[test]
fn the_cloud_converges_toward_the_exact_posterior() {
    let fixture = fixture();
    let runs = all_trials(&fixture);
    let trials = count(fixture.trials);
    let steps = fixture.measurements.len();

    // Skip the first third: the cloud starts from a wide prior and the exact filter's
    // early estimates move fast, so disagreement there measures a transient rather than
    // convergence.
    let mut worst = 0.0_f64;
    for step in (steps / 3)..steps {
        let mut mean = SVector::<f64, 6>::zeros();
        for run in &runs {
            mean += run[step];
        }
        mean /= trials;
        let exact = SVector::<f64, 6>::from_column_slice(&fixture.exact_posterior_mean[step]);
        // Position components only: with position-only measurements the velocity
        // marginal stays broad, and its Monte Carlo error is a different quantity from
        // the one this test is about.
        for axis in 0..3 {
            let sigma = fixture.r_diag[axis].sqrt();
            worst = worst.max((mean[axis] - exact[axis]).abs() / sigma);
        }
    }
    assert!(
        worst < 0.5,
        "the cloud's mean sat {worst} measurement standard deviations from the exact \
         posterior, so it is not converging to the right answer"
    );
}

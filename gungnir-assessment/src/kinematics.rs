// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The risk score's kinematic factor: how close a track is, whether it is closing, and how
//! soon it arrives (GAP-124, D-83; `docs/design/DN-01-defended-assets.md` amendment 2).
//!
//! # The rule
//!
//! For a track against one asset, with `r` the range to the asset's boundary, `v_c` the
//! closing speed along the line of sight, `τ` the deployment's urgency half-time:
//!
//! - **time to impact** `T = r / v_c`, only while `v_c > 0` (the definition DN-01 §5 and
//!   `RiskScore` already gave it, and what DN-03's warnings read);
//! - **urgency** `u = τ / (τ + T)`, written `1 / (1 + r / (τ v_c))` so nothing divides by
//!   the closing speed; zero when not closing, one at the boundary;
//! - **proximity** `p = 1 - r / max_range`, clamped to `[0, 1]`;
//! - **closing confidence** `c`: zero when not closing, rising to one as the closing speed
//!   clears its own one-sigma from the track's velocity covariance by more than
//!   [`CLOSING_SIGNIFICANCE_SIGMA`];
//! - the **factor** `K = c (1/2 + u/2) + (1 - c) p/2`.
//!
//! A track that is confidently closing scores on its time to impact alone, in
//! `(1/2, 1]`, so of two such tracks the one arriving sooner never scores lower, whatever
//! their ranges: that is MOP-28 and the `gungnir-assessment` Risk scoring row. A track that
//! is not closing scores on proximity alone, in `[0, 1/2]`, as it did before; it has no time
//! to impact and takes no urgency. Between them the covariance decides how much of each a
//! track gets, so a track whose closing the filter cannot tell from noise is not promoted
//! as if it were inbound, and the score moves continuously as a track turns from closing
//! to passing rather than halving in one tick.
//!
//! # What it survives
//!
//! Nothing here divides by the closing speed or by a sigma that could be zero, and every
//! input is checked finite before it is used: a zero or vanishing closing speed gives no
//! urgency and no time to impact, a non-finite state gives no factor at all (the assessor
//! then reports no exposure rather than a NaN that would sort to the top of a triage), and
//! a covariance that is not positive semi-definite along the line of sight gives the
//! estimate full credit -- down-rating a closing track because its covariance is corrupt is
//! the unsafe direction. The factor is always in `[0, 1]`.

use nalgebra::{Matrix3, Vector3};

/// How many of its own sigmas a closing speed must clear before the score treats the
/// track as closing (half credit at exactly this many).
///
/// Two, the ordinary two-sided 95 percent test: a stationary contact whose velocity
/// estimate wanders by a metre a second is not promoted as inbound on the noise.
pub const CLOSING_SIGNIFICANCE_SIGMA: f64 = 2.0;

/// The urgency half-time an assessor uses when the host gives none: sixty seconds, the
/// baseline's own default (`assessment.urgency_half_time_s`).
pub const DEFAULT_URGENCY_HALF_TIME_S: f64 = 60.0;

/// The terms behind one track's kinematic factor against the asset it is scored on, so
/// the evidence card can show what the score used rather than recomputing it (DN-01 §7).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct KinematicFactor {
    /// Closing speed along the line of sight to the asset's centre, m/s; negative when
    /// the track is opening.
    pub closing_speed_mps: f64,
    /// One-sigma of that closing speed from the track's velocity covariance, m/s; `None`
    /// when the covariance cannot give one (not finite, or negative along the line).
    pub closing_sigma_mps: Option<f64>,
    /// How far the score credits the track as closing, `[0, 1]`: zero when it is not,
    /// one when its closing speed is well clear of its uncertainty.
    pub closing_confidence: f64,
    /// `τ / (τ + time to impact)`, `[0, 1]`; zero when not closing.
    pub urgency: f64,
    /// `1 - range / max_range`, `[0, 1]`.
    pub proximity: f64,
    /// The factor the score multiplies, `[0, 1]`.
    pub value: f64,
}

/// A track's motion against one point, computed once for the exposure and the score.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Kinematics {
    /// Range to the asset's boundary, metres; zero inside an area asset.
    pub range_m: f64,
    /// Range to the asset's centre, metres.
    pub centre_range_m: f64,
    /// Seconds to reach the boundary at the current closing speed, `None` when not
    /// closing (or when that time is not representable).
    pub time_to_impact_s: Option<f32>,
    pub factor: KinematicFactor,
}

/// The inputs, gathered so the one computation serves both assessors.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Geometry<'a> {
    /// Track position minus the asset's centre, ENU metres.
    pub relative_enu: [f64; 3],
    /// Track velocity, ENU m/s.
    pub velocity_enu: [f64; 3],
    /// The track's 6x6 covariance; only its velocity block is read.
    pub covariance: &'a nalgebra::SMatrix<f64, 6, 6>,
    /// Radius of the asset's extent, metres; zero for a point.
    pub boundary_radius_m: f64,
    pub max_range_m: f64,
    pub urgency_half_time_s: f64,
}

/// The kinematic factor of one track against one point, or `None` when the track's
/// position or velocity is not a finite number and nothing honest can be said.
pub(crate) fn kinematics(g: &Geometry<'_>) -> Option<Kinematics> {
    let rel = Vector3::from(g.relative_enu);
    let v = Vector3::from(g.velocity_enu);
    if !(rel.iter().chain(v.iter()).all(|x| x.is_finite())) {
        return None;
    }
    let centre_range_m = rel.norm();
    let range_m = (centre_range_m - g.boundary_radius_m.max(0.0)).max(0.0);
    // The line of sight from the asset to the track; none when the track is on the
    // centre, where no direction is "toward" it.
    let line = (centre_range_m > f64::EPSILON).then(|| rel / centre_range_m);
    let closing_speed_mps = line.map_or(0.0, |u| -v.dot(&u));
    let closing_sigma_mps = line.and_then(|u| closing_sigma(g.covariance, &u));

    let closing = closing_speed_mps > 0.0;
    let urgency = if closing {
        urgency(range_m, closing_speed_mps, g.urgency_half_time_s)
    } else {
        0.0
    };
    let closing_confidence = if closing {
        closing_confidence(closing_speed_mps, closing_sigma_mps)
    } else {
        0.0
    };
    let proximity = if g.max_range_m > 0.0 {
        (1.0 - range_m / g.max_range_m).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let value = (closing_confidence * (0.5 + 0.5 * urgency)
        + (1.0 - closing_confidence) * 0.5 * proximity)
        .clamp(0.0, 1.0);

    // Scores and times are reported as f32 by design (`RiskScore`); a time beyond f32's
    // range is a track that is not coming, and is reported as not closing.
    #[allow(clippy::cast_possible_truncation)]
    let time_to_impact_s = closing
        .then(|| (range_m / closing_speed_mps) as f32)
        .filter(|t| t.is_finite());

    Some(Kinematics {
        range_m,
        centre_range_m,
        time_to_impact_s,
        factor: KinematicFactor {
            closing_speed_mps,
            closing_sigma_mps,
            closing_confidence,
            urgency,
            proximity,
            value: if value.is_finite() { value } else { 0.0 },
        },
    })
}

/// `τ / (τ + r / v)` for `v > 0`, as `1 / (1 + r / (τ v))`: no division by the closing
/// speed, one at the boundary, falling to zero as the time to impact grows.
fn urgency(range_m: f64, closing_speed_mps: f64, half_time_s: f64) -> f64 {
    if range_m <= 0.0 {
        return 1.0;
    }
    let reach_m = half_time_s * closing_speed_mps;
    if !(reach_m.is_finite() && reach_m > 0.0) {
        // A half-time the baseline refuses, or a closing speed so small the product
        // underflows: no urgency, rather than a NaN from 0/0.
        return 0.0;
    }
    let u = 1.0 / (1.0 + range_m / reach_m);
    if u.is_finite() {
        u.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// One-sigma of the closing speed along `line`, from the velocity block of the covariance.
/// `None` when the quadratic form is not a finite non-negative number, which is a
/// covariance the tracking core's own invariant forbids.
fn closing_sigma(covariance: &nalgebra::SMatrix<f64, 6, 6>, line: &Vector3<f64>) -> Option<f64> {
    let p_vv: Matrix3<f64> = covariance.fixed_view::<3, 3>(3, 3).into_owned();
    let variance = (line.transpose() * p_vv * line)[(0, 0)];
    (variance.is_finite() && variance >= 0.0).then(|| variance.sqrt())
}

/// How far a positive closing speed is credited, given its sigma: `Φ(z - k)` rescaled so
/// that it is exactly zero at `z = 0` and tends to one, where `z` is the speed in sigmas
/// and `k` is [`CLOSING_SIGNIFICANCE_SIGMA`]. Full credit when there is no usable sigma
/// or it is zero, because the estimate is then all there is.
fn closing_confidence(closing_speed_mps: f64, sigma_mps: Option<f64>) -> f64 {
    let Some(sigma) = sigma_mps.filter(|s| *s > 0.0) else {
        return 1.0;
    };
    let z = closing_speed_mps / sigma;
    if !z.is_finite() {
        return 1.0;
    }
    let floor = standard_normal_cdf(-CLOSING_SIGNIFICANCE_SIGMA);
    let credit = (standard_normal_cdf(z - CLOSING_SIGNIFICANCE_SIGMA) - floor) / (1.0 - floor);
    credit.clamp(0.0, 1.0)
}

/// The standard normal cumulative distribution, `Φ(x) = erfc(-x / √2) / 2`, with `erfc`
/// from the Chebyshev fit in Press et al., *Numerical Recipes* §6.2 (`erfcc`), whose
/// fractional error is below 1.2e-7 everywhere: far finer than a score is read to.
pub(crate) fn standard_normal_cdf(x: f64) -> f64 {
    0.5 * erfc(-x / std::f64::consts::SQRT_2)
}

fn erfc(x: f64) -> f64 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.5 * z);
    let poly = -z * z - 1.265_512_23
        + t * (1.000_023_68
            + t * (0.374_091_96
                + t * (0.096_784_18
                    + t * (-0.186_288_06
                        + t * (0.278_868_07
                            + t * (-1.135_203_98
                                + t * (1.488_515_87 + t * (-0.822_152_23 + t * 0.170_872_77))))))));
    let tail = t * poly.exp();
    if x >= 0.0 {
        tail
    } else {
        2.0 - tail
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::SMatrix;

    /// Against tabulated values of Φ.
    #[test]
    fn the_normal_cdf_matches_its_table() {
        for (x, phi) in [
            (0.0, 0.5),
            (1.0, 0.841_344_746),
            (-1.0, 0.158_655_254),
            (2.0, 0.977_249_868),
            (-2.0, 0.022_750_132),
            (3.0, 0.998_650_102),
            (-4.0, 0.000_031_671),
        ] {
            let got = standard_normal_cdf(x);
            assert!((got - phi).abs() < 2e-7, "Φ({x}) = {got}, table {phi}");
        }
        assert!((standard_normal_cdf(f64::INFINITY) - 1.0).abs() < f64::EPSILON);
        assert!(standard_normal_cdf(f64::NEG_INFINITY).abs() < f64::EPSILON);
    }

    fn geometry(
        relative_enu: [f64; 3],
        velocity_enu: [f64; 3],
        covariance: &SMatrix<f64, 6, 6>,
    ) -> Geometry<'_> {
        Geometry {
            relative_enu,
            velocity_enu,
            covariance,
            boundary_radius_m: 0.0,
            max_range_m: 10_000.0,
            urgency_half_time_s: 60.0,
        }
    }

    /// A closing speed indistinguishable from its noise earns almost nothing; one far
    /// clear of it earns everything; the credit rises between and is zero at zero.
    #[test]
    fn closing_confidence_follows_the_closing_speed_through_its_sigma() {
        assert!(closing_confidence(0.0, Some(1.0)).abs() < 1e-12);
        let mut previous = 0.0;
        for speed in [0.5, 1.0, 2.0, 3.0, 5.0, 10.0] {
            let c = closing_confidence(speed, Some(1.0));
            assert!(c > previous, "{speed} m/s: {c} after {previous}");
            previous = c;
        }
        assert!(closing_confidence(10.0, Some(1.0)) > 0.999_999);
        assert!(closing_confidence(0.5, Some(1.0)) < 0.05);
        // No usable sigma: the estimate is all there is.
        assert!((closing_confidence(0.1, None) - 1.0).abs() < f64::EPSILON);
        assert!((closing_confidence(0.1, Some(0.0)) - 1.0).abs() < f64::EPSILON);
    }

    /// Zero, vanishing, and enormous closing speeds, a NaN covariance, and a track on the
    /// asset's centre all give a finite factor in [0, 1] and never a NaN.
    #[test]
    fn the_factor_is_bounded_and_finite_at_the_degenerate_cases() {
        let identity = SMatrix::<f64, 6, 6>::identity();
        let mut poisoned = SMatrix::<f64, 6, 6>::identity();
        poisoned[(3, 3)] = f64::NAN;
        let mut negative = SMatrix::<f64, 6, 6>::identity();
        for i in 3..6 {
            negative[(i, i)] = -4.0;
        }
        let cases: [([f64; 3], [f64; 3], &SMatrix<f64, 6, 6>); 9] = [
            ([1_000.0, 0.0, 0.0], [0.0, 0.0, 0.0], &identity),
            ([1_000.0, 0.0, 0.0], [-1e-300, 0.0, 0.0], &identity),
            ([1_000.0, 0.0, 0.0], [-1e-12, 0.0, 0.0], &identity),
            ([1_000.0, 0.0, 0.0], [-1e300, 0.0, 0.0], &identity),
            ([1_000.0, 0.0, 0.0], [-50.0, 0.0, 0.0], &poisoned),
            ([1_000.0, 0.0, 0.0], [-50.0, 0.0, 0.0], &negative),
            ([0.0, 0.0, 0.0], [-50.0, 0.0, 0.0], &identity),
            ([1e-300, 0.0, 0.0], [-50.0, 0.0, 0.0], &identity),
            ([1e300, 0.0, 0.0], [-50.0, 0.0, 0.0], &identity),
        ];
        for (rel, vel, cov) in cases {
            let k = kinematics(&geometry(rel, vel, cov)).expect("finite inputs");
            let f = k.factor;
            for (name, x) in [
                ("value", f.value),
                ("urgency", f.urgency),
                ("confidence", f.closing_confidence),
                ("proximity", f.proximity),
            ] {
                assert!(
                    x.is_finite() && (0.0..=1.0).contains(&x),
                    "{name} = {x} for {rel:?} {vel:?}"
                );
            }
            if let Some(t) = k.time_to_impact_s {
                assert!(t.is_finite() && t >= 0.0, "time to impact {t}");
            }
        }
        // Not closing at all: no urgency and no time to impact.
        let still =
            kinematics(&geometry([1_000.0, 0.0, 0.0], [0.0; 3], &identity)).expect("finite");
        assert!(still.time_to_impact_s.is_none());
        assert!(still.factor.urgency.abs() < f64::EPSILON);
        // A corrupt covariance credits the estimate fully rather than down-rating it.
        let corrupt = kinematics(&geometry([1_000.0, 0.0, 0.0], [-50.0, 0.0, 0.0], &poisoned))
            .expect("finite");
        assert!(corrupt.factor.closing_sigma_mps.is_none());
        assert!((corrupt.factor.closing_confidence - 1.0).abs() < f64::EPSILON);
    }

    /// A position or velocity that is not a number gives no factor at all, so the
    /// assessor can report no exposure rather than a NaN that sorts to the top.
    #[test]
    fn a_non_finite_state_gives_no_factor() {
        let identity = SMatrix::<f64, 6, 6>::identity();
        for (rel, vel) in [
            ([f64::NAN, 0.0, 0.0], [-1.0, 0.0, 0.0]),
            ([1_000.0, 0.0, 0.0], [f64::INFINITY, 0.0, 0.0]),
        ] {
            assert!(kinematics(&geometry(rel, vel, &identity)).is_none());
        }
    }

    /// The factor is continuous as a track's closing speed passes through zero: it turns
    /// from closing to opening without a step, so a passing track fades rather than
    /// halving in one tick.
    #[test]
    fn the_factor_is_continuous_through_zero_closing_speed() {
        let identity = SMatrix::<f64, 6, 6>::identity();
        let at = |closing: f64| {
            kinematics(&geometry(
                [1_000.0, 0.0, 0.0],
                [-closing, 0.0, 0.0],
                &identity,
            ))
            .expect("finite")
            .factor
            .value
        };
        let opening = at(-1e-6);
        let zero = at(0.0);
        let closing = at(1e-6);
        assert!((opening - zero).abs() < 1e-6 && (closing - zero).abs() < 1e-6);
    }
}

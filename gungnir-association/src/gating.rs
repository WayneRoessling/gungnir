//! Ellipsoidal / chi-square gating: the verification-capability-table.md §1 row
//! "Gating (ellipsoidal / chi-square)".
//!
//! Oracle: a closed-form chi-square computation. Criterion: **exact match on gate
//! membership** for the same predicted state, covariance, and measurements.
//!
//! A gate answers one question: is this measurement plausibly from this track? The
//! test is the squared Mahalanobis distance of the innovation under the innovation
//! covariance,
//!
//! ```text
//! d² = yᵀ S⁻¹ y,   y = z - H x,   S = H P Hᵀ + R
//! ```
//!
//! which is chi-square distributed with as many degrees of freedom as the measurement
//! has dimensions when the filter's assumptions hold. The gate admits when
//! `d² ≤ threshold`.
//!
//! # Why a Cholesky solve rather than `S⁻¹`
//!
//! `d²` is computed by solving `S z = y` and taking `y · z`, not by forming `S⁻¹` and
//! multiplying. The two agree in exact arithmetic; in floating point the explicit
//! inverse loses roughly a squared condition number of accuracy, and `S` is at its
//! worst conditioned exactly when a track is well-observed in one axis and barely
//! observed in another -- which is the normal state of a radar track, not an edge
//! case. The membership criterion is *exact match*, so a gate that disagreed with the
//! oracle only for ill-conditioned `S` would still fail, and rightly.
//!
//! The Cholesky factorization also does the validation for free: it exists precisely
//! when `S` is positive definite, which is the condition under which `d²` is a
//! distance at all.

use crate::assignment::AssociationError;
use nalgebra::{SMatrix, SVector};

/// Ellipsoidal / chi-square gate: filters statistically implausible detections before
/// they reach an associator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChiSquareGate {
    /// Squared-distance threshold. Chi-square quantile for the measurement dimension
    /// at the confidence the deployment wants; [`ChiSquareGate::at_99_percent`] and
    /// [`ChiSquareGate::at_95_percent`] give the usual ones.
    pub gate_threshold: f64,
}

/// Chi-square quantiles at 95% and 99% for 1 to 6 degrees of freedom, from the
/// standard tables. Indexed by degrees of freedom, so entry 0 is unused.
///
/// Tabulated rather than computed: the inverse chi-square CDF needs an incomplete
/// gamma function, which is a large amount of numerics to carry for six constants
/// that never change. `gungnir-oracle`'s fixture checks these against
/// `scipy.stats.chi2.ppf`, so a typo here is caught rather than trusted.
const CHI2_95: [f64; 7] = [
    0.0,
    3.841_458_820_694_124,
    5.991_464_547_107_979,
    7.814_727_903_251_179,
    9.487_729_036_781_154,
    11.070_497_693_516_351,
    12.591_587_243_743_977,
];
const CHI2_99: [f64; 7] = [
    0.0,
    6.634_896_601_021_213,
    9.210_340_371_976_184,
    11.344_866_730_144_373,
    13.276_704_135_987_622,
    15.086_272_469_388_987,
    16.811_893_829_770_927,
];

impl ChiSquareGate {
    /// A gate admitting 95% of true measurements for `dof` measurement dimensions.
    ///
    /// # Panics
    /// If `dof` is zero or above 6; the table covers the measurement dimensions this
    /// workspace produces, and a caller outside that range has a bug rather than an
    /// unusual sensor.
    #[must_use]
    pub fn at_95_percent(dof: usize) -> Self {
        assert!(
            (1..CHI2_95.len()).contains(&dof),
            "chi-square table covers 1 to 6 degrees of freedom, got {dof}"
        );
        Self {
            gate_threshold: CHI2_95[dof],
        }
    }

    /// A gate admitting 99% of true measurements for `dof` measurement dimensions.
    ///
    /// # Panics
    /// If `dof` is zero or above 6, for the same reason as [`Self::at_95_percent`].
    #[must_use]
    pub fn at_99_percent(dof: usize) -> Self {
        assert!(
            (1..CHI2_99.len()).contains(&dof),
            "chi-square table covers 1 to 6 degrees of freedom, got {dof}"
        );
        Self {
            gate_threshold: CHI2_99[dof],
        }
    }

    /// Squared Mahalanobis distance of `innovation` under `innovation_covariance`.
    ///
    /// # Errors
    /// [`AssociationError::NonFiniteCost`] if an input is non-finite or if `S` is not
    /// positive definite, in which case no distance exists. The row and column in the
    /// error name the offending entry of `S`, or `(0, 0)` when the factorization
    /// failed as a whole.
    pub fn squared_distance<const M: usize>(
        innovation: &SVector<f64, M>,
        innovation_covariance: &SMatrix<f64, M, M>,
    ) -> Result<f64, AssociationError> {
        for (i, v) in innovation.iter().enumerate() {
            if !v.is_finite() {
                return Err(AssociationError::NonFiniteCost { row: i, col: 0 });
            }
        }
        for row in 0..M {
            for col in 0..M {
                if !innovation_covariance[(row, col)].is_finite() {
                    return Err(AssociationError::NonFiniteCost { row, col });
                }
            }
        }
        // Cholesky exists exactly when S is positive definite, which is the condition
        // under which d^2 is a distance; solving is also better conditioned than
        // forming the inverse. See the module documentation.
        let Some(chol) = innovation_covariance.cholesky() else {
            return Err(AssociationError::NonFiniteCost { row: 0, col: 0 });
        };
        let solved = chol.solve(innovation);
        Ok(innovation.dot(&solved))
    }

    /// Whether the measurement is inside the gate.
    ///
    /// # Errors
    /// As [`Self::squared_distance`].
    pub fn admits<const M: usize>(
        &self,
        innovation: &SVector<f64, M>,
        innovation_covariance: &SMatrix<f64, M, M>,
    ) -> Result<bool, AssociationError> {
        Ok(Self::squared_distance(innovation, innovation_covariance)? <= self.gate_threshold)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::{Matrix3, Vector3};

    #[test]
    fn identity_covariance_gives_the_squared_norm() {
        let y = Vector3::new(3.0, 4.0, 0.0);
        let s = Matrix3::identity();
        let d2 = ChiSquareGate::squared_distance(&y, &s).expect("positive definite");
        assert!((d2 - 25.0).abs() < 1e-12, "d2 was {d2}");
    }

    /// Scaling the covariance by k scales the squared distance by 1/k. This is the
    /// property that makes the gate a *statistical* test rather than a metric one.
    #[test]
    fn distance_scales_inversely_with_covariance() {
        let y = Vector3::new(1.0, 2.0, 3.0);
        let base = ChiSquareGate::squared_distance(&y, &Matrix3::identity()).expect("pd");
        let scaled = ChiSquareGate::squared_distance(&y, &(Matrix3::identity() * 4.0)).expect("pd");
        assert!((scaled - base / 4.0).abs() < 1e-12);
    }

    #[test]
    fn a_measurement_on_the_prediction_is_always_admitted() {
        let gate = ChiSquareGate::at_99_percent(3);
        let s = Matrix3::new(9.0, 1.0, 0.0, 1.0, 4.0, 0.5, 0.0, 0.5, 16.0);
        assert!(gate.admits(&Vector3::zeros(), &s).expect("pd"));
    }

    #[test]
    fn a_distant_measurement_is_rejected() {
        let gate = ChiSquareGate::at_95_percent(3);
        let s = Matrix3::identity();
        assert!(!gate.admits(&Vector3::new(100.0, 0.0, 0.0), &s).expect("pd"));
    }

    /// The boundary is inclusive: `d² == threshold` is inside. The criterion is exact
    /// match on membership, so which side the boundary falls on is part of the answer.
    #[test]
    fn the_boundary_is_inclusive() {
        let gate = ChiSquareGate {
            gate_threshold: 25.0,
        };
        let s = Matrix3::identity();
        assert!(gate.admits(&Vector3::new(3.0, 4.0, 0.0), &s).expect("pd"));
        assert!(!gate
            .admits(&Vector3::new(3.0, 4.000_001, 0.0), &s)
            .expect("pd"));
    }

    #[test]
    fn singular_covariance_is_an_error_not_a_panic() {
        let s = Matrix3::zeros();
        assert!(ChiSquareGate::squared_distance(&Vector3::new(1.0, 0.0, 0.0), &s).is_err());
    }

    #[test]
    fn non_finite_input_is_an_error_not_a_panic() {
        let s = Matrix3::identity();
        assert!(ChiSquareGate::squared_distance(&Vector3::new(f64::NAN, 0.0, 0.0), &s).is_err());
        let mut bad = Matrix3::identity();
        bad[(2, 2)] = f64::INFINITY;
        assert!(ChiSquareGate::squared_distance(&Vector3::new(1.0, 0.0, 0.0), &bad).is_err());
    }

    #[test]
    fn quantile_tables_are_ordered_and_increasing() {
        for dof in 1..CHI2_95.len() {
            assert!(CHI2_95[dof] < CHI2_99[dof], "95% exceeds 99% at dof {dof}");
            if dof > 1 {
                assert!(CHI2_95[dof] > CHI2_95[dof - 1]);
                assert!(CHI2_99[dof] > CHI2_99[dof - 1]);
            }
        }
    }
}

//! Debug-only numerical invariant checks, per agentic-coding-standards.md §2.1.
//!
//! [`assert_psd`] lives here rather than in `gungnir-testkit` because library code
//! (every `Filter::new` and every in-place covariance mutation) must be able to call
//! it, and `gungnir-testkit` is only ever a dev-dependency. In release builds the
//! function is an empty inline stub.

use nalgebra::SMatrix;

/// Largest tolerated asymmetry `max |P - P^T|` before a covariance is rejected.
pub const SYMMETRY_TOL: f64 = 1e-9;

/// Diagonal shift applied before the Cholesky test so that a covariance which is
/// positive *semi*-definite (singular but valid) is accepted while an indefinite one
/// is still rejected.
pub const PSD_SHIFT: f64 = 1e-12;

/// Assert, in debug builds only, that `cov` is symmetric and positive semi-definite.
///
/// Symmetry is checked element-wise against [`SYMMETRY_TOL`]; semi-definiteness is
/// checked by attempting a Cholesky factorization of `cov + PSD_SHIFT * I`, which
/// succeeds for every PSD matrix and fails for any matrix with a negative eigenvalue
/// below `-PSD_SHIFT`. Compiled out entirely in release builds.
///
/// ```
/// use nalgebra::SMatrix;
/// let cov = SMatrix::<f64, 3, 3>::identity();
/// gungnir_core::assert_psd(&cov);
/// ```
#[cfg(debug_assertions)]
pub fn assert_psd<const N: usize>(cov: &SMatrix<f64, N, N>) {
    let asymmetry = (cov - cov.transpose()).abs().max();
    debug_assert!(
        asymmetry < SYMMETRY_TOL,
        "covariance not symmetric: max |P - P^T| = {asymmetry}"
    );
    let mut shifted = *cov;
    for i in 0..N {
        shifted[(i, i)] += PSD_SHIFT;
    }
    debug_assert!(
        shifted.cholesky().is_some(),
        "covariance not positive semi-definite (Cholesky of P + {PSD_SHIFT} I failed)"
    );
}

/// Release-build stub: does nothing.
#[cfg(not(debug_assertions))]
#[inline(always)]
pub fn assert_psd<const N: usize>(_cov: &SMatrix<f64, N, N>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_psd() {
        assert_psd(&SMatrix::<f64, 4, 4>::identity());
    }

    #[test]
    fn singular_but_psd_is_accepted() {
        // rank-1 outer product: PSD with zero eigenvalues.
        let v = nalgebra::SVector::<f64, 3>::new(1.0, 2.0, 3.0);
        assert_psd(&(v * v.transpose()));
    }

    #[test]
    #[should_panic(expected = "not positive semi-definite")]
    fn indefinite_is_rejected() {
        let mut m = SMatrix::<f64, 2, 2>::identity();
        m[(1, 1)] = -1.0;
        assert_psd(&m);
    }

    #[test]
    #[should_panic(expected = "not symmetric")]
    fn asymmetric_is_rejected() {
        let mut m = SMatrix::<f64, 2, 2>::identity();
        m[(0, 1)] = 0.5;
        assert_psd(&m);
    }
}

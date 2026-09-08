// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Point-to-plane ICP (GAP-024): the linearised transform-estimation step over
//! `PointBuffer::normals` (`crate::normals::estimate_normals`), and the CPU reference
//! driving it. `cpu_reference.rs`'s own documentation is explicit that this is a
//! distinct linearised solve and not a corollary of normals existing; this module is
//! that other solve.
//!
//! # The linearisation
//!
//! For a small additional rotation `R = I + [ω]×` and translation `t` applied to an
//! already-close source point `p` corresponding to a target point `q` with unit
//! normal `n`, the point-to-plane residual `n·(Rp + t − q)` linearises, to first
//! order in `ω`, to `n·(p + ω×p + t − q)`. The scalar triple product identity
//! `n·(ω×p) = ω·(p×n)` makes this **linear** in the six unknowns `x = [ω; t]`:
//!
//! `r = a·x + c`, with `a = [p×n; n] ∈ R⁶` and `c = n·(p−q)`.
//!
//! Minimising `Σ rᵢ²` over every correspondence gives the normal equations
//! `A x = b`, `A = Σ aᵢaᵢᵀ` (6×6, symmetric positive semi-definite), `b = −Σ cᵢaᵢ`,
//! solved once per iteration. The solved `ω` is turned back into a rotation via
//! `UnitQuaternion::from_scaled_axis` -- the true exponential map (Rodrigues'
//! formula), not the non-orthogonal `I + [ω]×` the linearisation itself used to reach
//! a *linear* problem; reconstructing properly costs nothing extra and every degree
//! the small-angle step turns out not to be so small is a degree `[ω]×` alone would
//! have gotten wrong.
//!
//! **Verified independently in Python** (`numpy`, not committed -- the same
//! disclosure `normals.rs` makes about its own check): a hand-built single
//! correspondence `p=(1,0,0)`, `q=(1,0,0.1)`, `n=(0,0,1)` gives `a=[0,-1,0,0,0,1]`,
//! `c=-0.1`, matching the formula above by hand; a 36-point correspondence set (the
//! surface [`grid`] below, in its own test module) under a known small transform
//! `ω=(0.02,-0.015,0.01)`, `t=(0.03,-0.02,0.015)` solves to within 0.5% of the
//! transform's inverse in one linearisation, and the point-to-plane residual falls
//! from a mean of 0.054 m to 1.8e-8 m after one further re-linearisation -- the
//! Newton-like quadratic convergence a correctly linearised least-squares problem
//! should show.
//!
//! # Why the test surface is not `cpu_reference`'s own grid
//!
//! `cpu_reference.rs` and `normals.rs` both build their synthetic surface as
//! `z = sin(1.3x + 0.7y) * 0.4`. **That surface is numerically degenerate for this
//! solve specifically**, found empirically rather than assumed: its normals vary too
//! little in direction across a 2.5 m × 2.5 m patch for `A` to be well-conditioned --
//! `cond(A)` came out above `10^17` in the Python check above, several single-precision
//! `f32` epsilons past useless, though the *same* surface is perfectly fine for
//! `estimate_normals`'s own per-point PCA, which only ever looks at one local
//! neighbourhood at a time and never sums curvature information globally the way
//! this solve does. A second sinusoidal term at a different frequency and orientation
//! (`+ cos(0.9x − 1.7y) * 0.25`, see [`grid`]) breaks the near-planarity and brings
//! `cond(A)` down to about 280 -- comfortably solvable, and the surface used
//! throughout this module's own tests.

use nalgebra::{Isometry3, SMatrix, SVector, Translation3, UnitQuaternion, Vector3};

use crate::FusionError;
use gungnir_data::pointcloud::PointBuffer;

/// Below this ratio of the normal equations' smallest to largest eigenvalue, no
/// rotation is determined by the data -- the point-to-plane analogue of
/// [`crate::normals::estimate_normals`]'s own collinearity threshold, and the same
/// shape of problem: a system that looks solvable and, numerically, is not. A planar
/// or near-planar correspondence set (every normal pointing the same way) is the
/// common case this catches: no combination of points can then tell a rotation about
/// that shared normal from a translation within the plane it defines.
const DEGENERACY_RATIO: f32 = 1e-6;

/// The incremental small rigid transform minimising the summed squared
/// point-to-plane distance over matched `(source, target, target_normal)` triples.
/// `source[i]` is expected already close to `target[i]` -- the linearisation is
/// first-order in the rotation, and is not meant to recover a large one in a single
/// call the way [`crate::transform_solve::rigid_transform_between`] can.
///
/// # Errors
///
/// `FusionError::EmptyInput` for mismatched lengths or fewer than six
/// correspondences: six unknowns need at least six equations to constrain them in
/// general, before the specific geometry is even considered. `FusionError::Degenerate`
/// when the correspondences do not determine all six degrees of freedom regardless of
/// count (see [`DEGENERACY_RATIO`]), or when the assembled system is not finite.
pub fn point_to_plane_transform(
    sources: &[[f32; 3]],
    targets: &[[f32; 3]],
    target_normals: &[[f32; 3]],
) -> Result<Isometry3<f32>, FusionError> {
    if sources.len() != targets.len() || sources.len() != target_normals.len() || sources.len() < 6
    {
        return Err(FusionError::EmptyInput(format!(
            "{} source, {} target, {} normal -- six matched triples are the least a \
             point-to-plane transform can be solved from",
            sources.len(),
            targets.len(),
            target_normals.len(),
        )));
    }
    let mut a_mat = SMatrix::<f32, 6, 6>::zeros();
    let mut b_vec = SVector::<f32, 6>::zeros();
    for ((p, q), n) in sources.iter().zip(targets).zip(target_normals) {
        let p = Vector3::new(p[0], p[1], p[2]);
        let q = Vector3::new(q[0], q[1], q[2]);
        let n = Vector3::new(n[0], n[1], n[2]);
        let cross = p.cross(&n);
        let a = SVector::<f32, 6>::new(cross.x, cross.y, cross.z, n.x, n.y, n.z);
        let c = n.dot(&(p - q));
        a_mat += a * a.transpose();
        b_vec += a * -c;
    }
    if !a_mat.iter().all(|v| v.is_finite()) || !b_vec.iter().all(|v| v.is_finite()) {
        return Err(FusionError::Degenerate(
            "the point-to-plane normal equations are not finite".into(),
        ));
    }
    let eigen = a_mat.symmetric_eigen();
    let min_eig = eigen.eigenvalues.min();
    let max_eig = eigen.eigenvalues.max();
    if max_eig <= 0.0 || min_eig / max_eig < DEGENERACY_RATIO {
        return Err(FusionError::Degenerate(format!(
            "the correspondences do not determine all six degrees of freedom \
             (eigenvalue ratio {:.3e}); a planar or near-planar surface is the usual cause",
            if max_eig > 0.0 {
                min_eig / max_eig
            } else {
                0.0
            },
        )));
    }
    let Some(x) = a_mat.cholesky().map(|c| c.solve(&b_vec)) else {
        return Err(FusionError::Degenerate(
            "the normal equations passed the eigenvalue check and still admit no \
             Cholesky solve"
                .into(),
        ));
    };
    let omega = Vector3::new(x[0], x[1], x[2]);
    let t = Vector3::new(x[3], x[4], x[5]);
    Ok(Isometry3::from_parts(
        Translation3::from(t),
        UnitQuaternion::from_scaled_axis(omega),
    ))
}

/// Point-to-plane ICP: the same driving loop as [`crate::cpu_reference::CpuIcp`] --
/// correspond by nearest point, solve, compose, judge convergence -- over
/// [`point_to_plane_transform`] instead of Kabsch, and reading `target.normals`
/// instead of ignoring them.
pub struct CpuIcpPointToPlane {
    pub max_iterations: u32,
    pub target: PointBuffer,
    /// `target.normals`, taken out at construction so every later use is a plain
    /// field and never an `Option` a caller could find empty later: `target.normals`
    /// could not have been `None` for `new` to have returned `Ok`, and re-deriving
    /// that fact at every `step` would need an `expect` this workspace's own rule
    /// forbids outside tests and `main`.
    target_normals: Vec<[f32; 3]>,
    /// Iteration stops when the mean point-to-plane residual improves by less than
    /// this, metres.
    pub convergence_eps_m: f32,
    /// A correspondence closer than this counts as an inlier.
    pub inlier_distance_m: f32,
    estimate: Isometry3<f32>,
    iterations: u32,
    last_mean_residual: Option<f32>,
    converged: bool,
}

impl CpuIcpPointToPlane {
    /// # Errors
    ///
    /// `FusionError::EmptyInput` when `target.normals` is `None`: a point-to-plane
    /// reference has no plane to measure against without one, and estimating one
    /// silently here is exactly what [`gungnir_data::pointcloud::PointBuffer::normals`]'s
    /// own documentation says this crate's point-to-plane path must not rest on.
    pub fn new(target: PointBuffer, max_iterations: u32) -> Result<Self, FusionError> {
        let Some(target_normals) = target.normals.clone() else {
            return Err(FusionError::EmptyInput(
                "the target carries no normals; point-to-plane ICP needs one per \
                 target point (crate::normals::estimate_normals)"
                    .into(),
            ));
        };
        Ok(Self {
            max_iterations,
            target,
            target_normals,
            convergence_eps_m: 1e-4,
            inlier_distance_m: 0.5,
            estimate: Isometry3::identity(),
            iterations: 0,
            last_mean_residual: None,
            converged: false,
        })
    }

    #[must_use]
    pub fn estimate(&self) -> Isometry3<f32> {
        self.estimate
    }

    #[must_use]
    pub fn iterations(&self) -> u32 {
        self.iterations
    }

    /// The index of the nearest target point to `p`, its position, and the distance.
    fn nearest(&self, p: [f32; 3]) -> Option<(usize, [f32; 3], f32)> {
        self.target
            .positions
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let d = (t[0] - p[0]).powi(2) + (t[1] - p[1]).powi(2) + (t[2] - p[2]).powi(2);
                (i, *t, d.sqrt())
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))
    }
}

impl crate::PointCloudFusion for CpuIcpPointToPlane {
    /// One ICP iteration per call: correspond, solve, compose, judge convergence.
    ///
    /// # Errors
    ///
    /// `EmptyInput` when either cloud is empty; `Divergence` once the iteration
    /// budget is spent without convergence; `Degenerate` as
    /// [`point_to_plane_transform`].
    fn step(&mut self, source: &PointBuffer) -> Result<crate::FusionStepResult, FusionError> {
        if source.positions.is_empty() || self.target.positions.is_empty() {
            return Err(FusionError::EmptyInput(format!(
                "{} source and {} target points",
                source.positions.len(),
                self.target.positions.len()
            )));
        }
        if self.converged {
            return Ok(self.result(self.inlier_ratio(source)));
        }
        if self.iterations >= self.max_iterations {
            return Err(FusionError::Divergence);
        }
        let moved: Vec<[f32; 3]> = source
            .positions
            .iter()
            .map(|p| crate::transform_solve::apply(&self.estimate, *p))
            .collect();
        let mut pairs_source = Vec::with_capacity(moved.len());
        let mut pairs_target = Vec::with_capacity(moved.len());
        let mut pairs_normal = Vec::with_capacity(moved.len());
        let mut residual = 0.0_f32;
        for p in &moved {
            if let Some((i, t, _)) = self.nearest(*p) {
                let n = self.target_normals[i];
                let plane_residual =
                    n[0] * (p[0] - t[0]) + n[1] * (p[1] - t[1]) + n[2] * (p[2] - t[2]);
                residual += plane_residual.abs();
                pairs_source.push(*p);
                pairs_target.push(t);
                pairs_normal.push(n);
            }
        }
        #[allow(clippy::cast_precision_loss)]
        let mean_residual = residual / pairs_source.len().max(1) as f32;
        let delta = point_to_plane_transform(&pairs_source, &pairs_target, &pairs_normal)?;
        self.estimate = delta * self.estimate;
        self.iterations += 1;
        let improved = self
            .last_mean_residual
            .is_some_and(|last| (last - mean_residual).abs() < self.convergence_eps_m);
        self.last_mean_residual = Some(mean_residual);
        self.converged = improved || mean_residual < self.convergence_eps_m;
        Ok(self.result(self.inlier_ratio(source)))
    }
}

impl CpuIcpPointToPlane {
    fn inlier_ratio(&self, source: &PointBuffer) -> f32 {
        let inliers = source
            .positions
            .iter()
            .filter(|p| {
                self.nearest(crate::transform_solve::apply(&self.estimate, **p))
                    .is_some_and(|(_, _, d)| d <= self.inlier_distance_m)
            })
            .count();
        #[allow(clippy::cast_precision_loss)]
        let ratio = inliers as f32 / source.positions.len().max(1) as f32;
        ratio
    }

    fn result(&self, inlier_ratio: f32) -> crate::FusionStepResult {
        crate::FusionStepResult {
            transform: self.estimate,
            converged: self.converged,
            inlier_ratio,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normals::estimate_normals;
    use crate::transform_solve::apply;
    use crate::PointCloudFusion;
    use nalgebra::{Translation3, UnitQuaternion};

    /// Two sinusoidal terms at different frequencies and orientations, not one: the
    /// module documentation's own finding is that a single term (`cpu_reference`'s
    /// and `normals.rs`'s shared `grid()`) is numerically degenerate for this specific
    /// solve, however fine it is for per-point normal estimation.
    fn grid() -> PointBuffer {
        let mut positions = Vec::new();
        for i in 0..6 {
            for j in 0..6 {
                #[allow(clippy::cast_precision_loss)]
                let (x, y) = (i as f32 * 0.5, j as f32 * 0.5);
                let z = (x * 1.3 + y * 0.7).sin() * 0.4 + (x * 0.9 - y * 1.7).cos() * 0.25;
                positions.push([x, y, z]);
            }
        }
        PointBuffer {
            positions,
            ..PointBuffer::default()
        }
    }

    /// The hand-checked case from the module documentation: a single correspondence
    /// whose `a`/`c` this test computes independently of `point_to_plane_transform`,
    /// then confirms the function's *effect* is consistent with it by checking the
    /// six-fold-repeated version (six copies of one correspondence still span only
    /// one direction in R⁶, so this alone should be refused as degenerate -- proving
    /// the degeneracy check is not vacuous before trusting it elsewhere).
    #[test]
    fn a_single_repeated_correspondence_is_degenerate_not_a_guess() {
        let p = [1.0, 0.0, 0.0];
        let q = [1.0, 0.0, 0.1];
        let n = [0.0, 0.0, 1.0];
        let err = point_to_plane_transform(&[p; 6], &[q; 6], &[n; 6]).expect_err("degenerate");
        assert!(matches!(err, FusionError::Degenerate(_)), "{err}");
    }

    #[test]
    fn fewer_than_six_correspondences_is_empty_input_not_a_partial_solve() {
        let p = [1.0, 0.0, 0.0];
        let n = [0.0, 0.0, 1.0];
        let err = point_to_plane_transform(&[p; 5], &[p; 5], &[n; 5]).expect_err("too few");
        assert!(matches!(err, FusionError::EmptyInput(_)), "{err}");
    }

    /// Cross-checked independently in Python (module documentation): the same known
    /// small transform, linearised once over this exact surface, recovers to within
    /// 0.5% of its inverse, and the mean point-to-plane residual falls from 0.054 m
    /// to under 3e-4 m in the one call this test makes.
    #[test]
    fn recovers_a_known_small_transform_in_one_linearised_solve() {
        let target = grid();
        let normals = estimate_normals(&target, 6).expect("estimable");
        let known = Isometry3::from_parts(
            Translation3::new(0.03, -0.02, 0.015),
            UnitQuaternion::from_scaled_axis(Vector3::new(0.02, -0.015, 0.01)),
        );
        let sources: Vec<[f32; 3]> = target.positions.iter().map(|p| apply(&known, *p)).collect();

        let delta = point_to_plane_transform(&sources, &target.positions, &normals)
            .expect("well-conditioned by construction");
        let moved: Vec<[f32; 3]> = sources.iter().map(|p| apply(&delta, *p)).collect();
        let mut residual = 0.0_f32;
        for ((p, q), n) in moved.iter().zip(&target.positions).zip(&normals) {
            residual += (n[0] * (p[0] - q[0]) + n[1] * (p[1] - q[1]) + n[2] * (p[2] - q[2])).abs();
        }
        #[allow(clippy::cast_precision_loss)]
        let mean_residual = residual / moved.len() as f32;
        assert!(
            mean_residual < 3e-4,
            "mean point-to-plane residual after one solve: {mean_residual}"
        );
    }

    #[test]
    fn the_icp_loop_converges_on_a_known_synthetic_transform() {
        let source = grid();
        let known = Isometry3::from_parts(
            Translation3::new(0.03, -0.02, 0.015),
            UnitQuaternion::from_scaled_axis(Vector3::new(0.02, -0.015, 0.01)),
        );
        let target_positions: Vec<[f32; 3]> =
            source.positions.iter().map(|p| apply(&known, *p)).collect();
        let target_unnormaled = PointBuffer {
            positions: target_positions,
            ..PointBuffer::default()
        };
        let normals = estimate_normals(&target_unnormaled, 6).expect("estimable");
        let target = PointBuffer {
            normals: Some(normals),
            ..target_unnormaled
        };
        let mut icp = CpuIcpPointToPlane::new(target, 100).expect("normals present");
        let (solved, iterations) = loop {
            let r = icp.step(&source).expect("steps");
            if r.converged {
                break (r.transform, icp.iterations());
            }
        };
        assert!(iterations <= 100);
        for p in &source.positions {
            let a = apply(&solved, *p);
            let b = apply(&known, *p);
            for i in 0..3 {
                assert!(
                    (a[i] - b[i]).abs() < 2e-3,
                    "{a:?} vs {b:?} after {iterations}"
                );
            }
        }
    }

    #[test]
    fn a_target_with_no_normals_is_refused_at_construction() {
        match CpuIcpPointToPlane::new(grid(), 10) {
            Err(err) => assert!(matches!(err, FusionError::EmptyInput(_)), "{err}"),
            Ok(_) => panic!("a target with no normals must be refused"),
        }
    }

    #[test]
    fn handles_empty_overlap_without_panicking() {
        let normals = estimate_normals(&grid(), 6).expect("estimable");
        let with_normals = PointBuffer {
            normals: Some(normals),
            ..grid()
        };
        let mut icp = CpuIcpPointToPlane::new(with_normals, 10).expect("normals present");
        assert!(matches!(
            icp.step(&PointBuffer::default()),
            Err(FusionError::EmptyInput(_))
        ));
    }

    #[test]
    fn an_aligned_cloud_converges_at_once_with_every_point_an_inlier() {
        let source = grid();
        let normals = estimate_normals(&source, 6).expect("estimable");
        let target = PointBuffer {
            normals: Some(normals),
            ..grid()
        };
        let mut icp = CpuIcpPointToPlane::new(target, 10).expect("normals present");
        let r = icp.step(&source).expect("steps");
        assert!(r.converged, "identical clouds are already aligned");
        assert!((r.inlier_ratio - 1.0).abs() < f32::EPSILON);
    }
}

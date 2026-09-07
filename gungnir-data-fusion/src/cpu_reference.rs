//! Pure-CPU ICP reference (GAP-024, `rust-3d-data-ecosystem-build-vs-adopt.md` §3.6
//! step 1): no `wgpu` types at all, unit-tested against known synthetic transforms and
//! the degenerate cases. Also the runtime fallback for headless configurations.
//!
//! **Point-to-point, not point-to-plane.** `PointBuffer` carries no normals, and a normal
//! estimated here from a handful of neighbours would be a guess the GPU path would then
//! be validated against. The correspondence is the nearest target point by brute force,
//! which is `O(n·m)` per iteration and correct; a spatial index is an optimisation for
//! when a cloud is large enough to need one, and the reference should stay legible.

use crate::transform_solve::{apply, rigid_transform_between};
use crate::{FusionError, FusionStepResult, PointCloudFusion};
use gungnir_data::pointcloud::PointBuffer;
use nalgebra::Isometry3;

pub struct CpuIcp {
    pub max_iterations: u32,
    pub target: PointBuffer,
    /// Iteration stops when the mean residual improves by less than this, metres.
    pub convergence_eps_m: f32,
    /// A correspondence closer than this counts as an inlier.
    pub inlier_distance_m: f32,
    estimate: Isometry3<f32>,
    iterations: u32,
    last_mean_residual: Option<f32>,
    converged: bool,
}

impl CpuIcp {
    #[must_use]
    pub fn new(target: PointBuffer, max_iterations: u32) -> Self {
        Self {
            max_iterations,
            target,
            convergence_eps_m: 1e-4,
            inlier_distance_m: 0.5,
            estimate: Isometry3::identity(),
            iterations: 0,
            last_mean_residual: None,
            converged: false,
        }
    }

    /// The current estimate, whatever the convergence state.
    #[must_use]
    pub fn estimate(&self) -> Isometry3<f32> {
        self.estimate
    }

    #[must_use]
    pub fn iterations(&self) -> u32 {
        self.iterations
    }

    /// Nearest target point to `p`, and the distance.
    fn nearest(&self, p: [f32; 3]) -> Option<([f32; 3], f32)> {
        self.target
            .positions
            .iter()
            .map(|t| {
                let d = (t[0] - p[0]).powi(2) + (t[1] - p[1]).powi(2) + (t[2] - p[2]).powi(2);
                (*t, d.sqrt())
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }
}

impl PointCloudFusion for CpuIcp {
    /// One ICP iteration per call: correspond, solve, compose, judge convergence.
    ///
    /// # Errors
    ///
    /// `EmptyInput` when either cloud is empty; `Divergence` once the iteration budget is
    /// spent without convergence; `Degenerate` when the solve cannot produce a rotation.
    fn step(&mut self, source: &PointBuffer) -> Result<FusionStepResult, FusionError> {
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
            .map(|p| apply(&self.estimate, *p))
            .collect();
        let mut pairs_source = Vec::with_capacity(moved.len());
        let mut pairs_target = Vec::with_capacity(moved.len());
        let mut residual = 0.0_f32;
        for p in &moved {
            if let Some((t, d)) = self.nearest(*p) {
                pairs_source.push(*p);
                pairs_target.push(t);
                residual += d;
            }
        }
        #[allow(clippy::cast_precision_loss)]
        let mean_residual = residual / pairs_source.len().max(1) as f32;
        let delta = rigid_transform_between(&pairs_source, &pairs_target)?;
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

impl CpuIcp {
    fn inlier_ratio(&self, source: &PointBuffer) -> f32 {
        let inliers = source
            .positions
            .iter()
            .filter(|p| {
                self.nearest(apply(&self.estimate, **p))
                    .is_some_and(|(_, d)| d <= self.inlier_distance_m)
            })
            .count();
        #[allow(clippy::cast_precision_loss)]
        let ratio = inliers as f32 / source.positions.len().max(1) as f32;
        ratio
    }

    fn result(&self, inlier_ratio: f32) -> FusionStepResult {
        FusionStepResult {
            transform: self.estimate,
            converged: self.converged,
            inlier_ratio,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::{Translation3, UnitQuaternion};

    fn grid() -> PointBuffer {
        let mut positions = Vec::new();
        for i in 0..6 {
            for j in 0..6 {
                #[allow(clippy::cast_precision_loss)]
                let (x, y) = (i as f32 * 0.5, j as f32 * 0.5);
                positions.push([x, y, (x * 1.3 + y * 0.7).sin() * 0.4]);
            }
        }
        PointBuffer {
            positions,
            ..PointBuffer::default()
        }
    }

    fn run(
        target: PointBuffer,
        source: &PointBuffer,
        budget: u32,
    ) -> Result<(Isometry3<f32>, u32), FusionError> {
        let mut icp = CpuIcp::new(target, budget);
        loop {
            let r = icp.step(source)?;
            if r.converged {
                return Ok((r.transform, icp.iterations()));
            }
        }
    }

    #[test]
    fn converges_on_known_synthetic_transform() {
        // A small rotation and translation: ICP's basin of attraction.
        let known = Isometry3::from_parts(
            Translation3::new(0.08, -0.05, 0.03),
            UnitQuaternion::from_euler_angles(0.02, 0.03, 0.06),
        );
        let source = grid();
        let target = PointBuffer {
            positions: source.positions.iter().map(|p| apply(&known, *p)).collect(),
            ..PointBuffer::default()
        };
        let (solved, iterations) = run(target, &source, 100).expect("converges");
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
    fn handles_empty_overlap_without_panicking() {
        let mut icp = CpuIcp::new(PointBuffer::default(), 10);
        assert!(matches!(icp.step(&grid()), Err(FusionError::EmptyInput(_))));
        let mut icp = CpuIcp::new(grid(), 10);
        assert!(matches!(
            icp.step(&PointBuffer::default()),
            Err(FusionError::EmptyInput(_))
        ));
    }

    #[test]
    fn a_spent_budget_is_divergence_not_a_claimed_alignment() {
        // A target too far away for any correspondence to be meaningful, and a budget
        // too small for the residual to settle.
        let source = grid();
        let far = PointBuffer {
            positions: source
                .positions
                .iter()
                .map(|p| [p[0] + 50.0, p[1] - 30.0, p[2]])
                .collect(),
            ..PointBuffer::default()
        };
        let mut icp = CpuIcp::new(far, 1);
        icp.convergence_eps_m = 0.0;
        let _first = icp.step(&source).expect("one iteration runs");
        assert!(matches!(icp.step(&source), Err(FusionError::Divergence)));
    }

    #[test]
    fn an_aligned_cloud_converges_at_once_with_every_point_an_inlier() {
        let source = grid();
        let mut icp = CpuIcp::new(grid(), 10);
        let r = icp.step(&source).expect("steps");
        assert!(r.converged, "identical clouds are already aligned");
        assert!((r.inlier_ratio - 1.0).abs() < f32::EPSILON);
    }
}

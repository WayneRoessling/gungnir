// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! GPU-accelerated point cloud registration/fusion, per
//! rust-3d-data-ecosystem-build-vs-adopt.md §3. Point-to-plane ICP as the primary
//! algorithm (§3.2); NDT correspondence-search swap-in left as a follow-on per that
//! section's recommendation.

pub mod cpu_reference;
pub mod gpu;
pub mod normals;
pub mod point_to_plane;
pub mod transform_solve;

use gungnir_data::pointcloud::PointBuffer;

/// GPU-independent public surface -- `app::state`/`gungnir-viewport3d` depend on this
/// trait, never on the `wgpu` internals directly (§3.5's dependency-inversion rule).
pub trait PointCloudFusion {
    /// Non-blocking: kicks off/continues registration, returns current best
    /// transform estimate and convergence status. Never blocks the caller.
    /// # Errors
    ///
    /// When the implementation cannot take a step. **A `Result` rather than a
    /// `FusionStepResult`** (GAP-082): that struct carries a transform and a `converged`
    /// flag, and any default would be a registration reporting an alignment it never
    /// computed.
    fn step(&mut self, source: &PointBuffer) -> Result<FusionStepResult, FusionError>;
}

#[derive(Debug, Clone)]
pub struct FusionStepResult {
    pub transform: nalgebra::Isometry3<f32>,
    pub converged: bool,
    pub inlier_ratio: f32,
}

#[derive(Debug, thiserror::Error)]
pub enum FusionError {
    #[error("GPU device/pipeline initialization failed: {0}")]
    GpuInit(String),
    #[error("registration did not converge within the iteration budget")]
    Divergence,
    /// A cloud with nothing in it, or too few pairs to solve from (GAP-024). Distinct
    /// from : nothing ran.
    #[error("nothing to register: {0}")]
    EmptyInput(String),
    /// The solve cannot produce a proper rotation from this input: collinear points, a
    /// non-finite covariance.
    #[error("the transform solve is degenerate: {0}")]
    Degenerate(String),
    /// A step of the pipeline that is designed and not written.
    ///
    /// Named rather than panicked (GAP-082). **Distinct from `Divergence`**: a solver that
    /// ran and did not converge and a solver that does not exist are opposite claims about
    /// the same call.
    #[error("{what} is not implemented: waiting on {waiting_on}")]
    NotImplemented {
        what: &'static str,
        waiting_on: &'static str,
    },
}

/// GPU-backed implementation of point-to-point (Kabsch) ICP, correspondence-gated by
/// distance and, when both clouds carry normals, by normal angle (GAP-024).
///
/// **Owns no `wgpu::Device` of its own** -- `device`/`queue` are `Arc` clones of the
/// one `gungnir-render::GpuContext` created, per §3.3 ("avoiding a second GPU
/// context") and `gungnir_render::GpuContext`'s own doc comment on why `Arc` rather
/// than a borrowed reference.
///
/// **Point-to-point, not point-to-plane, by deliberate scope.** The transform solve
/// this engine drives is exactly `crate::transform_solve::solve_rigid_transform`,
/// unmodified -- the same Kabsch/SVD solve `cpu_reference::CpuIcp` uses -- so its
/// result is differentially checkable against `CpuIcp` byte-for-byte
/// (`tests/gpu_vs_cpu.rs`). `crate::point_to_plane`'s linearised 6-DOF normal-equations
/// solve is a materially different reduction (a 6x6 system, not a 3x3
/// cross-covariance) with no shared GPU kernel here; porting it is future work, named
/// rather than silently implied by this type's name. What normals *do* buy here is
/// real: `correspond_and_reject` (`shaders/correspondence.wgsl`) rejects a
/// correspondence whose source/target normals disagree past [`Self::min_normal_cos`],
/// exactly the "rejection by distance and normal angle" stage
/// `rust-3d-data-ecosystem-build-vs-adopt.md` §3.4 calls for -- it is simply a
/// correspondence-quality gate on top of Kabsch, not a switch to a different
/// transform-estimation algorithm.
///
/// **Defaults make this structurally identical to `CpuIcp` in what a well-behaved
/// registration accepts.** [`Self::min_normal_cos`] defaults to
/// [`gpu::params::IcpParams::NORMAL_GATE_DISABLED`] (accepts any angle) and
/// [`Self::max_correspondence_dist`] defaults to a multiple of the target cloud's own
/// mean point spacing (see [`GpuFusionEngine::new`]'s doc comment for exactly how, and
/// why it is not the cloud's bounding-box diagonal) -- generous enough that a
/// correspondence born of a modest misalignment is never spuriously excluded, which is
/// what makes this engine's Kabsch solve agree with `CpuIcp`'s (which never excludes
/// one at all) on the synthetic fixtures `tests/gpu_vs_cpu.rs` checks. The rejection
/// gates are real, working, `naga`-validated code, adjustable by any caller who wants a
/// tighter registration; they default to inert rather than to a threshold this crate
/// would have to guess is right for an arbitrary cloud.
pub struct GpuFusionEngine {
    device: std::sync::Arc<wgpu::Device>,
    queue: std::sync::Arc<wgpu::Queue>,
    pipelines: gpu::pipeline::GpuPipelines,
    grid: gpu::pipeline::TargetGrid,
    scratch: gpu::pipeline::IcpScratch,
    max_iterations: u32,
    /// Iteration stops when the mean residual improves by less than this, metres.
    /// Mirrors `CpuIcp::convergence_eps_m`'s default (1e-4).
    pub convergence_eps_m: f32,
    /// A correspondence closer than this counts as an inlier for the *reported*
    /// ratio; mirrors `CpuIcp::inlier_distance_m`'s default (0.5 m). Distinct from
    /// [`Self::max_correspondence_dist`], which gates what enters the solve at all --
    /// see `gpu::params::IcpParams`'s doc comment on the same two-threshold shape in
    /// `CpuIcp::inlier_ratio`.
    pub inlier_distance_m: f32,
    /// A correspondence farther than this never enters the solve. Defaults to three
    /// times the target cloud's own mean nearest-neighbour spacing, floored at
    /// [`Self::inlier_distance_m`]'s default (`GpuFusionEngine::new`'s doc comment has
    /// the full reasoning). Two invariants to keep changing this after construction:
    ///
    /// - **At or above [`Self::inlier_distance_m`]**: a smaller value would exclude
    ///   from correspondence entirely some points `CpuIcp` would still count as
    ///   inliers, undercounting the ratio relative to it.
    /// - **At or below the spatial hash's cell size** (fixed at construction to this
    ///   field's *default* value, not re-read from here later): a larger value here
    ///   silently breaks the correctness invariant atop `shaders/spatial_hash.wgsl`
    ///   (the 3x3x3 neighbour search is only guaranteed to find everything within one
    ///   cell width), which under-registers rather than errors. A caller that needs a
    ///   genuinely larger correspondence radius needs a new engine, not a larger value
    ///   here -- widening this field alone cannot widen the grid it searches.
    pub max_correspondence_dist: f32,
    /// `abs(dot(source_normal, target_normal))` below this rejects an
    /// otherwise-valid correspondence. `-1.0`
    /// ([`gpu::params::IcpParams::NORMAL_GATE_DISABLED`], the default) accepts every
    /// angle.
    pub min_normal_cos: f32,
    estimate: nalgebra::Isometry3<f32>,
    iterations: u32,
    last_mean_residual: Option<f32>,
    converged: bool,
}

/// Fixed per-cell capacity for the spatial hash (`shaders/spatial_hash.wgsl`'s own
/// doc comment on why a fixed capacity rather than an exact counting sort). 32 is
/// generous for the synthetic fixtures this crate's tests use (tens of points over a
/// few dozen cells) and for LiDAR-class density at `cell_size == max_correspondence_dist`
/// (§3.4's own uniform-density rationale for choosing a grid at all); a cell that
/// overflows it only drops the excess point from the index, never fabricates one.
const MAX_POINTS_PER_CELL: u32 = 32;

/// `CpuIcp::inlier_distance_m`'s default, named here too so
/// [`GpuFusionEngine::new`]'s density-based `max_correspondence_dist` default can be
/// floored at it (see that constructor): a correspondence-count invariant this type's
/// own field doc comments state (keep `max_correspondence_dist >= inlier_distance_m`)
/// is worth enforcing in the defaults that are supposed to demonstrate it, not just
/// asserting of a caller.
const DEFAULT_INLIER_DISTANCE_M: f32 = 0.5;

/// How generous [`GpuFusionEngine::new`]'s density-based `max_correspondence_dist`
/// default is, as a multiple of the target's mean nearest-neighbour spacing -- see
/// that constructor's doc comment for the full reasoning.
const DEFAULT_CORRESPONDENCE_SPACING_SCALE: f32 = 3.0;

/// The mean, over every point in `points`, of its distance to the *nearest other*
/// point in `points` -- brute force, `O(n^2)`, matching `cpu_reference.rs`'s and
/// `normals.rs`'s own stated reasoning for their nearest-neighbour searches: legible,
/// and a spatial index is an optimisation for a cloud large enough to need one.
///
/// `0.0` for zero or one point (nothing to measure a distance between).
fn mean_nearest_neighbor_distance(points: &[[f32; 3]]) -> f32 {
    if points.len() < 2 {
        return 0.0;
    }
    let mut sum = 0.0f32;
    let mut counted = 0u32;
    for (i, p) in points.iter().enumerate() {
        let mut best = f32::INFINITY;
        for (j, q) in points.iter().enumerate() {
            if i == j {
                continue;
            }
            let d = ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt();
            if d < best {
                best = d;
            }
        }
        if best.is_finite() {
            sum += best;
            counted += 1;
        }
    }
    if counted == 0 {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)]
    let mean = sum / counted as f32;
    mean
}

impl GpuFusionEngine {
    /// Builds the compute pipelines, uploads `target` once, and builds its spatial
    /// hash once. `target` never changes for the life of this engine; `step`'s
    /// `source` may. Takes `target` by reference, unlike `CpuIcp::new`: this engine
    /// uploads the cloud's data to GPU buffers and keeps only those, never the
    /// `PointBuffer` itself, so there is nothing here for ownership to buy.
    ///
    /// # Errors
    ///
    /// [`FusionError::EmptyInput`] when `target` is empty. [`FusionError::GpuInit`]
    /// when the target's extent cannot be turned into a usable grid (see
    /// [`gpu::pipeline::GridGeometry::from_bounds`]).
    pub fn new(
        device: std::sync::Arc<wgpu::Device>,
        queue: std::sync::Arc<wgpu::Queue>,
        target: &PointBuffer,
        max_iterations: u32,
    ) -> Result<Self, FusionError> {
        if target.positions.is_empty() {
            return Err(FusionError::EmptyInput("0 target points".into()));
        }
        let pipelines = gpu::pipeline::GpuPipelines::new(&device);

        // The spatial hash's cell size is also `max_correspondence_dist` by default
        // (the correctness invariant atop `shaders/spatial_hash.wgsl`), so this
        // default cannot be the cloud's full bounding-box diagonal the way an
        // earlier version of this engine picked it: that makes the *cell size* span
        // the whole cloud too, crowding every point into one or two cells regardless
        // of [`MAX_POINTS_PER_CELL`], and the resulting index drops most of them.
        // **Found on real hardware, not by inspection**: `tests/gpu_vs_cpu.rs`'s
        // `gpu_an_aligned_cloud_converges_at_once_with_every_point_an_inlier` failed
        // to converge in one step registering a cloud against itself -- a case with
        // zero actual correspondence-search difficulty -- and the fix here is what
        // turned it green (`gpu-fusion.yml`'s dispatch on this branch; see the PR).
        //
        // The default instead scales with the target's own point density: the mean
        // distance from each target point to its nearest other target point,
        // brute-force (the same reasoning `cpu_reference.rs`/`normals.rs` give for
        // their own brute-force search: legible, and a spatial index is an
        // optimisation for a cloud large enough to need one), times a factor generous
        // enough that a correspondence born of a modest misalignment is never
        // spuriously excluded, small enough that points spread across many cells
        // rather than a few overcrowded ones.
        let mean_spacing = mean_nearest_neighbor_distance(&target.positions);
        let default_correspondence_dist = if mean_spacing.is_finite() && mean_spacing > 0.0 {
            // Floored at `DEFAULT_INLIER_DISTANCE_M`: a target dense enough that
            // three times its own point spacing is still under 0.5 m would otherwise
            // default to a `max_correspondence_dist` smaller than
            // `inlier_distance_m`, which `Self::max_correspondence_dist`'s own doc
            // comment says to avoid (it would exclude from the solve some
            // correspondences `CpuIcp` still counts as inliers, undercounting the
            // ratio relative to it).
            (mean_spacing * DEFAULT_CORRESPONDENCE_SPACING_SCALE).max(DEFAULT_INLIER_DISTANCE_M)
        } else {
            // Every target point coincides with another (mean spacing 0), or there is
            // only one point: nothing meaningful to register against either way, and
            // this value only has to keep construction from failing on a merely
            // degenerate (as opposed to empty) target.
            1.0
        };

        let grid = gpu::pipeline::TargetGrid::new(
            &device,
            &queue,
            &pipelines,
            target,
            default_correspondence_dist,
            MAX_POINTS_PER_CELL,
        )?;

        Ok(Self {
            device,
            queue,
            pipelines,
            grid,
            scratch: gpu::pipeline::IcpScratch::new(),
            max_iterations,
            convergence_eps_m: 1e-4,
            inlier_distance_m: DEFAULT_INLIER_DISTANCE_M,
            max_correspondence_dist: default_correspondence_dist,
            min_normal_cos: gpu::params::IcpParams::NORMAL_GATE_DISABLED,
            estimate: nalgebra::Isometry3::identity(),
            iterations: 0,
            last_mean_residual: None,
            converged: false,
        })
    }

    /// The shared device this engine dispatches on (owned by `gungnir-render`).
    #[must_use]
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    #[must_use]
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// ICP iteration budget per `step` call (§3.4: bounded so a poorly converging
    /// registration never stalls the render loop).
    #[must_use]
    pub fn max_iterations(&self) -> u32 {
        self.max_iterations
    }

    /// The current estimate, whatever the convergence state. Mirrors
    /// `CpuIcp::estimate`.
    #[must_use]
    pub fn estimate(&self) -> nalgebra::Isometry3<f32> {
        self.estimate
    }

    /// Mirrors `CpuIcp::iterations`.
    #[must_use]
    pub fn iterations(&self) -> u32 {
        self.iterations
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn params_for(&self, num_source_points: usize, has_normals: bool) -> gpu::params::IcpParams {
        let r = self.estimate.rotation.to_rotation_matrix();
        let m = r.matrix();
        let t = self.estimate.translation.vector;
        gpu::params::IcpParams {
            grid_origin: self.grid.geometry.origin,
            cell_size: self.grid.geometry.cell_size,
            grid_dim: self.grid.geometry.dim,
            max_per_cell: MAX_POINTS_PER_CELL,
            num_source_points: num_source_points as u32,
            num_target_points: self.grid.num_target_points as u32,
            has_normals,
            rotation: [
                [m[(0, 0)], m[(0, 1)], m[(0, 2)]],
                [m[(1, 0)], m[(1, 1)], m[(1, 2)]],
                [m[(2, 0)], m[(2, 1)], m[(2, 2)]],
            ],
            translation: [t.x, t.y, t.z],
            max_correspondence_dist: self.max_correspondence_dist,
            min_normal_cos: self.min_normal_cos,
            inlier_distance: self.inlier_distance_m,
        }
    }

    fn result(&self, inlier_ratio: f32) -> FusionStepResult {
        FusionStepResult {
            transform: self.estimate,
            converged: self.converged,
            inlier_ratio,
        }
    }
}

impl PointCloudFusion for GpuFusionEngine {
    /// One ICP iteration per call: apply the current estimate to `source`
    /// (`apply_transform`), find and gate its nearest-neighbour correspondences
    /// against the target's spatial hash (`correspond_and_reject`), reduce them to
    /// the Kabsch cross-covariance (`reduce`), solve and compose
    /// (`crate::transform_solve::solve_rigid_transform`, on the CPU, per §3.4 stage
    /// 5), judge convergence -- the same shape `CpuIcp::step` drives, over GPU
    /// kernels instead of a brute-force CPU search.
    ///
    /// **`inlier_ratio` uses the pre-update estimate's correspondence pass**, not a
    /// second pass with the post-update estimate the way `CpuIcp::inlier_ratio`
    /// re-derives it. The two agree to within floating-point precision once
    /// convergence is reached, because convergence is defined as the point where an
    /// iteration's own solved correction becomes negligible
    /// (`|last - mean_residual| < convergence_eps_m`) -- which is exactly when the
    /// pre- and post-update estimates stop differing enough to move a correspondence
    /// across the inlier threshold. `tests/gpu_vs_cpu.rs` checks the *converged*
    /// ratio against `CpuIcp`'s, which is where this approximation is tightest, not
    /// an arbitrary intermediate iteration where it would be loosest.
    ///
    /// # Errors
    ///
    /// `EmptyInput` when either cloud is empty, or when fewer than three
    /// correspondences survive the rejection gates (the least
    /// `transform_solve::solve_rigid_transform` can determine a rotation from).
    /// `Divergence` once the iteration budget is spent without convergence.
    /// `Degenerate` as `solve_rigid_transform`.
    fn step(&mut self, source: &PointBuffer) -> Result<FusionStepResult, FusionError> {
        if source.positions.is_empty() || self.grid.num_target_points == 0 {
            return Err(FusionError::EmptyInput(format!(
                "{} source and {} target points",
                source.positions.len(),
                self.grid.num_target_points
            )));
        }
        if self.converged {
            let ratio = self.dispatch_and_measure(source)?.1;
            return Ok(self.result(ratio));
        }
        if self.iterations >= self.max_iterations {
            return Err(FusionError::Divergence);
        }

        let (sums, inlier_ratio) = self.dispatch_and_measure(source)?;
        if sums.n < 3.0 {
            return Err(FusionError::EmptyInput(format!(
                "{} valid correspondence(s); three matched pairs are the least a \
                 rigid transform can be solved from",
                sums.n
            )));
        }
        let Some((h, source_centroid, target_centroid)) = sums.cross_covariance() else {
            return Err(FusionError::EmptyInput(
                "no valid correspondences survived the rejection gates".into(),
            ));
        };
        let delta_rotation = transform_solve::solve_rigid_transform(&h)?.rotation;
        let source_centroid =
            nalgebra::Vector3::new(source_centroid[0], source_centroid[1], source_centroid[2]);
        let target_centroid =
            nalgebra::Vector3::new(target_centroid[0], target_centroid[1], target_centroid[2]);
        let delta_translation = target_centroid - delta_rotation * source_centroid;
        let delta = nalgebra::Isometry3::from_parts(
            nalgebra::Translation3::from(delta_translation),
            delta_rotation,
        );
        self.estimate = delta * self.estimate;
        self.iterations += 1;

        let mean_residual = sums.sum_residual / sums.n;
        let improved = self
            .last_mean_residual
            .is_some_and(|last| (last - mean_residual).abs() < self.convergence_eps_m);
        self.last_mean_residual = Some(mean_residual);
        self.converged = improved || mean_residual < self.convergence_eps_m;

        Ok(self.result(inlier_ratio))
    }
}

impl GpuFusionEngine {
    /// `apply_transform` + `correspond_and_reject` + `reduce` for the *current*
    /// estimate (before any update this call might go on to make), returning both
    /// the reduced sums (for the solve) and the inlier ratio they already imply --
    /// see [`PointCloudFusion::step`]'s doc comment on why one pass serves both.
    ///
    /// # Errors
    ///
    /// Propagates [`gpu::pipeline::dispatch_reduce`]'s errors (GPU buffer readback).
    #[allow(clippy::cast_precision_loss)]
    fn dispatch_and_measure(
        &mut self,
        source: &PointBuffer,
    ) -> Result<(gpu::pipeline::ReducedSums, f32), FusionError> {
        let n = source.positions.len();
        let has_normals = self.grid.has_target_normals && source.normals.is_some();
        let params = self.params_for(n, has_normals);

        let usage = gpu::pipeline::storage_rw_usage();
        let positions_bytes = gpu::buffers::flatten_positions(&source.positions);
        self.scratch.source_positions.ensure_capacity(
            &self.device,
            "gungnir-fusion-source-positions",
            n,
            usage,
        );
        if let Some(buf) = self.scratch.source_positions.buffer.as_ref() {
            self.queue.write_buffer(buf, 0, &positions_bytes);
        }
        self.scratch.source_normals.ensure_capacity(
            &self.device,
            "gungnir-fusion-source-normals",
            n,
            usage,
        );
        if has_normals {
            if let (Some(buf), Some(normals)) =
                (self.scratch.source_normals.buffer.as_ref(), &source.normals)
            {
                self.queue
                    .write_buffer(buf, 0, &gpu::buffers::flatten_positions(normals));
            }
        }
        self.scratch.transformed_positions.ensure_capacity(
            &self.device,
            "gungnir-fusion-transformed-positions",
            n,
            usage,
        );
        self.scratch.transformed_normals.ensure_capacity(
            &self.device,
            "gungnir-fusion-transformed-normals",
            n,
            usage,
        );
        self.scratch.correspondence_target.ensure_capacity(
            &self.device,
            "gungnir-fusion-correspondence-target",
            n,
            usage,
        );

        gpu::pipeline::dispatch_transform_and_correspond(
            &self.device,
            &self.queue,
            &self.pipelines,
            &self.grid,
            &self.scratch,
            &params,
            n,
        );
        let sums = gpu::pipeline::dispatch_reduce(
            &self.device,
            &self.queue,
            &self.pipelines,
            &self.grid,
            &mut self.scratch,
            &params,
            n,
        )?;
        let inlier_ratio = if n > 0 { sums.n_inlier / n as f32 } else { 0.0 };
        Ok((sums, inlier_ratio))
    }
}

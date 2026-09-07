// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! GPU-accelerated point cloud registration/fusion, per
//! rust-3d-data-ecosystem-build-vs-adopt.md §3. Point-to-plane ICP as the primary
//! algorithm (§3.2); NDT correspondence-search swap-in left as a follow-on per that
//! section's recommendation.

pub mod cpu_reference;
pub mod gpu;
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

/// GPU-backed implementation. Owns no `wgpu::Device` of its own -- shares the one
/// already created by gungnir-render, per §3.3 ("avoiding a second GPU context").
pub struct GpuFusionEngine<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    max_iterations: u32,
}

impl<'a> GpuFusionEngine<'a> {
    pub fn new(device: &'a wgpu::Device, queue: &'a wgpu::Queue, max_iterations: u32) -> Self {
        Self {
            device,
            queue,
            max_iterations,
        }
    }

    /// The shared device this engine dispatches on (owned by `gungnir-render`).
    pub fn device(&self) -> &wgpu::Device {
        self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        self.queue
    }

    /// ICP iteration budget per `step` call (§3.4: bounded so a poorly converging
    /// registration never stalls the render loop).
    pub fn max_iterations(&self) -> u32 {
        self.max_iterations
    }
}

impl PointCloudFusion for GpuFusionEngine<'_> {
    fn step(&mut self, _source: &PointBuffer) -> Result<FusionStepResult, FusionError> {
        Err(FusionError::NotImplemented {
            what: "the GPU ICP step (spatial_hash, correspondence, reduction, fuse_voxels)",
            waiting_on: "the WGSL pipeline of §3.4, which is not written",
        })
    }
}

// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Point-cloud registration backend selection (GAP-024).
//!
//! `gungnir_render::GpuContext::new` was never called by any binary before this
//! change -- `ARCHITECTURE.md` §7.1 drew the `gungnir-app` -> `gungnir-render` and
//! `gungnir-app` -> `gungnir-data-fusion` manifest edges, but no source file
//! referenced either crate. [`FusionBackend::init`] is the caller: it constructs the
//! compute device once, at start-up (`state::AppState::with_config_and_store`,
//! before anything else touches it), and falls back to the CPU reference on
//! `RenderError::NoAdapter` per `rust-3d-data-ecosystem-build-vs-adopt.md` §3.6 rule
//! 3 -- a full, honest fallback, never a silently-degraded one.
//!
//! **What this wires, and what it does not.** [`FusionBackend::engine_for`] is real,
//! working code: given a target cloud, it returns a
//! [`gungnir_data_fusion::GpuFusionEngine`] on the GPU path or a
//! [`gungnir_data_fusion::cpu_reference::CpuIcp`] on the fallback, both behind the
//! same [`PointCloudFusion`] trait object so a caller never has to know which it
//! got. Nothing in `update::tick` calls it: no point cloud ever reaches
//! `DataStore.point_clouds` today (GAP-098's own finding -- no configuration field
//! names one, and the only `LoadRequest` either binary sends is `Terrain`), so there
//! is no target to build an engine for. Constructing one anyway, against an empty or
//! fabricated cloud, would be exactly the fake wiring this workspace's culture
//! refuses: a registration engine reporting readiness for data it never received.
//! This module is therefore in the same state several productization crates were
//! before their own gaps closed -- wired and tested, waiting on a caller with real
//! data -- and `AppState::fusion`'s doc comment says so rather than implying
//! otherwise.

use gungnir_data::pointcloud::PointBuffer;
use gungnir_data_fusion::cpu_reference::CpuIcp;
use gungnir_data_fusion::{FusionError, GpuFusionEngine, PointCloudFusion};
use gungnir_render::{GpuContext, RenderError};
use std::sync::Arc;

/// Which registration backend this desktop constructed at start-up, and why, if it
/// fell back. Mirrors `gungnir_security::EncryptionStatus`'s shape: derived once at
/// start-up, from what actually happened rather than from what was hoped for.
pub enum FusionBackend {
    /// A `wgpu` compute device was created; [`FusionBackend::engine_for`] returns a
    /// GPU-backed engine.
    Gpu {
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
    },
    /// No usable GPU adapter, or device creation itself failed;
    /// [`FusionBackend::engine_for`] returns the CPU reference.
    Cpu { reason: String },
}

impl FusionBackend {
    /// Constructs the compute device once, blocking on the same runtime the rest of
    /// the desktop's start-up already blocks on for embedded-service wiring
    /// (`state::desktop_runtime`, `state::build_backends`).
    #[must_use]
    pub fn init(runtime: &tokio::runtime::Handle) -> Self {
        match runtime.block_on(GpuContext::new()) {
            Ok(ctx) => Self::Gpu {
                device: ctx.device,
                queue: ctx.queue,
            },
            Err(RenderError::NoAdapter) => Self::Cpu {
                reason: "no suitable GPU adapter".into(),
            },
            Err(RenderError::GpuInit(reason)) => Self::Cpu { reason },
        }
    }

    #[must_use]
    pub fn is_gpu(&self) -> bool {
        matches!(self, Self::Gpu { .. })
    }

    /// A human-readable line for a health panel, once one reads this field (no
    /// panel does yet -- see this module's own doc comment).
    #[must_use]
    pub fn status_text(&self) -> String {
        match self {
            Self::Gpu { .. } => "GPU point-cloud registration: compute device ready".into(),
            Self::Cpu { reason } => {
                format!("GPU point-cloud registration unavailable ({reason}); CPU reference in use")
            }
        }
    }

    /// Builds a registration engine for `target`: GPU-backed when one is available,
    /// the CPU reference otherwise -- the fallback
    /// `rust-3d-data-ecosystem-build-vs-adopt.md` §3.6 rule 3 requires. Returned
    /// behind [`PointCloudFusion`] (§3.5's dependency-inversion rule) so a caller
    /// depends on the trait, never on which concrete engine it got.
    ///
    /// # Errors
    ///
    /// Whatever the chosen engine's own constructor returns:
    /// [`FusionError::EmptyInput`] for an empty target, or on the GPU path,
    /// [`FusionError::GpuInit`] if the target's extent cannot be turned into a
    /// usable spatial-hash grid (`gungnir_data_fusion::gpu::pipeline::GridGeometry`).
    pub fn engine_for(
        &self,
        target: &PointBuffer,
        max_iterations: u32,
    ) -> Result<Box<dyn PointCloudFusion>, FusionError> {
        match self {
            Self::Gpu { device, queue } => {
                let engine = GpuFusionEngine::new(
                    Arc::clone(device),
                    Arc::clone(queue),
                    target,
                    max_iterations,
                )?;
                Ok(Box::new(engine))
            }
            Self::Cpu { .. } => Ok(Box::new(CpuIcp::new(target.clone(), max_iterations))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_cloud() -> PointBuffer {
        PointBuffer {
            positions: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
            ],
            ..PointBuffer::default()
        }
    }

    /// The CPU fallback path needs no GPU at all, so it is checkable in plain
    /// `cargo test`: given `FusionBackend::Cpu`, `engine_for` returns a working
    /// engine that steps without error.
    #[test]
    fn cpu_backend_builds_a_working_engine() {
        let backend = FusionBackend::Cpu {
            reason: "test: no adapter requested".into(),
        };
        assert!(!backend.is_gpu());
        assert!(backend.status_text().contains("CPU reference"));

        let target = tiny_cloud();
        let mut engine = backend.engine_for(&target, 10).expect("cpu engine builds");
        let source = tiny_cloud();
        let result = engine.step(&source).expect("cpu engine steps");
        assert!(result.converged, "an aligned cloud converges at once");
    }

    /// `engine_for` refuses an empty target the same way `CpuIcp::new` +
    /// `PointCloudFusion::step` and `GpuFusionEngine::new` each independently do --
    /// checked here on the CPU path, which needs no GPU to exercise.
    #[test]
    fn engine_for_refuses_to_build_against_nothing_on_the_cpu_path() {
        let backend = FusionBackend::Cpu {
            reason: "test".into(),
        };
        let empty = PointBuffer::default();
        // `CpuIcp::new` does not itself refuse an empty target (it refuses at the
        // first `step`, matching `cpu_reference.rs`'s own tests); confirm that
        // remains true through this wrapper rather than assuming it.
        let mut engine = backend
            .engine_for(&empty, 10)
            .expect("construction alone succeeds");
        assert!(matches!(
            engine.step(&tiny_cloud()),
            Err(FusionError::EmptyInput(_))
        ));
    }
}

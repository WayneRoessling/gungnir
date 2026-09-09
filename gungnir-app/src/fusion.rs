// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Point-cloud registration backend selection (GAP-024).
//!
//! `gungnir_render::GpuContext::new` was never called by any binary before this
//! change -- `ARCHITECTURE.md` §7.1 drew the `gungnir-app` -> `gungnir-render` and
//! `gungnir-app` -> `gungnir-data-fusion` manifest edges, but no source file
//! referenced either crate. [`FusionBackend`] is the caller.
//!
//! **Lazy, not eager, and that is load-bearing.** [`FusionBackend::new`] constructs
//! nothing: it returns [`FusionBackend::Uninitialized`], touching no `wgpu` API at
//! all. The compute device is requested only inside [`FusionBackend::engine_for`],
//! the first time a caller actually asks for an engine, and the result is memoized
//! from then on. This is not a style choice -- an earlier version of this module
//! called `GpuContext::new` eagerly from `state::AppState::with_config_and_store`,
//! which every one of `gungnir-app`'s several hundred integration tests calls to
//! build the `AppState` it tests against. That made plain `cargo test` request a
//! real `wgpu` device, over and over, from whatever test happened to run --
//! violating the workspace-wide rule that GPU point-cloud registration is validated
//! outside plain `cargo test`, never inside it (`docs/agentic-workflow.md`'s own
//! description of `gungnir-data-fusion`'s two gates), and it was slow enough under
//! the resulting contention to look like a hang. Lazy construction fixes both: no
//! test that never calls `engine_for` ever touches a GPU. `update::tick` calls it
//! now, through `crate::pointcloud::register` (see below), but only once a real
//! point-cloud pair has finished loading (GAP-098) -- so a test that configures no
//! pair, which is every test in this crate outside two (`gungnir-app/tests/
//! pointcloud.rs`'s real-fixture pair and `crate::pointcloud`'s own synthetic one),
//! still never resolves this backend at all, and those two force the CPU path
//! explicitly rather than let resolution touch a real device, the same way this
//! module's own tests below do.
//!
//! Falls back to the CPU reference on `RenderError::NoAdapter` per
//! `rust-3d-data-ecosystem-build-vs-adopt.md` §3.6 rule 3 -- a full, honest
//! fallback, never a silently-degraded one.
//!
//! **What this wires, and what it does not.** [`FusionBackend::engine_for`] is real,
//! working code: given a target cloud, it returns a
//! [`gungnir_data_fusion::GpuFusionEngine`] on the GPU path or a
//! [`gungnir_data_fusion::cpu_reference::CpuIcp`] on the fallback, both behind the
//! same [`PointCloudFusion`] trait object so a caller never has to know which it
//! got. **`crate::pointcloud::register` is that caller**, closed the same day
//! GAP-098 gave it a real pair to build against: called from `update::tick` right
//! after `crate::pointcloud::poll`, it builds an engine once a loaded source/target
//! pair is sitting in `DataStore.point_clouds` and steps it one tick at a time from
//! then on -- never against an empty or fabricated cloud, which would be exactly the
//! fake wiring this workspace's culture refuses: a registration engine reporting
//! readiness for data it never received. `AppState::fusion`'s own doc comment and
//! `gungnir_ui::panels::sensor_health::PointCloudRegistrationLine` (PN-09) both name
//! which backend actually ran, never implying more than what did.

use gungnir_data::pointcloud::PointBuffer;
use gungnir_data_fusion::cpu_reference::CpuIcp;
use gungnir_data_fusion::{FusionError, GpuFusionEngine, PointCloudFusion};
use gungnir_render::{GpuContext, RenderError};
use std::sync::Arc;

/// Which registration backend this desktop is using, lazily resolved. Mirrors
/// `gungnir_security::EncryptionStatus`'s shape once resolved: derived from what
/// actually happened rather than from what was hoped for, never optimistic.
pub enum FusionBackend {
    /// No `wgpu` call has been made yet. [`FusionBackend::engine_for`] resolves
    /// this to `Gpu` or `Cpu` on its first call and remembers the answer.
    Uninitialized,
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
    /// Touches no `wgpu` API. See this module's doc comment for why construction is
    /// deliberately inert.
    #[must_use]
    pub fn new() -> Self {
        Self::Uninitialized
    }

    #[must_use]
    pub fn is_gpu(&self) -> bool {
        matches!(self, Self::Gpu { .. })
    }

    /// A human-readable line for a health panel. PN-09's own line
    /// (`gungnir_ui::panels::sensor_health::PointCloudRegistrationLine`, built by
    /// `crate::pointcloud::registration_line`) matches this enum's variants directly
    /// rather than calling this method, so it can colour the GPU path, the CPU
    /// fallback and "no pair configured" differently; this is the plain-string form
    /// for anything -- today, only this module's own tests -- that just wants one line.
    #[must_use]
    pub fn status_text(&self) -> String {
        match self {
            Self::Uninitialized => "GPU point-cloud registration: not yet probed".into(),
            Self::Gpu { .. } => "GPU point-cloud registration: compute device ready".into(),
            Self::Cpu { reason } => {
                format!("GPU point-cloud registration unavailable ({reason}); CPU reference in use")
            }
        }
    }

    /// Resolves [`Self::Uninitialized`] to `Gpu` or `Cpu`, blocking on `runtime` the
    /// same way the rest of the desktop's start-up blocks on it for embedded-service
    /// wiring (`state::desktop_runtime`, `state::build_backends`). A no-op once
    /// already resolved.
    fn ensure_resolved(&mut self, runtime: &tokio::runtime::Handle) {
        if !matches!(self, Self::Uninitialized) {
            return;
        }
        *self = match runtime.block_on(GpuContext::new()) {
            Ok(ctx) => Self::Gpu {
                device: ctx.device,
                queue: ctx.queue,
            },
            Err(RenderError::NoAdapter) => Self::Cpu {
                reason: "no suitable GPU adapter".into(),
            },
            Err(RenderError::GpuInit(reason)) => Self::Cpu { reason },
        };
    }

    /// Builds a registration engine for `target`: GPU-backed when one is available,
    /// the CPU reference otherwise -- the fallback
    /// `rust-3d-data-ecosystem-build-vs-adopt.md` §3.6 rule 3 requires. Returned
    /// behind [`PointCloudFusion`] (§3.5's dependency-inversion rule) so a caller
    /// depends on the trait, never on which concrete engine it got.
    ///
    /// The first call against any given `FusionBackend` is the one that actually
    /// requests a `wgpu` device (or decides there is none); `runtime` is only ever
    /// blocked on that once.
    ///
    /// # Errors
    ///
    /// Whatever the chosen engine's own constructor returns:
    /// [`FusionError::EmptyInput`] for an empty target, or on the GPU path,
    /// [`FusionError::GpuInit`] if the target's extent cannot be turned into a
    /// usable spatial-hash grid (`gungnir_data_fusion::gpu::pipeline::GridGeometry`),
    /// or if this backend somehow remains `Uninitialized` after resolution was
    /// attempted (never observed; guarded rather than assumed).
    pub fn engine_for(
        &mut self,
        runtime: &tokio::runtime::Handle,
        target: &PointBuffer,
        max_iterations: u32,
    ) -> Result<Box<dyn PointCloudFusion>, FusionError> {
        self.ensure_resolved(runtime);
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
            Self::Uninitialized => Err(FusionError::GpuInit(
                "fusion backend did not resolve to a concrete backend".into(),
            )),
        }
    }
}

impl Default for FusionBackend {
    fn default() -> Self {
        Self::new()
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

    /// `FusionBackend::new` touches no `wgpu` API -- the whole point of making
    /// resolution lazy -- so this is checkable without a runtime at all.
    #[test]
    fn new_is_uninitialized_and_touches_nothing() {
        let backend = FusionBackend::new();
        assert!(!backend.is_gpu());
        assert!(matches!(backend, FusionBackend::Uninitialized));
        assert_eq!(
            backend.status_text(),
            "GPU point-cloud registration: not yet probed"
        );
    }

    /// The CPU fallback path needs no GPU at all, so it is checkable in plain
    /// `cargo test`: given `FusionBackend::Cpu` directly (bypassing resolution, so
    /// this test needs no runtime either), `engine_for` returns a working engine
    /// that steps without error.
    #[test]
    fn cpu_backend_builds_a_working_engine() {
        let mut backend = FusionBackend::Cpu {
            reason: "test: no adapter requested".into(),
        };
        assert!(!backend.is_gpu());
        assert!(backend.status_text().contains("CPU reference"));

        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("a current-thread runtime for this test");
        let target = tiny_cloud();
        let mut engine = backend
            .engine_for(&runtime.handle().clone(), &target, 10)
            .expect("cpu engine builds");
        let source = tiny_cloud();
        let result = engine.step(&source).expect("cpu engine steps");
        assert!(result.converged, "an aligned cloud converges at once");
    }

    /// `engine_for` refuses an empty target the same way `CpuIcp::new` +
    /// `PointCloudFusion::step` and `GpuFusionEngine::new` each independently do --
    /// checked here on the CPU path, which needs no GPU to exercise.
    #[test]
    fn engine_for_refuses_to_build_against_nothing_on_the_cpu_path() {
        let mut backend = FusionBackend::Cpu {
            reason: "test".into(),
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("a current-thread runtime for this test");
        let empty = PointBuffer::default();
        // `CpuIcp::new` does not itself refuse an empty target (it refuses at the
        // first `step`, matching `cpu_reference.rs`'s own tests); confirm that
        // remains true through this wrapper rather than assuming it.
        let mut engine = backend
            .engine_for(&runtime.handle().clone(), &empty, 10)
            .expect("construction alone succeeds");
        assert!(matches!(
            engine.step(&tiny_cloud()),
            Err(FusionError::EmptyInput(_))
        ));
    }
}

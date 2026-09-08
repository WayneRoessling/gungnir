// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! GPU-vs-CPU differential tests (GAP-024), the `verification-capability-table.md`
//! §2 row for `gungnir-data-fusion` "GPU path against CPU reference": same synthetic
//! fixtures, transform within 1e-3 (m, rad) of the CPU result, inlier ratio within
//! 0.01, run outside plain `cargo test`.
//!
//! **`#[ignore]`d, behind `gpu-tests`, on purpose** -- `docs/agentic-workflow.md`'s
//! own note on this crate's two gates: this file needs a real `wgpu` adapter, which
//! `gpu-fusion.yml` supplies (`gungnir-rtx-5060ti`) and this crate's own `cargo test`
//! never does. Written and compiling is not the same claim as passing on hardware;
//! see the PR this file was introduced in for exactly which of the two is true as of
//! that change.
//!
//! No dependency on `gungnir-render` here: `GpuFusionEngine::new` takes a plain
//! `wgpu::Device`/`Queue` (§3.5's dependency-inversion point -- this crate never
//! needs to know `gungnir-render::GpuContext` exists), and adding that crate as a
//! dev-dependency purely to reuse its ~15-line adapter/device request would be a new
//! Cargo.toml edge `ARCHITECTURE.md` does not show. [`gpu_context`] below duplicates
//! that request directly against `wgpu`, which this crate already depends on.

#![cfg(feature = "gpu-tests")]

use gungnir_data::pointcloud::PointBuffer;
use gungnir_data_fusion::cpu_reference::CpuIcp;
use gungnir_data_fusion::normals::estimate_normals;
use gungnir_data_fusion::{FusionStepResult, GpuFusionEngine, PointCloudFusion};
use nalgebra::{Isometry3, Translation3, UnitQuaternion, Vector3};
use std::sync::Arc;

/// Same construction `gungnir_render::GpuContext::new` uses; see this file's own doc
/// comment on why it is not reused directly from that crate. Panics (test-only,
/// exempt from the workspace's unwrap policy) rather than skipping quietly: a
/// `#[ignore]`d test run with `--ignored` on a runner that names itself `gpu` is
/// supposed to have a GPU, and a silent skip here would be exactly the kind of fake
/// green this workspace's culture refuses.
async fn gpu_context() -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
    let instance = wgpu::Instance::default();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        })
        .await
        .expect("gpu-tests requires a real wgpu adapter (gpu-fusion.yml's runner)");
    let (device, queue) = adapter
        .request_device(
            &wgpu::DeviceDescriptor {
                label: Some("gungnir-fusion-gpu-tests"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        )
        .await
        .expect("device request should succeed once an adapter is found");
    (Arc::new(device), Arc::new(queue))
}

/// `cpu_reference.rs`'s own fixture, reproduced rather than imported (it is private
/// to that module's `#[cfg(test)]` block): a 6x6 grid at 0.5 m spacing,
/// `z = sin(1.3x + 0.7y) * 0.4`. Fine for Kabsch/point-to-point (unlike the
/// point-to-plane linearised solve, which `point_to_plane.rs`'s own module
/// documentation found this exact surface numerically degenerate for) -- proven by
/// `cpu_reference::tests::converges_on_known_synthetic_transform` passing against it.
fn point_to_point_grid() -> PointBuffer {
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

/// `point_to_plane.rs`'s own fixture: two sinusoidal terms, well-conditioned for
/// normal estimation over a larger patch than the single-term surface above.
/// Reproduced for the same reason.
fn two_term_grid() -> PointBuffer {
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

fn apply(t: &Isometry3<f32>, p: [f32; 3]) -> [f32; 3] {
    let q = t * nalgebra::Point3::new(p[0], p[1], p[2]);
    [q.x, q.y, q.z]
}

/// Runs `CpuIcp` to convergence exactly as `cpu_reference.rs`'s own tests do.
fn run_cpu(target: PointBuffer, source: &PointBuffer, budget: u32) -> (FusionStepResult, u32) {
    let mut icp = CpuIcp::new(target, budget);
    loop {
        let r = icp.step(source).expect("cpu icp steps");
        if r.converged {
            return (r, icp.iterations());
        }
    }
}

/// Runs [`GpuFusionEngine`] to convergence the same way.
fn run_gpu(
    engine: &mut GpuFusionEngine,
    source: &PointBuffer,
    budget: u32,
) -> (FusionStepResult, u32) {
    let mut iterations = 0;
    loop {
        let r = engine.step(source).expect("gpu icp steps");
        iterations += 1;
        if r.converged || iterations >= budget {
            return (r, iterations);
        }
    }
}

/// The angular distance (radians) between two rotations, for comparing recovered
/// transforms without caring about the sign/axis convention either side happened to
/// land on.
fn angle_between(a: &UnitQuaternion<f32>, b: &UnitQuaternion<f32>) -> f32 {
    (a.inverse() * b).angle()
}

/// `verification-capability-table.md` §2's row, checked directly: the GPU path
/// recovers the same known synthetic transform `cpu_reference.rs`'s own test does,
/// and agrees with `CpuIcp`'s own result on the same fixture within 1e-3 (m, rad)
/// for the transform and 0.01 for the inlier ratio.
#[tokio::test]
#[ignore = "needs a real wgpu adapter; run with --features gpu-tests -- --ignored on a GPU host"]
async fn gpu_matches_cpu_reference_transform_and_inlier_ratio() {
    let known = Isometry3::from_parts(
        Translation3::new(0.08, -0.05, 0.03),
        UnitQuaternion::from_euler_angles(0.02, 0.03, 0.06),
    );
    let source = point_to_point_grid();
    let target = PointBuffer {
        positions: source.positions.iter().map(|p| apply(&known, *p)).collect(),
        ..PointBuffer::default()
    };

    let (cpu_result, cpu_iterations) = run_cpu(target.clone(), &source, 100);

    let (device, queue) = gpu_context().await;
    let mut gpu_engine =
        GpuFusionEngine::new(device, queue, &target, 100).expect("target is non-empty");
    let (gpu_result, gpu_iterations) = run_gpu(&mut gpu_engine, &source, 100);

    let translation_diff =
        (cpu_result.transform.translation.vector - gpu_result.transform.translation.vector).norm();
    let rotation_diff = angle_between(
        &cpu_result.transform.rotation,
        &gpu_result.transform.rotation,
    );
    let inlier_diff = (cpu_result.inlier_ratio - gpu_result.inlier_ratio).abs();

    assert!(
        translation_diff < 1e-3,
        "translation differs by {translation_diff} m (cpu {} iterations, gpu {} iterations); \
         cpu={:?} gpu={:?}",
        cpu_iterations,
        gpu_iterations,
        cpu_result.transform.translation,
        gpu_result.transform.translation,
    );
    assert!(
        rotation_diff < 1e-3,
        "rotation differs by {rotation_diff} rad; cpu={:?} gpu={:?}",
        cpu_result.transform.rotation,
        gpu_result.transform.rotation,
    );
    assert!(
        inlier_diff < 0.01,
        "inlier ratio differs by {inlier_diff}: cpu={} gpu={}",
        cpu_result.inlier_ratio,
        gpu_result.inlier_ratio,
    );

    // Both sides should also have recovered something close to the transform that
    // was actually injected, the way `cpu_reference.rs`'s own test checks -- not just
    // close to each other, in case both happened to converge to the same wrong place.
    for p in &source.positions {
        let a = apply(&gpu_result.transform, *p);
        let b = apply(&known, *p);
        for i in 0..3 {
            assert!(
                (a[i] - b[i]).abs() < 2e-3,
                "gpu-registered point {a:?} vs known-transform point {b:?}"
            );
        }
    }
}

/// The same check with normals present on both clouds (the well-conditioned
/// two-term surface `point_to_plane.rs` uses, not the single-term one, which
/// `estimate_normals` handles fine but which `point_to_plane.rs`'s own tests
/// deliberately avoid for a *different* solve's conditioning reasons not relevant
/// here) -- exercising `correspond_and_reject`'s normal-angle branch and
/// `apply_transform`'s normal-rotation branch, which the no-normals test above never
/// runs. `min_normal_cos` stays at its default (inert), so this is still a Kabsch
/// comparison against `CpuIcp`, which never reads normals at all.
#[tokio::test]
#[ignore = "needs a real wgpu adapter; run with --features gpu-tests -- --ignored on a GPU host"]
async fn gpu_matches_cpu_reference_with_normals_present() {
    let known = Isometry3::from_parts(
        Translation3::new(0.03, -0.02, 0.015),
        UnitQuaternion::from_scaled_axis(Vector3::new(0.02, -0.015, 0.01)),
    );
    let source_positions = two_term_grid();
    let source_normals = estimate_normals(&source_positions, 6).expect("well-conditioned surface");
    let source = PointBuffer {
        normals: Some(source_normals),
        ..source_positions.clone()
    };

    let target_positions: Vec<[f32; 3]> = source_positions
        .positions
        .iter()
        .map(|p| apply(&known, *p))
        .collect();
    let target_unnormaled = PointBuffer {
        positions: target_positions,
        ..PointBuffer::default()
    };
    let target_normals = estimate_normals(&target_unnormaled, 6).expect("well-conditioned surface");
    let target = PointBuffer {
        normals: Some(target_normals),
        ..target_unnormaled
    };

    let (cpu_result, _) = run_cpu(
        PointBuffer {
            normals: None, // CpuIcp never reads normals; kept absent to make that explicit here.
            ..target.clone()
        },
        &PointBuffer {
            normals: None,
            ..source.clone()
        },
        100,
    );

    let (device, queue) = gpu_context().await;
    let mut gpu_engine =
        GpuFusionEngine::new(device, queue, &target, 100).expect("target is non-empty");
    let (gpu_result, _) = run_gpu(&mut gpu_engine, &source, 100);

    let translation_diff =
        (cpu_result.transform.translation.vector - gpu_result.transform.translation.vector).norm();
    let rotation_diff = angle_between(
        &cpu_result.transform.rotation,
        &gpu_result.transform.rotation,
    );
    let inlier_diff = (cpu_result.inlier_ratio - gpu_result.inlier_ratio).abs();

    assert!(
        translation_diff < 1e-3,
        "translation differs by {translation_diff} m"
    );
    assert!(
        rotation_diff < 1e-3,
        "rotation differs by {rotation_diff} rad"
    );
    assert!(inlier_diff < 0.01, "inlier ratio differs by {inlier_diff}");
}

/// Mirrors `cpu_reference::tests::an_aligned_cloud_converges_at_once_with_every_point_an_inlier`:
/// a cloud registered against itself should need no correction and report every
/// point an inlier, on the GPU path too.
#[tokio::test]
#[ignore = "needs a real wgpu adapter; run with --features gpu-tests -- --ignored on a GPU host"]
async fn gpu_an_aligned_cloud_converges_at_once_with_every_point_an_inlier() {
    let source = point_to_point_grid();
    let (device, queue) = gpu_context().await;
    let mut engine =
        GpuFusionEngine::new(device, queue, &point_to_point_grid(), 10).expect("non-empty target");
    let r = engine.step(&source).expect("steps");
    assert!(r.converged, "identical clouds are already aligned");
    assert!(
        (r.inlier_ratio - 1.0).abs() < 0.01,
        "inlier ratio {} should be ~1.0",
        r.inlier_ratio
    );
}

/// `fuse_voxels.wgsl` has no CPU oracle (this file's own header on why: fusing an
/// aligned cloud into a running map is not something `cpu_reference.rs` does at
/// all), so unlike every test above, this checks self-consistency rather than
/// agreement with a second implementation: two points landing in the same voxel
/// merge to their mean with their summed confidence, and fusing the identical pair a
/// second time leaves the mean unchanged (already exactly its own average) while
/// confidence accumulates. Calls `gpu::pipeline` directly rather than through
/// `GpuFusionEngine`, which does not expose fusion (§3.4 stage 7 is not wired into
/// the registration loop `PointCloudFusion::step` drives; nothing in this crate
/// calls it yet, `gungnir-app`'s new caller included).
#[tokio::test]
#[ignore = "needs a real wgpu adapter; run with --features gpu-tests -- --ignored on a GPU host"]
async fn gpu_voxel_fusion_merges_consistently() {
    use gungnir_data_fusion::gpu::buffers::{unflatten_f32, unflatten_positions, zeros};
    use gungnir_data_fusion::gpu::pipeline::read_buffer_sync;
    use gungnir_data_fusion::gpu::pipeline::{
        dispatch_fuse_voxels, storage_rw_usage, GpuPipelines, GridGeometry,
    };

    let (device, queue) = gpu_context().await;
    let pipelines = GpuPipelines::new(&device);

    // Two points 0.2 m apart, well inside a single 1 m voxel at the origin. Built
    // directly (not via `GridGeometry::from_bounds`, which pads the box with a
    // one-cell margin and would shift both points into voxel (1,1,1) rather than
    // (0,0,0)) so this test's own arithmetic for "which voxel" stays trivial to
    // check by hand.
    let points = vec![[0.1_f32, 0.1, 0.1], [0.3, 0.1, 0.1]];
    let geometry = GridGeometry {
        origin: [0.0, 0.0, 0.0],
        cell_size: 1.0,
        dim: [1, 1, 1],
    };
    let total_cells = geometry.total_cells();
    let mean_bytes_len = total_cells * 12;
    let conf_bytes_len = total_cells * 4;
    let usage = storage_rw_usage();

    let voxel_mean = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test-voxel-mean"),
        size: mean_bytes_len.max(12),
        usage,
        mapped_at_creation: false,
    });
    let voxel_confidence = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test-voxel-confidence"),
        size: conf_bytes_len.max(4),
        usage,
        mapped_at_creation: false,
    });
    queue.write_buffer(
        &voxel_mean,
        0,
        &zeros(mean_bytes_len.try_into().unwrap_or(12)),
    );
    queue.write_buffer(
        &voxel_confidence,
        0,
        &zeros(conf_bytes_len.try_into().unwrap_or(4)),
    );

    dispatch_fuse_voxels(
        &device,
        &queue,
        &pipelines,
        &points,
        geometry,
        32,
        1.0,
        &voxel_mean,
        &voxel_confidence,
    )
    .expect("fuses");

    let means = unflatten_positions(
        &read_buffer_sync(&device, &queue, &voxel_mean, mean_bytes_len).expect("readback"),
    );
    let confidences = unflatten_f32(
        &read_buffer_sync(&device, &queue, &voxel_confidence, conf_bytes_len).expect("readback"),
    );
    assert!(
        (means[0][0] - 0.2).abs() < 1e-5,
        "expected mean x 0.2, got {}",
        means[0][0]
    );
    assert!((means[0][1] - 0.1).abs() < 1e-5);
    assert!(
        (confidences[0] - 2.0).abs() < 1e-5,
        "expected confidence 2.0 after one fusion, got {}",
        confidences[0]
    );

    // Fuse the identical pair again: the mean is already its own average, so it
    // should not move; confidence should accumulate.
    dispatch_fuse_voxels(
        &device,
        &queue,
        &pipelines,
        &points,
        geometry,
        32,
        1.0,
        &voxel_mean,
        &voxel_confidence,
    )
    .expect("fuses again");
    let means = unflatten_positions(
        &read_buffer_sync(&device, &queue, &voxel_mean, mean_bytes_len).expect("readback"),
    );
    let confidences = unflatten_f32(
        &read_buffer_sync(&device, &queue, &voxel_confidence, conf_bytes_len).expect("readback"),
    );
    assert!(
        (means[0][0] - 0.2).abs() < 1e-5,
        "mean should not move: {}",
        means[0][0]
    );
    assert!(
        (confidences[0] - 4.0).abs() < 1e-5,
        "expected confidence 4.0 after a second fusion, got {}",
        confidences[0]
    );
}

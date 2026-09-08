// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

// Merge aligned source cloud into the running voxelized map, confidence-weighted
// (§3.4 stage 7).
//
// **No CPU oracle for this stage.** `cpu_reference.rs` registers two clouds; it does
// not fuse an aligned cloud into a running map, so there is nothing in this crate to
// differentially test this kernel against the way `correspondence.wgsl`/
// `reduction.wgsl` are tested against `CpuIcp` -- `verification-capability-table.md`
// §2 has no row for voxel fusion. What is checked (`gpu::validation`, and the
// `#[ignore]`d `gpu-tests` in `tests/gpu_vs_cpu.rs`) is that this kernel is valid
// WGSL and that its own arithmetic is self-consistent (a voxel merges into what it
// already held, weighted, rather than overwriting it) -- not that it agrees with a
// second, independent implementation, because none exists.
//
// Two passes, the same atomic-slot-claim pattern `spatial_hash.wgsl` uses for the
// same reason (no floating-point atomics; §3.4's own rationale for a uniform grid
// applies here too):
//   `claim_voxel_slots` -- each new point claims a slot in its voxel's fixed-capacity
//                           list via `atomicAdd`, dropping the excess past
//                           `max_new_per_voxel` (same graceful degradation as the ICP
//                           grid: a dropped point is a missed contribution, never a
//                           fabricated one).
//   `merge_voxels`      -- one invocation per voxel folds that voxel's newly-claimed
//                           points (plain, non-atomic arithmetic; only one invocation
//                           ever touches a given voxel's output) into the running
//                           confidence-weighted mean.

struct VoxelParams {
    origin_x: f32,
    origin_y: f32,
    origin_z: f32,
    voxel_size: f32,
    dim_x: u32,
    dim_y: u32,
    dim_z: u32,
    max_new_per_voxel: u32,
    num_points: u32,
    new_weight: f32,
    _pad0: u32,
    _pad1: u32,
}

@group(0) @binding(0) var<uniform> params: VoxelParams;
@group(0) @binding(1) var<storage, read> new_positions: array<f32>;
@group(0) @binding(2) var<storage, read_write> claim_counts: array<atomic<u32>>;
@group(0) @binding(3) var<storage, read_write> claimed_points: array<u32>;
@group(0) @binding(4) var<storage, read_write> voxel_mean: array<f32>;
@group(0) @binding(5) var<storage, read_write> voxel_confidence: array<f32>;

fn voxel_index(p: vec3<f32>) -> u32 {
    let origin = vec3<f32>(params.origin_x, params.origin_y, params.origin_z);
    let dim = vec3<i32>(i32(params.dim_x), i32(params.dim_y), i32(params.dim_z));
    var c = vec3<i32>(floor((p - origin) / params.voxel_size));
    c = clamp(c, vec3<i32>(0, 0, 0), dim - vec3<i32>(1, 1, 1));
    return u32(c.x) + u32(c.y) * params.dim_x + u32(c.z) * params.dim_x * params.dim_y;
}

@compute @workgroup_size(64)
fn claim_voxel_slots(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= params.num_points) {
        return;
    }
    let p = vec3<f32>(new_positions[3u * i], new_positions[3u * i + 1u], new_positions[3u * i + 2u]);
    let v = voxel_index(p);
    let slot = atomicAdd(&claim_counts[v], 1u);
    if (slot < params.max_new_per_voxel) {
        claimed_points[v * params.max_new_per_voxel + slot] = i;
    }
}

@compute @workgroup_size(64)
fn merge_voxels(@builtin(global_invocation_id) gid: vec3<u32>) {
    let v = gid.x;
    let total_voxels = params.dim_x * params.dim_y * params.dim_z;
    if (v >= total_voxels) {
        return;
    }
    // `atomicLoad`, not a second plain binding for the same buffer: WGSL does not
    // allow two module-scope resource variables at the same binding, and this kernel
    // only needs to *read* the count `claim_voxel_slots` wrote with `atomicAdd`.
    let claimed = min(atomicLoad(&claim_counts[v]), params.max_new_per_voxel);
    if (claimed == 0u) {
        return;
    }

    var sum = vec3<f32>(0.0, 0.0, 0.0);
    for (var k: u32 = 0u; k < claimed; k = k + 1u) {
        let pidx = claimed_points[v * params.max_new_per_voxel + k];
        sum = sum + vec3<f32>(
            new_positions[3u * pidx],
            new_positions[3u * pidx + 1u],
            new_positions[3u * pidx + 2u],
        );
    }
    let new_count = f32(claimed);
    let new_mean = sum / new_count;
    let new_conf = params.new_weight * new_count;

    let old_conf = voxel_confidence[v];
    let old_mean = vec3<f32>(voxel_mean[3u * v], voxel_mean[3u * v + 1u], voxel_mean[3u * v + 2u]);

    let total_conf = old_conf + new_conf;
    // A voxel merges into what it already held, weighted by confidence -- the
    // standard combination of two weighted means. `old_conf == 0` (a voxel fused
    // into for the first time) collapses this to `new_mean` exactly, since the
    // `old_mean * 0.0` term vanishes. `total_conf <= 0.0` only if `new_weight <= 0.0`
    // (since `claimed > 0` here already), a misconfigured caller rather than a normal
    // state; guarded rather than left to divide by zero into a silent `NaN`.
    var merged = new_mean;
    if (total_conf > 0.0) {
        merged = (old_mean * old_conf + new_mean * new_conf) / total_conf;
    }

    voxel_mean[3u * v] = merged.x;
    voxel_mean[3u * v + 1u] = merged.y;
    voxel_mean[3u * v + 2u] = merged.z;
    voxel_confidence[v] = total_conf;
}

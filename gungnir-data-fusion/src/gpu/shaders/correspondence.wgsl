// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

// Two passes over the moving (source) cloud, §3.4 stages 6, 2 and 3:
//   `apply_transform`        -- stage 6: move the source cloud by the current estimate.
//   `correspond_and_reject`  -- stages 2+3: nearest-neighbour search against the
//                                target's spatial hash (`spatial_hash.wgsl`), then
//                                reject by distance and, when both clouds carry
//                                normals, by normal angle.
// Split into two entry points rather than one fused kernel because the CPU
// orchestration (`gpu::pipeline`) calls `apply_transform` once per estimate and
// `correspond_and_reject` is the one later re-run (unchanged) for the inlier count
// with a tighter distance and the normal gate disabled -- see `IcpParams::inlier_distance`
// and `crate::cpu_reference::CpuIcp::inlier_ratio`, whose two-threshold shape this
// mirrors.

struct IcpParams {
    grid_origin_x: f32,
    grid_origin_y: f32,
    grid_origin_z: f32,
    cell_size: f32,
    grid_dim_x: u32,
    grid_dim_y: u32,
    grid_dim_z: u32,
    max_per_cell: u32,
    num_source_points: u32,
    num_target_points: u32,
    has_normals: u32,
    _pad0: u32,
    r00: f32,
    r01: f32,
    r02: f32,
    r10: f32,
    r11: f32,
    r12: f32,
    r20: f32,
    r21: f32,
    r22: f32,
    tx: f32,
    ty: f32,
    tz: f32,
    max_correspondence_dist: f32,
    min_normal_cos: f32,
    inlier_distance: f32,
    _pad1: f32,
}

const INVALID: u32 = 0xFFFFFFFFu;

@group(0) @binding(0) var<uniform> params: IcpParams;
@group(0) @binding(1) var<storage, read> source_positions: array<f32>;
@group(0) @binding(2) var<storage, read> source_normals: array<f32>;
@group(0) @binding(3) var<storage, read_write> transformed_positions: array<f32>;
@group(0) @binding(4) var<storage, read_write> transformed_normals: array<f32>;
@group(0) @binding(5) var<storage, read> target_positions: array<f32>;
@group(0) @binding(6) var<storage, read> target_normals: array<f32>;
@group(0) @binding(7) var<storage, read> cell_counts: array<u32>;
@group(0) @binding(8) var<storage, read> cell_points: array<u32>;
@group(0) @binding(9) var<storage, read_write> correspondence_target: array<u32>;

/// `R*p`, `R` the row-major rotation `IcpParams` carries (`gpu::params::IcpParams`'s
/// doc comment: `rotation[row][col]`, so `(R*p)[row] = dot(row, p)`, the standard
/// matrix-vector product read one row at a time).
fn rotate(p: vec3<f32>) -> vec3<f32> {
    let row0 = vec3<f32>(params.r00, params.r01, params.r02);
    let row1 = vec3<f32>(params.r10, params.r11, params.r12);
    let row2 = vec3<f32>(params.r20, params.r21, params.r22);
    return vec3<f32>(dot(row0, p), dot(row1, p), dot(row2, p));
}

@compute @workgroup_size(64)
fn apply_transform(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= params.num_source_points) {
        return;
    }
    let p = vec3<f32>(
        source_positions[3u * i],
        source_positions[3u * i + 1u],
        source_positions[3u * i + 2u],
    );
    let moved = rotate(p) + vec3<f32>(params.tx, params.ty, params.tz);
    transformed_positions[3u * i] = moved.x;
    transformed_positions[3u * i + 1u] = moved.y;
    transformed_positions[3u * i + 2u] = moved.z;
    if (params.has_normals != 0u) {
        // A normal turns by the rotation only -- no translation, and no inverse-
        // transpose correction, since a rigid transform has no scale (that
        // correction only matters for non-uniform scaling, which an `Isometry3`
        // cannot express).
        let n = vec3<f32>(
            source_normals[3u * i],
            source_normals[3u * i + 1u],
            source_normals[3u * i + 2u],
        );
        let rn = rotate(n);
        transformed_normals[3u * i] = rn.x;
        transformed_normals[3u * i + 1u] = rn.y;
        transformed_normals[3u * i + 2u] = rn.z;
    }
}

@compute @workgroup_size(64)
fn correspond_and_reject(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= params.num_source_points) {
        return;
    }
    let p = vec3<f32>(
        transformed_positions[3u * i],
        transformed_positions[3u * i + 1u],
        transformed_positions[3u * i + 2u],
    );
    let origin = vec3<f32>(params.grid_origin_x, params.grid_origin_y, params.grid_origin_z);
    let center = vec3<i32>(floor((p - origin) / params.cell_size));
    let dim = vec3<i32>(i32(params.grid_dim_x), i32(params.grid_dim_y), i32(params.grid_dim_z));

    var best_dist_sq: f32 = 3.4e38;
    var best_idx: u32 = INVALID;

    // The 3x3x3 block of cells around the query's own cell is sufficient to find
    // every target point within `max_correspondence_dist` of it, given
    // `cell_size >= max_correspondence_dist` -- see the correctness note atop
    // `spatial_hash.wgsl`. A neighbour cell fully outside the grid is skipped rather
    // than clamped: clamping here would re-scan a boundary cell's own contents once
    // per skipped neighbour, double-counting nothing incorrectly but wasting work;
    // skipping is simply the direct statement of "not part of the grid".
    for (var dz: i32 = -1; dz <= 1; dz = dz + 1) {
        for (var dy: i32 = -1; dy <= 1; dy = dy + 1) {
            for (var dx: i32 = -1; dx <= 1; dx = dx + 1) {
                let neighbor = vec3<i32>(center.x + dx, center.y + dy, center.z + dz);
                if (neighbor.x < 0 || neighbor.y < 0 || neighbor.z < 0
                    || neighbor.x >= dim.x || neighbor.y >= dim.y || neighbor.z >= dim.z) {
                    continue;
                }
                let idx = u32(neighbor.x)
                    + u32(neighbor.y) * params.grid_dim_x
                    + u32(neighbor.z) * params.grid_dim_x * params.grid_dim_y;
                let count = min(cell_counts[idx], params.max_per_cell);
                for (var k: u32 = 0u; k < count; k = k + 1u) {
                    let cand = cell_points[idx * params.max_per_cell + k];
                    let q = vec3<f32>(
                        target_positions[3u * cand],
                        target_positions[3u * cand + 1u],
                        target_positions[3u * cand + 2u],
                    );
                    let d = p - q;
                    let dist_sq = dot(d, d);
                    if (dist_sq < best_dist_sq) {
                        best_dist_sq = dist_sq;
                        best_idx = cand;
                    }
                }
            }
        }
    }

    var accepted = false;
    if (best_idx != INVALID) {
        let dist = sqrt(best_dist_sq);
        if (dist <= params.max_correspondence_dist) {
            accepted = true;
            if (params.has_normals != 0u) {
                let na = vec3<f32>(
                    transformed_normals[3u * i],
                    transformed_normals[3u * i + 1u],
                    transformed_normals[3u * i + 2u],
                );
                let nb = vec3<f32>(
                    target_normals[3u * best_idx],
                    target_normals[3u * best_idx + 1u],
                    target_normals[3u * best_idx + 2u],
                );
                // Both normals are unit vectors with an unresolved sign
                // (`crate::normals::estimate_normals`'s own documented contract: PCA
                // gives an axis, not a direction), so the angle test compares
                // magnitudes via `abs`, exactly as `point_to_plane.rs`'s test module
                // does when checking an estimated normal against a known one.
                let cos_angle = abs(dot(na, nb));
                if (cos_angle < params.min_normal_cos) {
                    accepted = false;
                }
            }
        }
    }

    correspondence_target[i] = select(INVALID, best_idx, accepted);
}

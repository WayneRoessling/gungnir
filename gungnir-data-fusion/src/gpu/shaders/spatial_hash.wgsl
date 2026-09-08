// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

// Uniform-grid spatial hash build for the target cloud (§3.4 stage 1).
// Bucket-sort points by cell; grids favored over a GPU BVH/kd-tree since
// LiDAR-class point density is fairly uniform (§3.4 rationale).
//
// Correctness invariant this file depends on and `correspondence.wgsl` relies on:
// the CPU orchestration (`gpu::pipeline`) sets `cell_size >= max_correspondence_dist`,
// so any target point within `max_correspondence_dist` of a query is guaranteed to lie
// in the query's own cell or one of its 26 neighbours -- the 3x3x3 search
// `correspond_and_reject` performs. A point within distance D of a query, with cell
// size >= D, cannot be more than one cell away along any axis: the query's own cell
// spans a width of `cell_size` >= D starting at the query's cell origin, so a point at
// most D away in x still falls within [query_cell_origin.x - cell_size,
// query_cell_origin.x + 2*cell_size), i.e. the immediately adjacent cell at most, never
// the one beyond it.
//
// Per-cell capacity is fixed (`max_per_cell`, `IcpParams`) rather than an exact
// counting-sort with a prefix sum: a cell that overflows drops the excess point from
// the index (this pass keeps writing correct counts via `atomicAdd`, but stores no more
// than `max_per_cell` indices). `correspond_and_reject` reads `min(count, max_per_cell)`
// for the same reason, so the two sides agree on what "in the grid" means. A dropped
// point can only ever cause a *missed* correspondence, never a wrong one: nothing here
// fabricates a neighbour that is not the point actually stored.

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

@group(0) @binding(0) var<uniform> params: IcpParams;
@group(0) @binding(1) var<storage, read> target_positions: array<f32>;
@group(0) @binding(2) var<storage, read_write> cell_counts: array<atomic<u32>>;
@group(0) @binding(3) var<storage, read_write> cell_points: array<u32>;

/// The cell coordinate `p` falls in, before clamping to the grid's own bounds.
fn cell_coord_of(p: vec3<f32>) -> vec3<i32> {
    let origin = vec3<f32>(params.grid_origin_x, params.grid_origin_y, params.grid_origin_z);
    return vec3<i32>(floor((p - origin) / params.cell_size));
}

/// Linear cell index, clamping into range. The CPU side sizes the grid to cover the
/// target's own bounding box plus a one-cell margin (`gpu::pipeline::GridGeometry`),
/// so this clamp is a defensive bound rather than the common case -- it exists so a
/// point exactly on the far edge of the box, which floating point can place a hair
/// outside it, still lands in a real cell instead of reading/writing out of bounds.
fn cell_index_clamped(coord: vec3<i32>) -> u32 {
    let dim = vec3<i32>(i32(params.grid_dim_x), i32(params.grid_dim_y), i32(params.grid_dim_z));
    let c = clamp(coord, vec3<i32>(0, 0, 0), dim - vec3<i32>(1, 1, 1));
    return u32(c.x) + u32(c.y) * params.grid_dim_x + u32(c.z) * params.grid_dim_x * params.grid_dim_y;
}

@compute @workgroup_size(64)
fn build_grid(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= params.num_target_points) {
        return;
    }
    let p = vec3<f32>(
        target_positions[3u * i],
        target_positions[3u * i + 1u],
        target_positions[3u * i + 2u],
    );
    let cell = cell_index_clamped(cell_coord_of(p));
    let slot = atomicAdd(&cell_counts[cell], 1u);
    if (slot < params.max_per_cell) {
        cell_points[cell * params.max_per_cell + slot] = i;
    }
    // else: this cell is already at capacity: correctness note above.
}

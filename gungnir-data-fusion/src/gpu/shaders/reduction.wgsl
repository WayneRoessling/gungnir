// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

// Parallel reduction: accumulate cross-covariance/residual terms for CPU-side
// transform estimation (§3.4 stage 4). One partial sum per workgroup, written to
// `partial_sums`; `gpu::pipeline` sums the (small number of) per-workgroup partials
// on the CPU and hands the total to `crate::transform_solve::solve_rigid_transform`
// -- the same Kabsch/SVD solve `cpu_reference.rs` uses, unmodified, which is what
// this reduction exists to feed rather than to replace (§3.4 stage 5: "cheap enough
// to read back to CPU and solve there").
//
// **Why raw moments, not centred sums.** Kabsch's cross-covariance is
// `H = Sum (s_i - cs)(t_i - ct)^T` with `cs`/`ct` the centroids -- but a centroid
// needs every point summed *before* it can be subtracted from any of them, which
// would take two passes (one to find the centroids, a second to accumulate H against
// them). Expanding the product algebraically:
//   Sum (s_i - cs)(t_i - ct)^T
//     = Sum s_i t_i^T - (Sum s_i) ct^T - cs (Sum t_i)^T + n cs ct^T
//     = Sum s_i t_i^T - n cs ct^T - n cs ct^T + n cs ct^T      [Sum s_i = n*cs, Sum t_i = n*ct]
//     = Sum s_i t_i^T - n cs ct^T
// so `H = Sum(s_i (x) t_i) - n cs ct^T` exactly, and every term on the right is a sum
// over independent points -- reducible in one pass. `gpu::pipeline` does the last
// step (dividing by `n`, forming `cs`/`ct`, subtracting) after reading the totals
// back; this kernel only ever sums.
//
// **No atomics on `f32`.** WGSL/wgpu do not guarantee a floating-point `atomicAdd`
// (it is not part of the core spec this workspace's `Features::empty()` requests),
// so summing across the whole dispatch cannot be one shared counter. Instead: each
// invocation's own contribution goes into workgroup-shared memory, a standard binary
// tree halves the live range every step (`workgroupBarrier` between steps orders the
// reads and writes), and thread 0 of each workgroup writes that workgroup's total to
// `partial_sums[workgroup_id]` -- small enough (one `f32` struct per workgroup, not
// per point) that `gpu::pipeline` sums the rest on the CPU with a plain loop.

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
const WORKGROUP_SIZE: u32 = 64u;
/// Fields written per workgroup into `partial_sums`: n, sum_s (3), sum_t (3),
/// sum_st (9, row-major s(x)t), sum_residual, n_inlier. `gpu::pipeline::ReducedSums`
/// reads them back in this exact order.
const FIELDS_PER_WORKGROUP: u32 = 18u;

@group(0) @binding(0) var<uniform> params: IcpParams;
@group(0) @binding(1) var<storage, read> transformed_positions: array<f32>;
@group(0) @binding(2) var<storage, read> target_positions: array<f32>;
@group(0) @binding(3) var<storage, read> correspondence_target: array<u32>;
@group(0) @binding(4) var<storage, read_write> partial_sums: array<f32>;

var<workgroup> sh_n: array<f32, 64>;
var<workgroup> sh_sx: array<f32, 64>;
var<workgroup> sh_sy: array<f32, 64>;
var<workgroup> sh_sz: array<f32, 64>;
var<workgroup> sh_tx: array<f32, 64>;
var<workgroup> sh_ty: array<f32, 64>;
var<workgroup> sh_tz: array<f32, 64>;
var<workgroup> sh_sxtx: array<f32, 64>;
var<workgroup> sh_sxty: array<f32, 64>;
var<workgroup> sh_sxtz: array<f32, 64>;
var<workgroup> sh_sytx: array<f32, 64>;
var<workgroup> sh_syty: array<f32, 64>;
var<workgroup> sh_sytz: array<f32, 64>;
var<workgroup> sh_sztx: array<f32, 64>;
var<workgroup> sh_szty: array<f32, 64>;
var<workgroup> sh_sztz: array<f32, 64>;
var<workgroup> sh_residual: array<f32, 64>;
var<workgroup> sh_inlier: array<f32, 64>;

@compute @workgroup_size(64)
fn reduce(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let i = gid.x;
    let li = lid.x;

    var n: f32 = 0.0;
    var sx: f32 = 0.0;
    var sy: f32 = 0.0;
    var sz: f32 = 0.0;
    var tx: f32 = 0.0;
    var ty: f32 = 0.0;
    var tz: f32 = 0.0;
    var sxtx: f32 = 0.0;
    var sxty: f32 = 0.0;
    var sxtz: f32 = 0.0;
    var sytx: f32 = 0.0;
    var syty: f32 = 0.0;
    var sytz: f32 = 0.0;
    var sztx: f32 = 0.0;
    var szty: f32 = 0.0;
    var sztz: f32 = 0.0;
    var residual: f32 = 0.0;
    var inlier: f32 = 0.0;

    if (i < params.num_source_points) {
        let ti = correspondence_target[i];
        if (ti != INVALID) {
            let s = vec3<f32>(
                transformed_positions[3u * i],
                transformed_positions[3u * i + 1u],
                transformed_positions[3u * i + 2u],
            );
            let t = vec3<f32>(
                target_positions[3u * ti],
                target_positions[3u * ti + 1u],
                target_positions[3u * ti + 2u],
            );
            let d = distance(s, t);
            n = 1.0;
            sx = s.x;
            sy = s.y;
            sz = s.z;
            tx = t.x;
            ty = t.y;
            tz = t.z;
            sxtx = s.x * t.x;
            sxty = s.x * t.y;
            sxtz = s.x * t.z;
            sytx = s.y * t.x;
            syty = s.y * t.y;
            sytz = s.y * t.z;
            sztx = s.z * t.x;
            szty = s.z * t.y;
            sztz = s.z * t.z;
            residual = d;
            // Same distance test as `cpu_reference::CpuIcp::inlier_ratio`, applied to
            // whichever transform this dispatch's `apply_transform` used -- see
            // `gpu::pipeline`'s doc comment on why that is the pre-update estimate
            // rather than the post-update one `CpuIcp` re-derives with a second pass.
            if (d <= params.inlier_distance) {
                inlier = 1.0;
            }
        }
    }

    sh_n[li] = n;
    sh_sx[li] = sx;
    sh_sy[li] = sy;
    sh_sz[li] = sz;
    sh_tx[li] = tx;
    sh_ty[li] = ty;
    sh_tz[li] = tz;
    sh_sxtx[li] = sxtx;
    sh_sxty[li] = sxty;
    sh_sxtz[li] = sxtz;
    sh_sytx[li] = sytx;
    sh_syty[li] = syty;
    sh_sytz[li] = sytz;
    sh_sztx[li] = sztx;
    sh_szty[li] = szty;
    sh_sztz[li] = sztz;
    sh_residual[li] = residual;
    sh_inlier[li] = inlier;
    workgroupBarrier();

    var stride: u32 = WORKGROUP_SIZE / 2u;
    loop {
        if (stride == 0u) {
            break;
        }
        if (li < stride) {
            sh_n[li] = sh_n[li] + sh_n[li + stride];
            sh_sx[li] = sh_sx[li] + sh_sx[li + stride];
            sh_sy[li] = sh_sy[li] + sh_sy[li + stride];
            sh_sz[li] = sh_sz[li] + sh_sz[li + stride];
            sh_tx[li] = sh_tx[li] + sh_tx[li + stride];
            sh_ty[li] = sh_ty[li] + sh_ty[li + stride];
            sh_tz[li] = sh_tz[li] + sh_tz[li + stride];
            sh_sxtx[li] = sh_sxtx[li] + sh_sxtx[li + stride];
            sh_sxty[li] = sh_sxty[li] + sh_sxty[li + stride];
            sh_sxtz[li] = sh_sxtz[li] + sh_sxtz[li + stride];
            sh_sytx[li] = sh_sytx[li] + sh_sytx[li + stride];
            sh_syty[li] = sh_syty[li] + sh_syty[li + stride];
            sh_sytz[li] = sh_sytz[li] + sh_sytz[li + stride];
            sh_sztx[li] = sh_sztx[li] + sh_sztx[li + stride];
            sh_szty[li] = sh_szty[li] + sh_szty[li + stride];
            sh_sztz[li] = sh_sztz[li] + sh_sztz[li + stride];
            sh_residual[li] = sh_residual[li] + sh_residual[li + stride];
            sh_inlier[li] = sh_inlier[li] + sh_inlier[li + stride];
        }
        workgroupBarrier();
        stride = stride / 2u;
    }

    if (li == 0u) {
        let base = wid.x * FIELDS_PER_WORKGROUP;
        partial_sums[base + 0u] = sh_n[0];
        partial_sums[base + 1u] = sh_sx[0];
        partial_sums[base + 2u] = sh_sy[0];
        partial_sums[base + 3u] = sh_sz[0];
        partial_sums[base + 4u] = sh_tx[0];
        partial_sums[base + 5u] = sh_ty[0];
        partial_sums[base + 6u] = sh_tz[0];
        partial_sums[base + 7u] = sh_sxtx[0];
        partial_sums[base + 8u] = sh_sxty[0];
        partial_sums[base + 9u] = sh_sxtz[0];
        partial_sums[base + 10u] = sh_sytx[0];
        partial_sums[base + 11u] = sh_syty[0];
        partial_sums[base + 12u] = sh_sytz[0];
        partial_sums[base + 13u] = sh_sztx[0];
        partial_sums[base + 14u] = sh_szty[0];
        partial_sums[base + 15u] = sh_sztz[0];
        partial_sums[base + 16u] = sh_residual[0];
        partial_sums[base + 17u] = sh_inlier[0];
    }
}

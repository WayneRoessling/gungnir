// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Uniform-buffer parameter structs shared with the WGSL kernels under `shaders/`.
//!
//! **No `bytemuck`.** The workspace stack (`ARCHITECTURE.md` §9) does not pin it, and
//! adding it would be a new `[workspace.dependencies]` entry for what a dozen
//! `to_le_bytes()` calls already do safely. Every field is packed by hand, in
//! declaration order, so the byte layout below is exactly the struct layout the WGSL
//! side reads -- see each struct's doc comment for the field-by-field byte offsets,
//! hand-verified against the WGSL `struct` it mirrors (`shaders/spatial_hash.wgsl`,
//! `shaders/correspondence.wgsl`, `shaders/reduction.wgsl` all share [`IcpParams`];
//! `shaders/fuse_voxels.wgsl` has its own [`VoxelParams`]).
//!
//! **Only `f32`/`u32` fields, deliberately.** WGSL's uniform address-space layout
//! rules bump a `vec3`/`vec4`/nested-`struct` member to 16-byte alignment (the
//! well-known "vec3 gotcha": `array<vec3<f32>>` has a 16-byte stride, not 12), which
//! is exactly the kind of mismatch that is invisible in Rust and only surfaces as
//! wrong numbers on a real GPU -- the one place this crate cannot check its own work
//! (`gpu-fusion.yml`). A struct of plain scalars has no such rule: every field is
//! 4-byte aligned and the layout is the field order, full stop. Padding fields below
//! exist only to round the struct's total size to a multiple of 16 bytes, which
//! `naga`'s validator (exercised offline in `gpu::validation`, no GPU needed) checks
//! independently of the rule above.

/// Shared by `build_grid` (`spatial_hash.wgsl`), `apply_transform` and
/// `correspond_and_reject` (`correspondence.wgsl`), and `reduce` (`reduction.wgsl`).
/// Not every kernel reads every field; each kernel documents which ones it uses.
///
/// WGSL mirror (`shaders/spatial_hash.wgsl`, byte offsets in comments):
/// ```wgsl
/// struct IcpParams {
///     grid_origin_x: f32, grid_origin_y: f32, grid_origin_z: f32, cell_size: f32,   // 0..16
///     grid_dim_x: u32, grid_dim_y: u32, grid_dim_z: u32, max_per_cell: u32,          // 16..32
///     num_source_points: u32, num_target_points: u32, has_normals: u32, _pad0: u32,  // 32..48
///     r00: f32, r01: f32, r02: f32, r10: f32,                                        // 48..64
///     r11: f32, r12: f32, r20: f32, r21: f32,                                        // 64..80
///     r22: f32, tx: f32, ty: f32, tz: f32,                                           // 80..96
///     max_correspondence_dist: f32, min_normal_cos: f32, inlier_distance: f32, _pad1: f32, // 96..112
/// }
/// ```
/// Total 112 bytes (28 four-byte fields), a multiple of 16.
#[derive(Debug, Clone, Copy)]
pub struct IcpParams {
    pub grid_origin: [f32; 3],
    pub cell_size: f32,
    pub grid_dim: [u32; 3],
    pub max_per_cell: u32,
    pub num_source_points: u32,
    pub num_target_points: u32,
    pub has_normals: bool,
    /// Row-major 3x3 rotation: `rotation[row][col]`, applied as `R*p` (row dotted with
    /// `p`) in `apply_transform` -- see that kernel's doc comment.
    pub rotation: [[f32; 3]; 3],
    pub translation: [f32; 3],
    /// A correspondence farther than this is not a correspondence at all (never enters
    /// the reduction). Distinct from `inlier_distance`, which only gates the reported
    /// inlier count -- see `crate::cpu_reference::CpuIcp::inlier_ratio`, which applies
    /// exactly that second, separate threshold to a correspondence search with no
    /// distance gate of its own.
    pub max_correspondence_dist: f32,
    /// `abs(dot(source_normal, target_normal))` below this rejects a correspondence
    /// that otherwise passed the distance gate. `-1.0` (the default,
    /// [`IcpParams::normal_gate_disabled`]) accepts every angle, which is what makes
    /// this kernel's default behaviour structurally identical to `CpuIcp`'s (no
    /// correspondence is ever rejected before the solve) rather than a different
    /// algorithm that happens to agree on easy inputs.
    pub min_normal_cos: f32,
    pub inlier_distance: f32,
}

impl IcpParams {
    /// `min_normal_cos` value that accepts every angle (`abs(cos) >= -1.0` always
    /// holds), i.e. the normal-angle gate is present in the kernel but inert.
    pub const NORMAL_GATE_DISABLED: f32 = -1.0;

    pub const SIZE_BYTES: usize = 112;

    #[must_use]
    pub fn to_bytes(&self) -> [u8; Self::SIZE_BYTES] {
        let mut out = [0u8; Self::SIZE_BYTES];
        let mut w = ByteWriter::new(&mut out);
        w.f32(self.grid_origin[0]);
        w.f32(self.grid_origin[1]);
        w.f32(self.grid_origin[2]);
        w.f32(self.cell_size);
        w.u32(self.grid_dim[0]);
        w.u32(self.grid_dim[1]);
        w.u32(self.grid_dim[2]);
        w.u32(self.max_per_cell);
        w.u32(self.num_source_points);
        w.u32(self.num_target_points);
        w.u32(u32::from(self.has_normals));
        w.u32(0); // _pad0
        w.f32(self.rotation[0][0]);
        w.f32(self.rotation[0][1]);
        w.f32(self.rotation[0][2]);
        w.f32(self.rotation[1][0]);
        w.f32(self.rotation[1][1]);
        w.f32(self.rotation[1][2]);
        w.f32(self.rotation[2][0]);
        w.f32(self.rotation[2][1]);
        w.f32(self.rotation[2][2]);
        w.f32(self.translation[0]);
        w.f32(self.translation[1]);
        w.f32(self.translation[2]);
        w.f32(self.max_correspondence_dist);
        w.f32(self.min_normal_cos);
        w.f32(self.inlier_distance);
        w.f32(0.0); // _pad1
        debug_assert_eq!(
            w.offset,
            Self::SIZE_BYTES,
            "IcpParams::to_bytes wrote the wrong length"
        );
        out
    }
}

/// `claim_voxel_slots` and `merge_voxels` (`shaders/fuse_voxels.wgsl`).
///
/// WGSL mirror:
/// ```wgsl
/// struct VoxelParams {
///     origin_x: f32, origin_y: f32, origin_z: f32, voxel_size: f32,          // 0..16
///     dim_x: u32, dim_y: u32, dim_z: u32, max_new_per_voxel: u32,             // 16..32
///     num_points: u32, new_weight: f32, _pad0: u32, _pad1: u32,              // 32..48
/// }
/// ```
/// Total 48 bytes (12 four-byte fields), a multiple of 16.
#[derive(Debug, Clone, Copy)]
pub struct VoxelParams {
    pub origin: [f32; 3],
    pub voxel_size: f32,
    pub dim: [u32; 3],
    pub max_new_per_voxel: u32,
    pub num_points: u32,
    /// The confidence one newly-fused point contributes, before the running-average
    /// weighting in `merge_voxels` folds it against what a voxel already holds.
    pub new_weight: f32,
}

impl VoxelParams {
    pub const SIZE_BYTES: usize = 48;

    #[must_use]
    pub fn to_bytes(&self) -> [u8; Self::SIZE_BYTES] {
        let mut out = [0u8; Self::SIZE_BYTES];
        let mut w = ByteWriter::new(&mut out);
        w.f32(self.origin[0]);
        w.f32(self.origin[1]);
        w.f32(self.origin[2]);
        w.f32(self.voxel_size);
        w.u32(self.dim[0]);
        w.u32(self.dim[1]);
        w.u32(self.dim[2]);
        w.u32(self.max_new_per_voxel);
        w.u32(self.num_points);
        w.f32(self.new_weight);
        w.u32(0); // _pad0
        w.u32(0); // _pad1
        debug_assert_eq!(
            w.offset,
            Self::SIZE_BYTES,
            "VoxelParams::to_bytes wrote the wrong length"
        );
        out
    }
}

/// A little cursor over a fixed-size byte array, so every `to_bytes` above is a flat
/// list of `w.f32(...)`/`w.u32(...)` calls in the same order as the WGSL struct it
/// mirrors, with no manual offset arithmetic to get wrong.
struct ByteWriter<'a> {
    buf: &'a mut [u8],
    offset: usize,
}

impl<'a> ByteWriter<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, offset: 0 }
    }

    fn write(&mut self, bytes: &[u8]) {
        let end = self.offset + bytes.len();
        // A fixed-size caller array too short for its own field list is a programming
        // error caught immediately by this slice write panicking, and by the
        // `debug_assert_eq!` on the final offset above; both fire long before any GPU
        // is involved.
        self.buf[self.offset..end].copy_from_slice(bytes);
        self.offset = end;
    }

    fn f32(&mut self, v: f32) {
        self.write(&v.to_le_bytes());
    }

    fn u32(&mut self, v: u32) {
        self.write(&v.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::float_cmp)]
    fn icp_params_round_trips_field_order() {
        let p = IcpParams {
            grid_origin: [1.0, 2.0, 3.0],
            cell_size: 4.0,
            grid_dim: [5, 6, 7],
            max_per_cell: 8,
            num_source_points: 9,
            num_target_points: 10,
            has_normals: true,
            rotation: [[11.0, 12.0, 13.0], [14.0, 15.0, 16.0], [17.0, 18.0, 19.0]],
            translation: [20.0, 21.0, 22.0],
            max_correspondence_dist: 23.0,
            min_normal_cos: 24.0,
            inlier_distance: 25.0,
        };
        let bytes = p.to_bytes();
        assert_eq!(bytes.len(), IcpParams::SIZE_BYTES);
        // Spot-check a handful of offsets by hand against the WGSL layout in the doc
        // comment above, rather than trusting the writer to check itself.
        assert_eq!(
            f32::from_le_bytes(bytes[0..4].try_into().unwrap_or([0; 4])),
            1.0
        );
        assert_eq!(
            u32::from_le_bytes(bytes[16..20].try_into().unwrap_or([0; 4])),
            5
        );
        assert_eq!(
            u32::from_le_bytes(bytes[40..44].try_into().unwrap_or([0; 4])),
            1
        ); // has_normals
        assert_eq!(
            f32::from_le_bytes(bytes[48..52].try_into().unwrap_or([0; 4])),
            11.0
        ); // r00
        assert_eq!(
            f32::from_le_bytes(bytes[84..88].try_into().unwrap_or([0; 4])),
            20.0
        ); // tx
        assert_eq!(
            f32::from_le_bytes(bytes[96..100].try_into().unwrap_or([0; 4])),
            23.0
        ); // max_correspondence_dist
        assert_eq!(bytes.len() % 16, 0);
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn voxel_params_round_trips_field_order() {
        let p = VoxelParams {
            origin: [1.0, 2.0, 3.0],
            voxel_size: 4.0,
            dim: [5, 6, 7],
            max_new_per_voxel: 8,
            num_points: 9,
            new_weight: 10.0,
        };
        let bytes = p.to_bytes();
        assert_eq!(bytes.len(), VoxelParams::SIZE_BYTES);
        assert_eq!(
            u32::from_le_bytes(bytes[32..36].try_into().unwrap_or([0; 4])),
            9
        );
        assert_eq!(
            f32::from_le_bytes(bytes[36..40].try_into().unwrap_or([0; 4])),
            10.0
        );
        assert_eq!(bytes.len() % 16, 0);
    }
}

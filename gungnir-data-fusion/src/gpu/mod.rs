// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! wgpu compute pipeline setup. Owns no `wgpu::Device` -- one is passed in from
//! `gungnir-render`, shared, never a second device (§3.3).
//!
//! - [`buffers`]: byte-packing and persistent-buffer helpers, GPU-independent.
//! - [`params`]: the `IcpParams`/`VoxelParams` uniform structs shared with the WGSL
//!   kernels below, and their hand-written byte layout.
//! - [`pipeline`]: shader modules, compute pipelines, bind groups, dispatch and
//!   readback -- the orchestration `crate::GpuFusionEngine` drives.
//! - [`validation`]: offline WGSL parse/validate via `naga` (no GPU needed).

pub mod buffers;
pub mod params;
pub mod pipeline;
pub mod validation;

pub const SPATIAL_HASH_SHADER: &str = include_str!("shaders/spatial_hash.wgsl");
pub const CORRESPONDENCE_SHADER: &str = include_str!("shaders/correspondence.wgsl");
pub const REDUCTION_SHADER: &str = include_str!("shaders/reduction.wgsl");
pub const FUSE_VOXELS_SHADER: &str = include_str!("shaders/fuse_voxels.wgsl");

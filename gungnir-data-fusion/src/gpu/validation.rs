// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Offline WGSL correctness checking, no GPU adapter required.
//!
//! This crate cannot exercise its own GPU code in this environment (`ARCHITECTURE.md`
//! §9, `gpu-fusion.yml`'s own header), but it does not follow that nothing about the
//! four `shaders/*.wgsl` files can be checked without one. `wgpu` re-exports the
//! `naga` shader-translation library it uses internally to turn WGSL source into the
//! form a real backend consumes (`wgpu::naga`, gated on `wgpu_core`, which the
//! workspace's default `dx12`/native features enable -- no new dependency: this is
//! the same crate `wgpu.workspace = true` already pulls in, reached through its own
//! public re-export). `wgpu::naga::front::wgsl::parse_str` and `wgpu::naga::valid::Validator` are
//! the same parser and validator `wgpu::Device::create_shader_module` runs before it
//! ever touches a driver, so a module that fails here would fail identically on real
//! hardware, and a module that passes here has had its syntax, types, control flow
//! and resource bindings checked -- everything `naga` catches independent of a
//! specific adapter's capabilities.
//!
//! **What this does not check.** `wgpu::naga::valid::Capabilities::all()` below is more
//! permissive than any specific `wgpu::Features` set: it does not confirm this crate's
//! shaders run within the `Features::empty()` descriptor `gungnir_render::GpuContext`
//! requests (`gpu::pipeline`'s doc comment covers that separately), and it cannot
//! catch a logic error that is valid WGSL and still computes the wrong number --
//! that is what the `#[ignore]`d `gpu-tests` in `tests/gpu_vs_cpu.rs` are for, and
//! they need real hardware this environment does not have.

use wgpu::naga::valid::{Capabilities, ValidationFlags, Validator};

/// Parses and fully validates one WGSL source string, as `wgpu` would before
/// creating a shader module from it.
///
/// # Errors
///
/// A human-readable message combining the parse or validation failure, when the
/// source is not valid WGSL naga accepts.
pub fn validate_wgsl(label: &str, source: &str) -> Result<(), String> {
    let module = wgpu::naga::front::wgsl::parse_str(source)
        .map_err(|e| format!("{label}: WGSL parse error: {e}"))?;
    let mut validator = Validator::new(ValidationFlags::all(), Capabilities::all());
    validator
        .validate(&module)
        .map_err(|e| format!("{label}: WGSL validation error: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::{
        CORRESPONDENCE_SHADER, FUSE_VOXELS_SHADER, REDUCTION_SHADER, SPATIAL_HASH_SHADER,
    };

    /// Every shipped kernel is valid WGSL that `naga` accepts -- syntax, types,
    /// control flow and resource bindings -- without needing a GPU. This is the one
    /// genuinely GPU-independent correctness check this crate can make of its own
    /// shader code, and it runs in plain `cargo test`, not behind `gpu-tests`.
    #[test]
    fn every_shader_is_valid_wgsl() {
        let shaders = [
            ("spatial_hash.wgsl", SPATIAL_HASH_SHADER),
            ("correspondence.wgsl", CORRESPONDENCE_SHADER),
            ("reduction.wgsl", REDUCTION_SHADER),
            ("fuse_voxels.wgsl", FUSE_VOXELS_SHADER),
        ];
        let mut problems = Vec::new();
        for (name, source) in shaders {
            if let Err(e) = validate_wgsl(name, source) {
                problems.push(e);
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    /// Each expected entry point actually exists with a `@compute` stage, catching a
    /// renamed or deleted function the parse/validate check alone would not: those
    /// only require the *module* to be valid WGSL, not that any particular function
    /// is in it.
    #[test]
    fn every_expected_entry_point_is_present() {
        let expectations: &[(&str, &str, &[&str])] = &[
            ("spatial_hash.wgsl", SPATIAL_HASH_SHADER, &["build_grid"]),
            (
                "correspondence.wgsl",
                CORRESPONDENCE_SHADER,
                &["apply_transform", "correspond_and_reject"],
            ),
            ("reduction.wgsl", REDUCTION_SHADER, &["reduce"]),
            (
                "fuse_voxels.wgsl",
                FUSE_VOXELS_SHADER,
                &["claim_voxel_slots", "merge_voxels"],
            ),
        ];
        let mut problems = Vec::new();
        for (name, source, entry_points) in expectations {
            let module = match wgpu::naga::front::wgsl::parse_str(source) {
                Ok(m) => m,
                Err(e) => {
                    problems.push(format!("{name}: failed to parse: {e}"));
                    continue;
                }
            };
            for wanted in *entry_points {
                let found = module
                    .entry_points
                    .iter()
                    .any(|ep| ep.name == *wanted && ep.stage == wgpu::naga::ShaderStage::Compute);
                if !found {
                    problems.push(format!("{name}: no @compute entry point named `{wanted}`"));
                }
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }
}

// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! `wgpu` orchestration: shader modules, compute pipelines, bind groups, dispatch
//! and readback for the four `shaders/*.wgsl` kernels. [`crate::GpuFusionEngine`] is
//! the public, GPU-independent-trait-facing surface (`crate::PointCloudFusion`); this
//! module is everything underneath it.
//!
//! **Auto pipeline layouts throughout** (`layout: None` on every
//! `ComputePipelineDescriptor`, then `pipeline.get_bind_group_layout(0)`). Two entry
//! points in one WGSL module (`apply_transform`/`correspond_and_reject` in
//! `correspondence.wgsl`; `claim_voxel_slots`/`merge_voxels` in `fuse_voxels.wgsl`)
//! reference different, overlapping subsets of that module's bindings, and `naga`'s
//! own per-entry-point reflection is the thing that knows exactly which subset each
//! one needs -- hand-writing a second `BindGroupLayoutDescriptor` that has to agree
//! with it by construction is exactly the kind of duplication this crate cannot
//! cross-check against a real device, so it is not duplicated at all: each pipeline's
//! layout comes from the same shader module it dispatches.
//!
//! **Everything smaller than the workgroup limit stays a flat `f32`/`u32` array**
//! (`gpu::buffers`'s doc comment). Every buffer holding more than one point's data
//! (positions, normals, `partial_sums`) is indexed `3*i + component` or
//! `FIELDS_PER_WORKGROUP*workgroup + field` by both the WGSL and this file, matching
//! by construction rather than by a shared layout type neither side can check the
//! other against.

use crate::gpu::buffers::SizedBuffer;
use crate::gpu::params::{IcpParams, VoxelParams};
use crate::gpu::{
    CORRESPONDENCE_SHADER, FUSE_VOXELS_SHADER, REDUCTION_SHADER, SPATIAL_HASH_SHADER,
};
use crate::FusionError;

/// Number of invocations per workgroup for every kernel in this crate.
///
/// 64 rather than a larger power of two so `reduction.wgsl`'s 18 parallel
/// `var<workgroup>` accumulator arrays (one per reduced field, §4's note on why no
/// `array<Contribution>` struct is used instead) stay well under the 16 KiB
/// `maxComputeWorkgroupStorageSize` the WebGPU spec guarantees as a baseline: 18
/// fields x 64 threads x 4 bytes = 4608 bytes. The same 256-thread choice a spec-
/// baseline device also supports would have used 18 x 256 x 4 = 18 432 bytes,
/// already over that floor on the most conservative device `wgpu::Limits::default()`
/// allows for.
pub const WORKGROUP_SIZE: u32 = 64;

/// Ceiling division by [`WORKGROUP_SIZE`], the dispatch size for `n` items.
#[allow(clippy::cast_possible_truncation)]
fn workgroup_count(n: usize) -> u32 {
    let n = n.min(usize::try_from(u32::MAX).unwrap_or(usize::MAX)) as u32;
    n.div_ceil(WORKGROUP_SIZE).max(1)
}

/// The uniform-grid spatial hash's extent, derived from the target cloud's bounding
/// box plus a one-cell margin.
///
/// `cell_size` is fixed to (at least) the correspondence search radius: see the
/// correctness note atop `shaders/spatial_hash.wgsl` for why that guarantees the
/// 3x3x3 neighbour search in `correspond_and_reject` finds every candidate within
/// that radius.
#[derive(Debug, Clone, Copy)]
pub struct GridGeometry {
    pub origin: [f32; 3],
    pub cell_size: f32,
    pub dim: [u32; 3],
}

/// A grid this large would allocate more `cell_points` memory than any test fixture
/// or a first real deployment should ever need; past this, `GridGeometry::from_bounds`
/// refuses rather than silently reaching for gigabytes of GPU memory on a caller's
/// mistake (a `cell_size` far smaller than the cloud's extent).
const MAX_GRID_CELLS: u64 = 4_000_000;

impl GridGeometry {
    /// # Errors
    ///
    /// [`FusionError::GpuInit`] when `cell_size` is not finite and positive, or when
    /// the resulting grid would exceed [`MAX_GRID_CELLS`].
    pub fn from_bounds(min: [f32; 3], max: [f32; 3], cell_size: f32) -> Result<Self, FusionError> {
        if !cell_size.is_finite() || cell_size <= 0.0 {
            return Err(FusionError::GpuInit(format!(
                "spatial hash cell size must be finite and positive, got {cell_size}"
            )));
        }
        let margin = cell_size;
        let origin = [min[0] - margin, min[1] - margin, min[2] - margin];
        let mut dim = [0u32; 3];
        for i in 0..3 {
            let extent = (max[i] - min[i]).max(0.0) + 2.0 * margin;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let cells = (extent / cell_size).ceil().max(1.0) as u32;
            dim[i] = cells;
        }
        let geometry = Self {
            origin,
            cell_size,
            dim,
        };
        if geometry.total_cells() > MAX_GRID_CELLS {
            return Err(FusionError::GpuInit(format!(
                "spatial hash grid would need {} cells (limit {MAX_GRID_CELLS}); \
                 increase the correspondence distance or shrink the target cloud",
                geometry.total_cells()
            )));
        }
        Ok(geometry)
    }

    #[must_use]
    pub fn total_cells(&self) -> u64 {
        u64::from(self.dim[0]) * u64::from(self.dim[1]) * u64::from(self.dim[2])
    }
}

/// The 18 fields `reduction.wgsl` writes per workgroup, already summed across every
/// workgroup in the dispatch. Field order and count are documented in that file
/// (`FIELDS_PER_WORKGROUP`) and must move in lockstep with it.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReducedSums {
    pub n: f32,
    pub sum_s: [f32; 3],
    pub sum_t: [f32; 3],
    /// Row-major: `sum_st[row][col] = Sum s_row * t_col`.
    pub sum_st: [[f32; 3]; 3],
    pub sum_residual: f32,
    pub n_inlier: f32,
}

impl ReducedSums {
    pub const FIELDS_PER_WORKGROUP: usize = 18;

    /// Sums every workgroup's partial contribution. `raw` is the whole `partial_sums`
    /// buffer read back: `raw.len()` need not be an exact multiple of
    /// [`Self::FIELDS_PER_WORKGROUP`] (a trailing partial chunk, which should not
    /// occur given how the buffer is sized, is simply not summed by `as_chunks`)
    /// rather than panicking on a length this function does not own.
    #[must_use]
    pub fn accumulate(raw: &[f32]) -> Self {
        let mut out = Self::default();
        for c in raw.as_chunks::<{ Self::FIELDS_PER_WORKGROUP }>().0 {
            out.n += c[0];
            out.sum_s[0] += c[1];
            out.sum_s[1] += c[2];
            out.sum_s[2] += c[3];
            out.sum_t[0] += c[4];
            out.sum_t[1] += c[5];
            out.sum_t[2] += c[6];
            out.sum_st[0][0] += c[7];
            out.sum_st[0][1] += c[8];
            out.sum_st[0][2] += c[9];
            out.sum_st[1][0] += c[10];
            out.sum_st[1][1] += c[11];
            out.sum_st[1][2] += c[12];
            out.sum_st[2][0] += c[13];
            out.sum_st[2][1] += c[14];
            out.sum_st[2][2] += c[15];
            out.sum_residual += c[16];
            out.n_inlier += c[17];
        }
        out
    }

    /// The centred cross-covariance `H = Sum (s_i - cs)(t_i - ct)^T`, from the raw
    /// moments this struct carries -- `H = Sum(s_i (x) t_i) - n*cs*ct^T`, the
    /// algebraic identity `reduction.wgsl`'s doc comment derives. `None` when
    /// `self.n == 0.0` (nothing to centre).
    #[must_use]
    pub fn cross_covariance(&self) -> Option<(nalgebra::Matrix3<f32>, [f32; 3], [f32; 3])> {
        if self.n <= 0.0 {
            return None;
        }
        let cs = [
            self.sum_s[0] / self.n,
            self.sum_s[1] / self.n,
            self.sum_s[2] / self.n,
        ];
        let ct = [
            self.sum_t[0] / self.n,
            self.sum_t[1] / self.n,
            self.sum_t[2] / self.n,
        ];
        #[rustfmt::skip]
        let h = nalgebra::Matrix3::new(
            self.sum_st[0][0] - self.n * cs[0] * ct[0],
            self.sum_st[0][1] - self.n * cs[0] * ct[1],
            self.sum_st[0][2] - self.n * cs[0] * ct[2],
            self.sum_st[1][0] - self.n * cs[1] * ct[0],
            self.sum_st[1][1] - self.n * cs[1] * ct[1],
            self.sum_st[1][2] - self.n * cs[1] * ct[2],
            self.sum_st[2][0] - self.n * cs[2] * ct[0],
            self.sum_st[2][1] - self.n * cs[2] * ct[1],
            self.sum_st[2][2] - self.n * cs[2] * ct[2],
        );
        Some((h, cs, ct))
    }
}

/// The six compute pipelines, built once from the four shader modules.
pub struct GpuPipelines {
    pub build_grid: wgpu::ComputePipeline,
    pub apply_transform: wgpu::ComputePipeline,
    pub correspond_and_reject: wgpu::ComputePipeline,
    pub reduce: wgpu::ComputePipeline,
    pub claim_voxel_slots: wgpu::ComputePipeline,
    pub merge_voxels: wgpu::ComputePipeline,
}

impl GpuPipelines {
    #[must_use]
    pub fn new(device: &wgpu::Device) -> Self {
        let spatial_hash = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gungnir-fusion-spatial-hash"),
            source: wgpu::ShaderSource::Wgsl(SPATIAL_HASH_SHADER.into()),
        });
        let correspondence = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gungnir-fusion-correspondence"),
            source: wgpu::ShaderSource::Wgsl(CORRESPONDENCE_SHADER.into()),
        });
        let reduction = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gungnir-fusion-reduction"),
            source: wgpu::ShaderSource::Wgsl(REDUCTION_SHADER.into()),
        });
        let fuse_voxels = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gungnir-fusion-fuse-voxels"),
            source: wgpu::ShaderSource::Wgsl(FUSE_VOXELS_SHADER.into()),
        });

        let pipeline = |module: &wgpu::ShaderModule, entry_point: &str, label: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: None,
                module,
                entry_point,
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            })
        };

        Self {
            build_grid: pipeline(&spatial_hash, "build_grid", "gungnir-fusion-build-grid"),
            apply_transform: pipeline(
                &correspondence,
                "apply_transform",
                "gungnir-fusion-apply-transform",
            ),
            correspond_and_reject: pipeline(
                &correspondence,
                "correspond_and_reject",
                "gungnir-fusion-correspond-and-reject",
            ),
            reduce: pipeline(&reduction, "reduce", "gungnir-fusion-reduce"),
            claim_voxel_slots: pipeline(
                &fuse_voxels,
                "claim_voxel_slots",
                "gungnir-fusion-claim-voxel-slots",
            ),
            merge_voxels: pipeline(&fuse_voxels, "merge_voxels", "gungnir-fusion-merge-voxels"),
        }
    }
}

/// Persistent GPU-side state for one target cloud's spatial hash, built once at
/// [`crate::GpuFusionEngine::new`] and never recreated for the life of the engine
/// (rust-ui-architecture-coding-standards.md §5) -- the target does not change
/// between `step` calls, only the source does.
pub struct TargetGrid {
    pub geometry: GridGeometry,
    pub target_positions: wgpu::Buffer,
    pub target_normals: wgpu::Buffer,
    pub cell_counts: wgpu::Buffer,
    pub cell_points: wgpu::Buffer,
    pub num_target_points: usize,
    pub has_target_normals: bool,
}

impl TargetGrid {
    /// Uploads `target`'s positions (and normals, when present) once, builds its
    /// spatial hash once, and never touches either again for the life of the engine
    /// -- the target cloud does not change between `step` calls, only the source
    /// does (`crate::cpu_reference::CpuIcp` has the same asymmetry: `target` is a
    /// constructor argument, `source` a per-`step` one).
    ///
    /// # Errors
    ///
    /// Propagates [`GridGeometry::from_bounds`]'s errors (a non-finite/non-positive
    /// `cell_size`, or a grid too large to allocate).
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipelines: &GpuPipelines,
        target: &crate::PointBuffer,
        cell_size: f32,
        max_per_cell: u32,
    ) -> Result<Self, FusionError> {
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for p in &target.positions {
            for i in 0..3 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
        let geometry = GridGeometry::from_bounds(min, max, cell_size)?;
        let num_target_points = target.positions.len();

        let positions_bytes = crate::gpu::buffers::flatten_positions(&target.positions);
        let target_positions = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gungnir-fusion-target-positions"),
            size: positions_bytes.len().max(12) as u64,
            usage: STORAGE_RW,
            mapped_at_creation: false,
        });
        queue.write_buffer(&target_positions, 0, &positions_bytes);

        let has_target_normals = target.normals.is_some();
        let normals_bytes = target.normals.as_ref().map_or_else(
            || crate::gpu::buffers::zeros(positions_bytes.len().max(12)),
            |n| crate::gpu::buffers::flatten_positions(n),
        );
        let target_normals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gungnir-fusion-target-normals"),
            size: normals_bytes.len().max(12) as u64,
            usage: STORAGE_RW,
            mapped_at_creation: false,
        });
        queue.write_buffer(&target_normals, 0, &normals_bytes);

        let total_cells = geometry.total_cells();
        let cell_counts = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gungnir-fusion-cell-counts"),
            size: (total_cells * 4).max(4),
            usage: STORAGE_RW,
            mapped_at_creation: false,
        });
        let cell_points = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gungnir-fusion-cell-points"),
            size: (total_cells * u64::from(max_per_cell) * 4).max(4),
            usage: STORAGE_RW,
            mapped_at_creation: false,
        });

        let grid = Self {
            geometry,
            target_positions,
            target_normals,
            cell_counts,
            cell_points,
            num_target_points,
            has_target_normals,
        };

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let params = IcpParams {
            grid_origin: geometry.origin,
            cell_size: geometry.cell_size,
            grid_dim: geometry.dim,
            max_per_cell,
            num_source_points: 0,
            num_target_points: num_target_points as u32,
            has_normals: has_target_normals,
            rotation: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [0.0, 0.0, 0.0],
            max_correspondence_dist: cell_size,
            min_normal_cos: IcpParams::NORMAL_GATE_DISABLED,
            inlier_distance: cell_size,
        };
        dispatch_build_grid(device, queue, pipelines, &grid, &params);
        Ok(grid)
    }
}

/// Per-`step`-call scratch buffers, sized to the current source cloud and resized
/// (never recreated wholesale) only when that count grows -- [`SizedBuffer`]'s job.
pub struct IcpScratch {
    pub source_positions: SizedBuffer,
    pub source_normals: SizedBuffer,
    pub transformed_positions: SizedBuffer,
    pub transformed_normals: SizedBuffer,
    pub correspondence_target: SizedBuffer,
    pub partial_sums: SizedBuffer,
}

impl IcpScratch {
    #[must_use]
    pub fn new() -> Self {
        Self {
            source_positions: SizedBuffer::new(12),
            source_normals: SizedBuffer::new(12),
            transformed_positions: SizedBuffer::new(12),
            transformed_normals: SizedBuffer::new(12),
            correspondence_target: SizedBuffer::new(4),
            partial_sums: SizedBuffer::new(4), // element = one f32; sized in `reduce_pass`
        }
    }
}

impl Default for IcpScratch {
    fn default() -> Self {
        Self::new()
    }
}

const STORAGE_RW: wgpu::BufferUsages = wgpu::BufferUsages::STORAGE
    .union(wgpu::BufferUsages::COPY_DST)
    .union(wgpu::BufferUsages::COPY_SRC);

/// Blocks until `source`'s contents (already the target of a `copy_buffer_to_buffer`
/// this function issues itself) are readable on the CPU, via a staging buffer --
/// `MAP_READ` cannot be combined with `STORAGE` usage in `wgpu`/WebGPU, so the
/// storage buffer a compute shader wrote is never mappable directly.
///
/// # Errors
///
/// [`FusionError::GpuInit`] if the map callback's channel is dropped (the device was
/// lost) or the map itself fails.
pub fn read_buffer_sync(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &wgpu::Buffer,
    len_bytes: u64,
) -> Result<Vec<u8>, FusionError> {
    if len_bytes == 0 {
        return Ok(Vec::new());
    }
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gungnir-fusion-readback"),
        size: len_bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("gungnir-fusion-readback-encoder"),
    });
    encoder.copy_buffer_to_buffer(source, 0, &staging, 0, len_bytes);
    queue.submit(std::iter::once(encoder.finish()));

    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    let _ = device.poll(wgpu::Maintain::Wait);
    let mapped = rx
        .recv()
        .map_err(|_| FusionError::GpuInit("GPU buffer read-back channel closed".into()))?;
    mapped.map_err(|e| FusionError::GpuInit(format!("GPU buffer mapping failed: {e}")))?;
    let bytes = slice.get_mapped_range().to_vec();
    staging.unmap();
    Ok(bytes)
}

/// One dispatch of `build_grid` over `target_positions`, after zeroing `cell_counts`
/// (an additive `atomicAdd` target, so it must start at zero every time the target
/// cloud -- and therefore the grid -- changes; this crate calls it exactly once, in
/// [`crate::GpuFusionEngine::new`]).
pub fn dispatch_build_grid(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipelines: &GpuPipelines,
    grid: &TargetGrid,
    params: &IcpParams,
) {
    let param_buf = upload_params(
        device,
        queue,
        &params.to_bytes(),
        "gungnir-fusion-params-grid",
    );
    queue.write_buffer(
        &grid.cell_counts,
        0,
        &crate::gpu::buffers::zeros(usize_from_u64(grid.cell_counts.size())),
    );

    let layout = pipelines.build_grid.get_bind_group_layout(0);
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("gungnir-fusion-build-grid-bind-group"),
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: param_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: grid.target_positions.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: grid.cell_counts.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: grid.cell_points.as_entire_binding(),
            },
        ],
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("gungnir-fusion-build-grid-encoder"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("gungnir-fusion-build-grid-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipelines.build_grid);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(workgroup_count(grid.num_target_points), 1, 1);
    }
    queue.submit(std::iter::once(encoder.finish()));
}

/// `apply_transform` then `correspond_and_reject`, in one command submission:
/// moves `source` by the transform `params` carries, then finds and gates each
/// moved point's nearest neighbour in `grid`. Returns nothing; the result lives in
/// `scratch.correspondence_target` and `scratch.transformed_{positions,normals}`
/// for [`dispatch_reduce`] to read.
///
/// One dispatch function per GPU pass rather than a builder that hides the argument
/// count is deliberate: every buffer and pipeline this call touches is visible at
/// the call site, which is what makes it possible to check by inspection that the
/// bind group entries a few lines down name exactly the bindings the corresponding
/// WGSL `struct`/`var` declarations expect (this crate's only line of defence against
/// a mismatch, absent a GPU to catch one at runtime).
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn dispatch_transform_and_correspond(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipelines: &GpuPipelines,
    grid: &TargetGrid,
    scratch: &IcpScratch,
    params: &IcpParams,
    num_source_points: usize,
) {
    let param_buf = upload_params(
        device,
        queue,
        &params.to_bytes(),
        "gungnir-fusion-params-correspond",
    );

    let Some(source_positions) = scratch.source_positions.buffer.as_ref() else {
        return;
    };
    let Some(source_normals) = scratch.source_normals.buffer.as_ref() else {
        return;
    };
    let Some(transformed_positions) = scratch.transformed_positions.buffer.as_ref() else {
        return;
    };
    let Some(transformed_normals) = scratch.transformed_normals.buffer.as_ref() else {
        return;
    };
    let Some(correspondence_target) = scratch.correspondence_target.buffer.as_ref() else {
        return;
    };

    let apply_layout = pipelines.apply_transform.get_bind_group_layout(0);
    let apply_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("gungnir-fusion-apply-transform-bind-group"),
        layout: &apply_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: param_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: source_positions.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: source_normals.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: transformed_positions.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: transformed_normals.as_entire_binding(),
            },
        ],
    });

    let correspond_layout = pipelines.correspond_and_reject.get_bind_group_layout(0);
    let correspond_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("gungnir-fusion-correspond-bind-group"),
        layout: &correspond_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: param_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: transformed_positions.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: transformed_normals.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: grid.target_positions.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: grid.target_normals.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: grid.cell_counts.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: grid.cell_points.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 9,
                resource: correspondence_target.as_entire_binding(),
            },
        ],
    });

    let workgroups = workgroup_count(num_source_points);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("gungnir-fusion-correspond-encoder"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("gungnir-fusion-apply-transform-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipelines.apply_transform);
        pass.set_bind_group(0, &apply_bind_group, &[]);
        pass.dispatch_workgroups(workgroups, 1, 1);
    }
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("gungnir-fusion-correspond-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipelines.correspond_and_reject);
        pass.set_bind_group(0, &correspond_bind_group, &[]);
        pass.dispatch_workgroups(workgroups, 1, 1);
    }
    queue.submit(std::iter::once(encoder.finish()));
}

/// Dispatches `reduce` and reads the summed [`ReducedSums`] back.
///
/// # Errors
///
/// Propagates [`read_buffer_sync`]'s errors.
pub fn dispatch_reduce(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipelines: &GpuPipelines,
    grid: &TargetGrid,
    scratch: &mut IcpScratch,
    params: &IcpParams,
    num_source_points: usize,
) -> Result<ReducedSums, FusionError> {
    let param_buf = upload_params(
        device,
        queue,
        &params.to_bytes(),
        "gungnir-fusion-params-reduce",
    );
    let workgroups = workgroup_count(num_source_points);
    let partial_bytes = u64::from(workgroups) * (ReducedSums::FIELDS_PER_WORKGROUP as u64) * 4;
    scratch.partial_sums.ensure_capacity(
        device,
        "gungnir-fusion-partial-sums",
        usize_from_u64(partial_bytes),
        STORAGE_RW,
    );

    let (Some(transformed_positions), Some(correspondence_target), Some(partial_sums)) = (
        scratch.transformed_positions.buffer.as_ref(),
        scratch.correspondence_target.buffer.as_ref(),
        scratch.partial_sums.buffer.as_ref(),
    ) else {
        return Ok(ReducedSums::default());
    };

    let layout = pipelines.reduce.get_bind_group_layout(0);
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("gungnir-fusion-reduce-bind-group"),
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: param_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: transformed_positions.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: grid.target_positions.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: correspondence_target.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: partial_sums.as_entire_binding(),
            },
        ],
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("gungnir-fusion-reduce-encoder"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("gungnir-fusion-reduce-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipelines.reduce);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(workgroups, 1, 1);
    }
    queue.submit(std::iter::once(encoder.finish()));

    let bytes = read_buffer_sync(device, queue, partial_sums, partial_bytes)?;
    let raw = crate::gpu::buffers::unflatten_f32(&bytes);
    Ok(ReducedSums::accumulate(&raw))
}

/// Uploads a fresh, small uniform buffer for one dispatch's parameters. A uniform
/// buffer this size (112 bytes, [`IcpParams::SIZE_BYTES`]) recreated a handful of
/// times per `step` call is not the per-frame *large* allocation
/// rust-ui-architecture-coding-standards.md §5 warns against (persistent point and
/// grid buffers, resized only on point-count change, are); a fresh tiny uniform
/// buffer per dispatch is the ordinary, cheap way to hand a compute pass its
/// arguments and avoids a `SizedBuffer` for 112 bytes.
fn upload_params(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    bytes: &[u8],
    label: &str,
) -> wgpu::Buffer {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: bytes.len() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buffer, 0, bytes);
    buffer
}

#[allow(clippy::cast_possible_truncation)]
fn usize_from_u64(v: u64) -> usize {
    v.min(usize::MAX as u64) as usize
}

/// Storage-buffer usage every persistent point/grid buffer in this crate needs:
/// written by `queue.write_buffer` or a compute shader, read by another compute
/// shader, and copyable out for [`read_buffer_sync`].
#[must_use]
pub fn storage_rw_usage() -> wgpu::BufferUsages {
    STORAGE_RW
}

/// Fuse an aligned cloud (`new_positions`, already transformed into the map's frame
/// by the caller) into the running voxel map, confidence-weighted.
///
/// # Errors
///
/// [`FusionError::GpuInit`] if `voxel_geometry`'s grid would exceed the same cell
/// budget [`GridGeometry::from_bounds`] enforces for the ICP grid.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn dispatch_fuse_voxels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipelines: &GpuPipelines,
    new_positions: &[[f32; 3]],
    voxel_geometry: GridGeometry,
    max_new_per_voxel: u32,
    new_weight: f32,
    voxel_mean: &wgpu::Buffer,
    voxel_confidence: &wgpu::Buffer,
) -> Result<(), FusionError> {
    if voxel_geometry.total_cells() > MAX_GRID_CELLS {
        return Err(FusionError::GpuInit(format!(
            "voxel fusion grid would need {} cells (limit {MAX_GRID_CELLS})",
            voxel_geometry.total_cells()
        )));
    }
    if new_positions.is_empty() {
        return Ok(());
    }
    #[allow(clippy::cast_possible_truncation)]
    let num_points = new_positions.len() as u32;
    let params = VoxelParams {
        origin: voxel_geometry.origin,
        voxel_size: voxel_geometry.cell_size,
        dim: voxel_geometry.dim,
        max_new_per_voxel,
        num_points,
        new_weight,
    };
    let param_buf = upload_params(
        device,
        queue,
        &params.to_bytes(),
        "gungnir-fusion-params-voxels",
    );

    let positions_bytes = crate::gpu::buffers::flatten_positions(new_positions);
    let positions_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gungnir-fusion-voxel-new-positions"),
        size: positions_bytes.len().max(12) as u64,
        usage: STORAGE_RW,
        mapped_at_creation: false,
    });
    queue.write_buffer(&positions_buf, 0, &positions_bytes);

    let total_voxels = voxel_geometry.total_cells();
    let claim_counts = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gungnir-fusion-voxel-claim-counts"),
        size: (total_voxels * 4).max(4),
        usage: STORAGE_RW,
        mapped_at_creation: false,
    });
    queue.write_buffer(
        &claim_counts,
        0,
        &crate::gpu::buffers::zeros(usize_from_u64(claim_counts.size())),
    );
    let claimed_points = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gungnir-fusion-voxel-claimed-points"),
        size: (total_voxels * u64::from(max_new_per_voxel) * 4).max(4),
        usage: STORAGE_RW,
        mapped_at_creation: false,
    });

    let claim_layout = pipelines.claim_voxel_slots.get_bind_group_layout(0);
    let claim_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("gungnir-fusion-claim-voxel-slots-bind-group"),
        layout: &claim_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: param_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: positions_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: claim_counts.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: claimed_points.as_entire_binding(),
            },
        ],
    });

    let merge_layout = pipelines.merge_voxels.get_bind_group_layout(0);
    let merge_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("gungnir-fusion-merge-voxels-bind-group"),
        layout: &merge_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: param_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: positions_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: claim_counts.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: claimed_points.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: voxel_mean.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: voxel_confidence.as_entire_binding(),
            },
        ],
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("gungnir-fusion-voxel-encoder"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("gungnir-fusion-claim-voxel-slots-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipelines.claim_voxel_slots);
        pass.set_bind_group(0, &claim_bind_group, &[]);
        pass.dispatch_workgroups(workgroup_count(new_positions.len()), 1, 1);
    }
    {
        #[allow(clippy::cast_possible_truncation)]
        let voxel_workgroups = workgroup_count(usize_from_u64(total_voxels));
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("gungnir-fusion-merge-voxels-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipelines.merge_voxels);
        pass.set_bind_group(0, &merge_bind_group, &[]);
        pass.dispatch_workgroups(voxel_workgroups, 1, 1);
    }
    queue.submit(std::iter::once(encoder.finish()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workgroup_count_covers_every_element() {
        assert_eq!(workgroup_count(0), 1);
        assert_eq!(workgroup_count(1), 1);
        assert_eq!(workgroup_count(64), 1);
        assert_eq!(workgroup_count(65), 2);
        assert_eq!(workgroup_count(128), 2);
    }

    #[test]
    fn grid_geometry_covers_the_bounding_box_with_margin() {
        let g = GridGeometry::from_bounds([0.0, 0.0, 0.0], [2.5, 2.5, 0.4], 0.5)
            .expect("reasonable bounds");
        // Extent 2.5 + 2*0.5 margin = 3.5, / 0.5 = 7 cells; a degenerate axis (z here,
        // extent 0.4) still gets at least one full cell of margin on each side.
        assert_eq!(g.dim[0], 7);
        assert_eq!(g.dim[1], 7);
        assert!(g.dim[2] >= 1);
        assert!((g.origin[0] - (-0.5)).abs() < 1e-6);
    }

    #[test]
    fn grid_geometry_refuses_a_non_positive_cell_size() {
        assert!(GridGeometry::from_bounds([0.0; 3], [1.0; 3], 0.0).is_err());
        assert!(GridGeometry::from_bounds([0.0; 3], [1.0; 3], -1.0).is_err());
    }

    #[test]
    fn grid_geometry_refuses_an_unreasonably_fine_cell_size() {
        // A 1000x1000x1000 m cloud with a 1 mm cell would need ~10^18 cells.
        let err = GridGeometry::from_bounds([0.0; 3], [1000.0; 3], 0.001).expect_err("too fine");
        assert!(matches!(err, FusionError::GpuInit(_)));
    }

    /// The algebraic identity `reduction.wgsl`'s doc comment derives, checked here in
    /// plain Rust against a small hand-built point set with a known centred
    /// cross-covariance, independent of any GPU: `H = Sum(s (x) t) - n*cs*ct^T` must
    /// equal `Sum (s_i - cs)(t_i - ct)^T` computed the direct (centred) way.
    ///
    /// Explicit `[i]`/`[j]` indexing throughout, deliberately, rather than the
    /// iterator-chain form `clippy::needless_range_loop` would otherwise suggest:
    /// the entire point of this test is a second, by-hand, line-by-line computation
    /// to check against -- the same reason `transform_solve.rs`'s own tests build
    /// their expected values by direct arithmetic rather than a clever reduction.
    #[test]
    #[allow(clippy::needless_range_loop, clippy::float_cmp)]
    fn cross_covariance_matches_the_direct_centred_computation() {
        let s = [
            [1.0_f32, 0.0, 0.0],
            [0.0, 2.0, 0.0],
            [1.0, 1.0, 1.0],
            [-1.0, 0.5, 2.0],
        ];
        let t = [
            [1.1, 0.1, -0.2],
            [0.2, 2.3, 0.1],
            [1.3, 0.9, 1.1],
            [-0.8, 0.4, 1.7],
        ];

        #[allow(clippy::cast_precision_loss)]
        let mut sums = ReducedSums {
            n: s.len() as f32,
            ..ReducedSums::default()
        };
        for (a, b) in s.iter().zip(&t) {
            for i in 0..3 {
                sums.sum_s[i] += a[i];
                sums.sum_t[i] += b[i];
            }
            for i in 0..3 {
                for j in 0..3 {
                    sums.sum_st[i][j] += a[i] * b[j];
                }
            }
        }
        let (h, source_centroid, target_centroid) = sums.cross_covariance().expect("n > 0");

        #[allow(clippy::cast_precision_loss)]
        let direct_source_centroid = {
            let mut c = [0.0f32; 3];
            for p in &s {
                for i in 0..3 {
                    c[i] += p[i];
                }
            }
            for v in &mut c {
                *v /= s.len() as f32;
            }
            c
        };
        #[allow(clippy::cast_precision_loss)]
        let direct_target_centroid = {
            let mut c = [0.0f32; 3];
            for p in &t {
                for i in 0..3 {
                    c[i] += p[i];
                }
            }
            for v in &mut c {
                *v /= t.len() as f32;
            }
            c
        };
        for i in 0..3 {
            assert!((source_centroid[i] - direct_source_centroid[i]).abs() < 1e-6);
            assert!((target_centroid[i] - direct_target_centroid[i]).abs() < 1e-6);
        }

        let mut h_direct = nalgebra::Matrix3::<f32>::zeros();
        for (a, b) in s.iter().zip(&t) {
            let da = nalgebra::Vector3::new(
                a[0] - direct_source_centroid[0],
                a[1] - direct_source_centroid[1],
                a[2] - direct_source_centroid[2],
            );
            let db = nalgebra::Vector3::new(
                b[0] - direct_target_centroid[0],
                b[1] - direct_target_centroid[1],
                b[2] - direct_target_centroid[2],
            );
            h_direct += da * db.transpose();
        }
        for i in 0..3 {
            for j in 0..3 {
                assert!(
                    (h[(i, j)] - h_direct[(i, j)]).abs() < 1e-5,
                    "H[{i}][{j}]: raw-moment {} vs direct {}",
                    h[(i, j)],
                    h_direct[(i, j)]
                );
            }
        }
    }
}

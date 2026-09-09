// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Persistent GPU buffer management -- created once, resized only on point-count
//! change, never recreated per frame (rust-ui-architecture-coding-standards.md §5).
//!
//! Positions and normals travel as flat `f32` triples (`x0,y0,z0,x1,y1,z1,...`)
//! rather than `array<vec3<f32>>`: WGSL's storage-layout rules give `vec3<f32>` a
//! 16-byte array stride (4 bytes of padding after every 12-byte element -- the
//! well-known "vec3 gotcha"), which this crate cannot cross-check against a real
//! GPU. A flat `array<f32>`, indexed `3*i, 3*i+1, 3*i+2` on both sides (every WGSL
//! kernel under `shaders/` does exactly this), has no such ambiguity: every element
//! is 4 bytes, full stop.

/// A point buffer's positions, flattened to bytes for `queue.write_buffer`.
#[must_use]
pub fn flatten_positions(points: &[[f32; 3]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(points.len() * 12);
    for p in points {
        out.extend_from_slice(&p[0].to_le_bytes());
        out.extend_from_slice(&p[1].to_le_bytes());
        out.extend_from_slice(&p[2].to_le_bytes());
    }
    out
}

/// The inverse of [`flatten_positions`], for reading a GPU buffer back.
///
/// Trailing bytes that do not complete a whole `[f32; 3]` are ignored rather than
/// erroring: every caller in this crate sizes buffers in exact multiples of 12 bytes,
/// so this only ever discards nothing in practice, and a helper reused for readback
/// has no natural error type to report a length mismatch through.
#[must_use]
pub fn unflatten_positions(bytes: &[u8]) -> Vec<[f32; 3]> {
    bytes
        .as_chunks::<12>()
        .0
        .iter()
        .map(|c| {
            [
                f32::from_le_bytes(c[0..4].try_into().unwrap_or([0; 4])),
                f32::from_le_bytes(c[4..8].try_into().unwrap_or([0; 4])),
                f32::from_le_bytes(c[8..12].try_into().unwrap_or([0; 4])),
            ]
        })
        .collect()
}

/// A flat `f32` buffer (not point triples -- used for `partial_sums`).
#[must_use]
pub fn unflatten_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

/// A flat `u32` buffer (used for `correspondence_target`).
#[must_use]
pub fn unflatten_u32(bytes: &[u8]) -> Vec<u32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect()
}

/// Zero-filled bytes for clearing an atomic counter buffer before a pass that
/// accumulates into it (`cell_counts`, `claim_counts`): `queue.write_buffer` with
/// this is the reset every `build_grid`/`claim_voxel_slots` dispatch needs, since
/// both are additive (`atomicAdd`) rather than assigning.
#[must_use]
pub fn zeros(len_bytes: usize) -> Vec<u8> {
    vec![0u8; len_bytes]
}

/// A GPU-side buffer paired with the point/element count it currently holds, resized
/// (not recreated) only when that count changes -- the persistent-buffer rule this
/// module exists for.
pub struct SizedBuffer {
    pub buffer: Option<wgpu::Buffer>,
    pub capacity_elements: usize,
    pub bytes_per_element: usize,
}

impl SizedBuffer {
    #[must_use]
    pub fn new(bytes_per_element: usize) -> Self {
        Self {
            buffer: None,
            capacity_elements: 0,
            bytes_per_element,
        }
    }

    /// Ensures the buffer can hold at least `elements` items with `usage`,
    /// reallocating only when the current one is absent or too small. Returns the
    /// buffer's current byte length (which may exceed `elements * bytes_per_element`
    /// if a larger point count was seen earlier and never shrunk back down -- shrinking
    /// on every reduction would defeat the point of not recreating every frame).
    pub fn ensure_capacity(
        &mut self,
        device: &wgpu::Device,
        label: &str,
        elements: usize,
        usage: wgpu::BufferUsages,
    ) -> u64 {
        if self.buffer.is_none() || elements > self.capacity_elements {
            let size = (elements.max(1) * self.bytes_per_element) as u64;
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            }));
            self.capacity_elements = elements.max(1);
        }
        self.buffer.as_ref().map_or(0, wgpu::Buffer::size)
    }
}

/// Legacy name kept for the module doc's own description in `gpu::mod`; superseded
/// in practice by [`SizedBuffer`], which tracks the byte stride alongside the
/// capacity so a caller cannot resize using the wrong element size.
pub struct PersistentPointBuffer {
    pub buffer: Option<wgpu::Buffer>,
    pub capacity_points: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_round_trip_through_flatten_and_unflatten() {
        let points = vec![[1.0, 2.0, 3.0], [-4.5, 0.0, 6.25]];
        let bytes = flatten_positions(&points);
        assert_eq!(bytes.len(), points.len() * 12);
        let back = unflatten_positions(&bytes);
        assert_eq!(back, points);
    }

    #[test]
    fn f32_and_u32_flat_buffers_round_trip() {
        let values = [1.0_f32, -2.5, 3.0, 0.0];
        let mut bytes = Vec::new();
        for v in values {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        assert_eq!(unflatten_f32(&bytes), values);

        let ints = [0u32, 1, u32::MAX, 42];
        let mut bytes = Vec::new();
        for v in ints {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        assert_eq!(unflatten_u32(&bytes), ints);
    }

    #[test]
    fn zeros_is_the_requested_length_and_all_zero() {
        let z = zeros(37);
        assert_eq!(z.len(), 37);
        assert!(z.iter().all(|&b| b == 0));
    }
}

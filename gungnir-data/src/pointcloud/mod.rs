// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Point cloud / LAS / LAZ / COPC I/O -- adopts `las` (GAP-023, 2026-09-06), and (once
//! pinned) `copc-rs`, `pasture-core`, `pasture-io` per the ecosystem decision table.

use std::path::Path;

use crate::DataError;

/// Project-internal point buffer, decoupled from `pasture`'s type so downstream
/// crates never take a direct dependency on `pasture`.
///
/// Positions are **relative to `origin`**, which is the file's minimum bound: a UTM
/// coordinate in the hundreds of thousands loses centimetres in an `f32`, and a
/// registration solved on such positions would be solved on noise. The origin carries
/// the large part in `f64`.
#[derive(Debug, Clone, Default)]
pub struct PointBuffer {
    pub positions: Vec<[f32; 3]>,
    pub intensity: Option<Vec<f32>>,
    pub classification: Option<Vec<u8>>,
    /// What `positions` are relative to, in the file's own frame.
    pub origin: [f64; 3],
}

impl PointBuffer {
    /// `[min, max]` of the positions in the file's frame (origin added back), or `None`
    /// for an empty buffer: an empty cloud has no bounds rather than zero ones.
    #[must_use]
    pub fn bounds(&self) -> Option<[[f64; 3]; 2]> {
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for p in &self.positions {
            for i in 0..3 {
                let v = f64::from(p[i]) + self.origin[i];
                min[i] = min[i].min(v);
                max[i] = max[i].max(v);
            }
        }
        (!self.positions.is_empty()).then_some([min, max])
    }
}

/// Load a LAS or LAZ file (the `las` crate handles both; LAZ through its `laz` feature).
///
/// # Errors
///
/// `DataError::Io` when the file cannot be opened; `DataError::Parse` when it is not a
/// LAS file, its header and its records disagree, or a record is truncated. Never a
/// panic, whatever the bytes.
pub fn load_las(path: &Path) -> Result<PointBuffer, DataError> {
    if !path.is_file() {
        return Err(DataError::Io(format!("{}: no such file", path.display())));
    }
    let parse = |e: las::Error| DataError::Parse(format!("{}: {e}", path.display()));
    let mut reader = las::Reader::from_path(path).map_err(parse)?;
    let header = reader.header().clone();
    let bounds = header.bounds();
    let origin = [bounds.min.x, bounds.min.y, bounds.min.z];
    let promised = header.number_of_points();
    let mut positions = Vec::new();
    let mut intensity = Vec::new();
    let mut classification = Vec::new();
    for point in reader.points() {
        let point = point.map_err(parse)?;
        positions.push([
            relative(point.x, origin[0]),
            relative(point.y, origin[1]),
            relative(point.z, origin[2]),
        ]);
        intensity.push(f32::from(point.intensity));
        classification.push(u8::from(point.classification));
    }
    if u64::try_from(positions.len()).unwrap_or(u64::MAX) != promised {
        return Err(DataError::Parse(format!(
            "{}: the header promises {promised} points and the file holds {}",
            path.display(),
            positions.len()
        )));
    }
    Ok(PointBuffer {
        positions,
        intensity: Some(intensity),
        classification: Some(classification),
        origin,
    })
}

/// A coordinate less its origin: metres from the cloud's corner, which `f32` carries to
/// well under a millimetre for any cloud the viewport can hold.
#[allow(clippy::cast_possible_truncation)]
fn relative(v: f64, origin: f64) -> f32 {
    (v - origin) as f32
}

/// # Errors
///
/// Always: bounded COPC reading is designed and not written (GAP-082).
pub fn load_copc_bounded(_path: &Path, _bounds: [f32; 6]) -> Result<PointBuffer, DataError> {
    Err(DataError::NotImplemented {
        what: "bounded COPC point-cloud loading",
        waiting_on: "copc-rs, which agentic-coding-standards.md §2.9 lists as unpinned",
    })
}

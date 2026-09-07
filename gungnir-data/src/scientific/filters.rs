// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

use super::MeshData;
use crate::DataError;

/// # Errors
///
/// Always: the threshold filter is designed and not written (GAP-082).
///
/// **Returns a `Result` rather than an empty mesh**, which a viewport would have drawn as
/// a region containing nothing.
pub fn threshold(_mesh: &MeshData, _min: f32, _max: f32) -> Result<MeshData, DataError> {
    Err(DataError::NotImplemented {
        what: "the threshold filter",
        waiting_on: "nothing but the work; the filters are project-owned",
    })
}

/// # Errors
///
/// Always: the clip filter is designed and not written (GAP-082). Same reason as
/// [`threshold`] for returning a `Result`.
pub fn clip_by_plane(
    _mesh: &MeshData,
    _normal: [f32; 3],
    _offset: f32,
) -> Result<MeshData, DataError> {
    Err(DataError::NotImplemented {
        what: "the clip-by-plane filter",
        waiting_on: "nothing but the work; the filters are project-owned",
    })
}

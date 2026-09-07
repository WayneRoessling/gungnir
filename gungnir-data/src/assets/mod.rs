// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! glTF asset loading -- adopts `gltf`.
//!
//! **What loads (2026-09-06, GAP-023)**: `.gltf` with external or data-URI buffers and
//! `.glb`, every mesh primitive with positions concatenated into one triangle list;
//! normals when the primitive carries them, else computed flat per face; indices when
//! present, else sequential. Node transforms are **not** applied: a scene of placed
//! instances is a viewport concern, and an asset is loaded once and placed by the
//! caller.

use std::path::Path;

use crate::DataError;

#[derive(Debug, Clone, Default)]
pub struct StaticMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

impl StaticMesh {
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

fn parse(reason: impl std::fmt::Display) -> DataError {
    DataError::Parse(reason.to_string())
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn normalise(v: [f32; 3]) -> [f32; 3] {
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if n > 0.0 {
        [v[0] / n, v[1] / n, v[2] / n]
    } else {
        [0.0, 0.0, 1.0]
    }
}

/// Flat normals: each vertex takes the normal of the last face that uses it, which is
/// exact for unshared vertices and a fair reading for the rest.
fn flat_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut n = vec![[0.0f32, 0.0, 1.0]; positions.len()];
    let (triangles, _) = indices.as_chunks::<3>();
    for tri in triangles {
        let (a, b, c) = (
            positions[tri[0] as usize],
            positions[tri[1] as usize],
            positions[tri[2] as usize],
        );
        let face = normalise(cross(sub(b, a), sub(c, a)));
        for &i in tri {
            n[i as usize] = face;
        }
    }
    n
}

/// Load every triangle primitive of a glTF asset.
///
/// # Errors
///
/// `DataError::Io` when the file or a buffer it names cannot be read; `DataError::Parse`
/// when it is not glTF, holds no triangles, or an index names a vertex it does not have.
pub fn load_gltf(path: &Path) -> Result<StaticMesh, DataError> {
    // `gltf-json`'s validator indexes the accessor table before checking the index is in
    // range, so a file that names an accessor it does not have panics inside the
    // dependency. The verification row asks for a `DataError` and never a panic, and the
    // loader thread must survive a bad file; the unwind is caught here and named.
    let imported = std::panic::catch_unwind(|| gltf::import(path))
        .map_err(|_| parse("the glTF reader panicked on this file: it is malformed"))?;
    let (document, buffers, _images) = imported.map_err(|e| match e {
        gltf::Error::Io(io) => DataError::Io(io.to_string()),
        other => parse(other),
    })?;
    let mut mesh = StaticMesh::default();
    for gltf_mesh in document.meshes() {
        for primitive in gltf_mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let reader = primitive.reader(|buffer| buffers.get(buffer.index()).map(|b| &b.0[..]));
            let Some(positions) = reader.read_positions() else {
                continue;
            };
            let base = mesh.positions.len();
            let positions: Vec<[f32; 3]> = positions.collect();
            let count = positions.len();
            let indices: Vec<u32> = match reader.read_indices() {
                Some(read) => read.into_u32().collect(),
                None => {
                    (0..u32::try_from(count).map_err(|_| parse("too many vertices"))?).collect()
                }
            };
            if !indices.len().is_multiple_of(3) {
                return Err(parse(
                    "a triangle primitive's index count is not a multiple of three",
                ));
            }
            for &i in &indices {
                if (i as usize) >= count {
                    return Err(parse(format!("index {i} names a vertex of {count}")));
                }
            }
            let normals: Vec<[f32; 3]> = if let Some(read) = reader.read_normals() {
                read.collect()
            } else {
                flat_normals(&positions, &indices)
            };
            if normals.len() != count {
                return Err(parse(format!(
                    "{} normals for {count} positions",
                    normals.len()
                )));
            }
            let base = u32::try_from(base).map_err(|_| parse("too many vertices"))?;
            mesh.positions.extend(positions);
            mesh.normals.extend(normals);
            mesh.indices.extend(indices.into_iter().map(|i| i + base));
        }
    }
    if mesh.indices.is_empty() {
        return Err(parse("the asset holds no triangle primitive"));
    }
    Ok(mesh)
}

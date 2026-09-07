// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! VTK file format I/O (`vtkio`), plus project-owned filters (threshold, clip,
//! derive-scalar). `vtk-pure-rs` adopted only if its filter pipeline is needed
//! beyond what filters.rs reasonably covers -- see source doc §1.4.
//!
//! **What loads (2026-09-06, GAP-023)**: legacy `.vtk` files whose data set is
//! `POLYDATA`, read through `vtkio::Vtk::import`. Polygons of any arity are fanned into
//! triangles; the first scalar point attribute becomes the scalar field. An
//! `UNSTRUCTURED_GRID`, image, rectilinear or structured data set is refused by name
//! rather than read as an empty mesh, because an empty mesh is a claim that the file
//! held nothing.
//!
//! **XML VTK (`.vtp`, `.vtu`, ...) is refused by name since 2026-09-07.** vtkio's XML
//! reader is compiled out (the workspace `Cargo.toml` vtkio row says why: its quick-xml
//! 0.22 carries two open RUSTSEC advisories and no vtkio release is on a patched line),
//! and a file with an XML extension gets an error that names the extension and the
//! reason, not "unknown format". The `VertexNumbers::XML` arm below stays because the
//! model type is not feature-gated; it is unreachable until the reader returns.

pub mod filters;

use std::path::Path;

use vtkio::model::{Attribute, DataSet, ElementType, VertexNumbers};
use vtkio::IOBuffer;

use crate::DataError;

#[derive(Debug, Clone, Default)]
pub struct MeshData {
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub scalar_field: Option<Vec<f32>>,
}

impl MeshData {
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

fn parse(reason: impl std::fmt::Display) -> DataError {
    DataError::Parse(reason.to_string())
}

fn to_f32(buffer: IOBuffer, what: &str) -> Result<Vec<f32>, DataError> {
    buffer
        .cast_into::<f32>()
        .ok_or_else(|| parse(format!("{what} are not a numeric array")))
}

fn index(v: u64, vertex_count: usize) -> Result<u32, DataError> {
    let i = u32::try_from(v).map_err(|_| parse(format!("vertex index {v} exceeds u32")))?;
    if (i as usize) >= vertex_count {
        return Err(parse(format!("polygon names vertex {i} of {vertex_count}")));
    }
    Ok(i)
}

/// Fan each polygon into triangles, appending to `indices`.
fn fan(polygon: &[u64], vertex_count: usize, indices: &mut Vec<u32>) -> Result<(), DataError> {
    if polygon.len() < 3 {
        return Err(parse(format!(
            "a polygon of {} vertices is not a face",
            polygon.len()
        )));
    }
    let first = index(polygon[0], vertex_count)?;
    for pair in polygon[1..].windows(2) {
        indices.push(first);
        indices.push(index(pair[0], vertex_count)?);
        indices.push(index(pair[1], vertex_count)?);
    }
    Ok(())
}

fn triangulate(polys: &VertexNumbers, vertex_count: usize) -> Result<Vec<u32>, DataError> {
    let mut indices = Vec::new();
    match polys {
        VertexNumbers::Legacy {
            num_cells,
            vertices,
        } => {
            let mut at = 0usize;
            for _ in 0..*num_cells {
                let n = *vertices
                    .get(at)
                    .ok_or_else(|| parse("polygon list ends before its cell count"))?
                    as usize;
                let cell = vertices
                    .get(at + 1..at + 1 + n)
                    .ok_or_else(|| parse("polygon list ends inside a cell"))?;
                let cell: Vec<u64> = cell.iter().map(|&v| u64::from(v)).collect();
                fan(&cell, vertex_count, &mut indices)?;
                at += 1 + n;
            }
        }
        VertexNumbers::XML {
            connectivity,
            offsets,
        } => {
            let mut start = 0usize;
            for &end in offsets {
                let end = usize::try_from(end).map_err(|_| parse("offset exceeds usize"))?;
                let cell = connectivity
                    .get(start..end)
                    .ok_or_else(|| parse("connectivity ends before its offsets"))?;
                fan(cell, vertex_count, &mut indices)?;
                start = end;
            }
        }
    }
    Ok(indices)
}

/// Load a VTK `POLYDATA` file into a triangle mesh.
///
/// # Errors
///
/// `DataError::Io` when the file cannot be read; `DataError::Parse` when it is not VTK,
/// is a data set other than `POLYDATA`, or its polygons name vertices it does not have.
pub fn load_vtk(path: &Path) -> Result<MeshData, DataError> {
    let vtk = vtkio::Vtk::import(path).map_err(|e| match e {
        vtkio::Error::IO(io) => DataError::Io(io.to_string()),
        // With vtkio's `xml` feature off this is every extension but `.vtk`, and a
        // missing extension. Name the cause: an operator handed a `.vtp` should learn
        // that the XML reader is out, not that the file is unrecognised.
        vtkio::Error::UnknownFileExtension(_) => parse(format!(
            "{}: only legacy .vtk is read; the XML VTK reader (.vtp, .vtu, ...) is compiled \
             out until vtkio moves off quick-xml 0.22 (RUSTSEC-2026-0194/0195; see the \
             workspace Cargo.toml)",
            path.display()
        )),
        other => parse(other),
    })?;
    let DataSet::PolyData { pieces, .. } = vtk.data else {
        return Err(parse(
            "the data set is not POLYDATA; only polygon surfaces are read (GAP-023)",
        ));
    };
    let mut mesh = MeshData::default();
    for piece in pieces {
        let piece = piece.load_piece_data(None).map_err(parse)?;
        let base = mesh.positions.len();
        let points = to_f32(piece.points, "points")?;
        if !points.len().is_multiple_of(3) {
            return Err(parse("the point array is not a multiple of three"));
        }
        let count = points.len() / 3;
        let (triples, _) = points.as_chunks::<3>();
        mesh.positions.extend(triples.iter().copied());
        if let Some(polys) = &piece.polys {
            let base = u32::try_from(base).map_err(|_| parse("too many vertices"))?;
            mesh.indices
                .extend(triangulate(polys, count)?.into_iter().map(|i| i + base));
        }
        // The first scalar point attribute is the field the viewport colours by.
        for attribute in piece.data.point {
            if let Attribute::DataArray(array) = attribute {
                if matches!(array.elem, ElementType::Scalars { .. }) {
                    let values = to_f32(array.data, "scalars")?;
                    if values.len() != count {
                        return Err(parse(format!(
                            "scalar {} has {} values for {count} points",
                            array.name,
                            values.len()
                        )));
                    }
                    let field = mesh.scalar_field.get_or_insert_with(Vec::new);
                    field.extend(values);
                    break;
                }
            }
        }
    }
    if mesh.positions.is_empty() {
        return Err(parse("the file holds no points"));
    }
    if let Some(field) = &mesh.scalar_field {
        if field.len() != mesh.positions.len() {
            // A second piece without the scalar would leave the field short; refuse
            // rather than colour half a surface.
            return Err(parse("not every piece carries the scalar field"));
        }
    }
    Ok(mesh)
}

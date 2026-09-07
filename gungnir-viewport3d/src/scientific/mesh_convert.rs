// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! `gungnir_data::scientific::MeshData` into a three-d `CpuMesh` (GAP-023).
//!
//! Colour comes from the scalar field when the mesh has one (the viridis-like ramp in
//! [`crate::scientific::colormap`]), else a single neutral grey; normals are computed by
//! three-d from the faces. An empty mesh is an error rather than an empty `CpuMesh`,
//! which the scene would draw as a mesh that is there and has no geometry.

use gungnir_data::scientific::MeshData;

/// # Errors
///
/// [`crate::ViewportError::EmptyMesh`] when the mesh has no triangle, and
/// [`crate::ViewportError::MalformedMesh`] when an index names a vertex it does not have.
pub fn to_cpu_mesh(mesh: &MeshData) -> Result<three_d::CpuMesh, crate::ViewportError> {
    if mesh.indices.len() < 3 || mesh.positions.is_empty() {
        return Err(crate::ViewportError::EmptyMesh);
    }
    let count = mesh.positions.len();
    if let Some(bad) = mesh.indices.iter().find(|&&i| i as usize >= count) {
        return Err(crate::ViewportError::MalformedMesh {
            reason: format!("index {bad} names a vertex of {count}"),
        });
    }
    let positions: Vec<three_d::Vector3<f32>> = mesh
        .positions
        .iter()
        .map(|p| three_d::vec3(p[0], p[1], p[2]))
        .collect();
    let colors = mesh.scalar_field.as_ref().map(|field| {
        let (lo, hi) = field
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| {
                (lo.min(v), hi.max(v))
            });
        field
            .iter()
            .map(|&v| {
                let t = if hi > lo && v.is_finite() {
                    (v - lo) / (hi - lo)
                } else {
                    0.0
                };
                let [r, g, b] = super::colormap::viridis(t);
                three_d::Srgba::new(r, g, b, 255)
            })
            .collect()
    });
    let mut cpu = three_d::CpuMesh {
        positions: three_d::Positions::F32(positions),
        indices: three_d::Indices::U32(mesh.indices.clone()),
        colors,
        ..Default::default()
    };
    cpu.compute_normals();
    Ok(cpu)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mesh_with_a_scalar_converts_with_a_colour_per_vertex() {
        let mesh = MeshData {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            indices: vec![0, 1, 2],
            scalar_field: Some(vec![0.0, 0.5, 1.0]),
        };
        let cpu = to_cpu_mesh(&mesh).expect("converts");
        assert_eq!(cpu.positions.len(), 3);
        assert_eq!(cpu.colors.as_ref().map(Vec::len), Some(3));
        assert!(cpu.normals.is_some());
    }

    #[test]
    fn an_empty_or_malformed_mesh_is_an_error_and_never_an_empty_scene_object() {
        assert!(matches!(
            to_cpu_mesh(&MeshData::default()),
            Err(crate::ViewportError::EmptyMesh)
        ));
        let bad = MeshData {
            positions: vec![[0.0, 0.0, 0.0]],
            indices: vec![0, 1, 2],
            scalar_field: None,
        };
        assert!(matches!(
            to_cpu_mesh(&bad),
            Err(crate::ViewportError::MalformedMesh { .. })
        ));
    }
}

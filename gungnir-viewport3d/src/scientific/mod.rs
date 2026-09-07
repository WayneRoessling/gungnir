// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! vtk-threed-bridge: `MeshData` -> `three_d::CpuMesh` + colormap, per
//! rust-3d-data-ecosystem-build-vs-adopt.md §2.2. Small, no streaming/LOD concern --
//! the recommended first bridge to build (§4 build order).

pub mod colormap;
pub mod mesh_convert;

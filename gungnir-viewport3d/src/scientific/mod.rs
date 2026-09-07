//! vtk-threed-bridge: `MeshData` -> `three_d::CpuMesh` + colormap, per
//! rust-3d-data-ecosystem-build-vs-adopt.md §2.2. Small, no streaming/LOD concern --
//! the recommended first bridge to build (§4 build order).

pub mod colormap;
pub mod mesh_convert;

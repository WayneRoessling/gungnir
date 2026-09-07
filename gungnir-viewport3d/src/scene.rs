// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Static mesh/surface construction & updates -- terrain, glTF assets, VTK meshes.
//! Objects are created once when data loads, never per frame
//! (rust-ui-architecture-coding-standards.md §5).

/// The static three-d objects (terrain, assets, scientific meshes). Empty until a GL
/// context exists and `gungnir-data` has loaded something.
#[derive(Default)]
pub struct Scene {
    pub objects: Vec<Box<dyn three_d::Object>>,
}

impl Scene {
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }
}

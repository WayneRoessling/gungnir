// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! tiles3d-threed bridge: streaming 3D Tiles / COPC-EPT into three-d, per
//! rust-3d-data-ecosystem-build-vs-adopt.md §2.1. Deferred until a specific panel
//! actually requires planet/site-scale streamed terrain (§4 build order) -- scaffolded
//! here so the module boundary exists when that requirement lands.

pub mod content;
pub mod sse;
pub mod tile_cache;
pub mod tileset;

pub struct StreamingLayer {
    root: tileset::TilesetNode,
    cache: tile_cache::TileCache,
}

impl StreamingLayer {
    pub fn new(root: tileset::TilesetNode) -> Self {
        Self {
            root,
            cache: tile_cache::TileCache,
        }
    }

    /// The tileset root this layer streams from.
    pub fn root(&self) -> &tileset::TilesetNode {
        &self.root
    }

    /// Called once per frame. Cheap: only walks already-resident nodes and issues
    /// background fetch requests for newly-needed tiles; never blocks.
    /// # Errors
    ///
    /// Always: tile refinement is designed and not written (GAP-082).
    ///
    /// **Returns a `Result` rather than doing nothing.** A silent no-op here is a
    /// streaming layer that reports it updated and never loads a tile, which looks
    /// exactly like a scene with nothing in view.
    pub fn update(
        &mut self,
        _camera: &three_d::Camera,
        _ctx: &three_d::Context,
    ) -> Result<(), crate::ViewportError> {
        Err(crate::ViewportError::NotImplemented {
            what: "streaming tile refinement",
            waiting_on: "screen_space_error and the tileset parser, both unwritten",
        })
    }

    pub fn visible_objects(&self) -> impl Iterator<Item = &dyn three_d::Object> {
        self.cache.resident_objects()
    }
}

// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

pub struct TilesetNode {
    pub children: Vec<TilesetNode>,
}
/// # Errors
///
/// Always: the tileset parser is designed and not written (GAP-082).
///
/// **Returns a `Result` rather than an empty root**, which the streaming layer would have
/// walked as a tileset containing no tiles.
pub fn parse_tileset_json(_path: &std::path::Path) -> Result<TilesetNode, crate::ViewportError> {
    Err(crate::ViewportError::NotImplemented {
        what: "3D Tiles tileset.json parsing",
        waiting_on: "a signed-off 3D Tiles dependency; none is in the workspace",
    })
}

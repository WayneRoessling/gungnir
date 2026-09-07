// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Screen-space-error calc against the live three-d Camera -- the actual novel work
//! per §2.1, since no existing crate wires tile refinement to three-d's camera types.
/// # Errors
///
/// Always: the screen-space-error calculation is designed and not written (GAP-082).
///
/// **Returns a `Result` rather than a number.** Zero would mean "this tile needs no
/// refinement" and infinity "refine it immediately"; both are answers, and there is none.
pub fn screen_space_error(
    _node: &super::tileset::TilesetNode,
    _camera: &three_d::Camera,
) -> Result<f32, crate::ViewportError> {
    Err(crate::ViewportError::NotImplemented {
        what: "screen-space error against the live camera",
        waiting_on: "nothing but the work; this is the novel part of the streaming design",
    })
}

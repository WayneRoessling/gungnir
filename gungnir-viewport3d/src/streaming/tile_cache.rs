// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! LRU GPU-resource cache keyed by `TileId`, per §2.1: GPU buffers created once per
//! tile and reused, never recreated per frame.
pub struct TileCache;
impl TileCache {
    pub fn resident_objects(&self) -> impl Iterator<Item = &dyn three_d::Object> {
        std::iter::empty()
    }
}

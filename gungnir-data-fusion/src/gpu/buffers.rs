// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Persistent GPU buffer management -- created once, resized only on point-count
//! change, never recreated per frame (rust-ui-architecture-coding-standards.md §5).

pub struct PersistentPointBuffer {
    pub buffer: Option<wgpu::Buffer>,
    pub capacity_points: usize,
}

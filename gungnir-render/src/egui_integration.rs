// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! egui <-> wgpu render pass wiring. **Inactive**: the application presents through
//! eframe's `glow` backend so that three-d has an OpenGL context
//! (ARCHITECTURE.md §9). Kept as the seam where an egui-over-wgpu presentation path
//! would attach if the 3D layer were ever replaced by a wgpu-native renderer. If it
//! is ever activated, the render loop must never panic
//! (rust-ui-architecture-coding-standards.md §4): a transient resource failure
//! logs and skips the frame's draw.

/// Placeholder; constructing it does nothing until the wgpu presentation path exists.
#[derive(Debug, Default)]
pub struct EguiRenderer;

impl EguiRenderer {
    /// Reports whether a wgpu presentation path is compiled in. Always false today.
    pub fn is_active(&self) -> bool {
        false
    }
}

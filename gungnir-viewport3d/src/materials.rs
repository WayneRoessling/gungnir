// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Shared materials/palettes -- colours come from `gungnir-ui::theme` so 2D and 3D
//! visualizations stay consistent (rust-3d-data-ecosystem-build-vs-adopt.md §2.2).

use gungnir_model::TrackStatus;
use gungnir_ui::theme;

/// Normalized RGBA for a three-d material, from the shared egui palette.
pub fn track_rgba(status: TrackStatus, stale: bool) -> [f32; 4] {
    let c = theme::track_color(status, stale);
    [
        f32::from(c.r()) / 255.0,
        f32::from(c.g()) / 255.0,
        f32::from(c.b()) / 255.0,
        f32::from(c.a()) / 255.0,
    ]
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn rgba_is_normalized() {
        let c = track_rgba(TrackStatus::Confirmed, false);
        assert!(c.iter().all(|v| (0.0..=1.0).contains(v)));
        assert_eq!(c[3], 1.0);
    }
}

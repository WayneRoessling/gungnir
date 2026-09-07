// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! track-manager: init/confirm/coast/delete lifecycle --
//! verification-capability-table.md "Track lifecycle" row. Exact match on step index
//! vs. Stone Soup initiators/deleters and MATLAB trackHistoryLogic/trackScoreLogic.

pub mod lifecycle;

pub use lifecycle::{StepOutcome, TrackManager};

/// Re-exported from `gungnir-core`, the single owning crate for these primitives
/// (agentic-coding-standards.md §1.2). `gungnir-model` re-exports the same types.
// `MotionModel` and the concrete models are re-exported rather than left for a
// dependent to reach directly. `gungnir-rfs` may depend on `gungnir-track` and nothing
// else (ARCHITECTURE.md §7.1) and its PHD filter needs a motion model to predict with;
// re-exporting is what CLAUDE.md's "never redefine a type that gungnir-core owns"
// requires, and the same pattern `gungnir-model` uses for `gungnir-assessment`.
pub use gungnir_core::{
    ConstantAcceleration, ConstantVelocity, CoordinatedTurn, MotionModel, TrackId, TrackStatus,
};

/// A single tracked object as the tracking core sees it: kinematic state only. This
/// is the type re-exported (not redefined) by gungnir-rfs and gungnir-track-fusion,
/// per agentic-coding-standards.md §1.2. The service facade projects it into
/// `gungnir_model::TrackView`, which adds provenance, quality, and classification.
#[derive(Debug, Clone)]
pub struct Track {
    pub id: TrackId,
    pub status: TrackStatus,
    /// Position (m) and velocity (m/s) per axis in the local ENU tangent frame the
    /// tracking service was configured with: `[e, n, u, ve, vn, vu]`.
    pub state: nalgebra::SVector<f64, 6>,
    pub covariance: nalgebra::SMatrix<f64, 6, 6>,
    /// Consecutive cycles since the last hit. Reset by a hit; the deletion rule reads
    /// it (`lifecycle`).
    pub misses_since_update: u32,
    /// Cumulative hits over the track's life. The confirmation rule reads it, and it
    /// is cumulative rather than a consecutive run because that is what the row's
    /// oracle does -- see `lifecycle`, which records why the scaffold's original
    /// comment was corrected.
    pub hits: u32,
}

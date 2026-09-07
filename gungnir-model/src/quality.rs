// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Association confidence, covariance quality, and freshness/latency, per
//! docs/gungnir-capabilities.md §5.2.

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Quality {
    /// 0.0 (no confidence) .. 1.0 (unambiguous association).
    pub association_confidence: f32,
    /// Receipt-to-estimate latency, seconds.
    pub latency_s: f32,
    /// True once the estimate is older than the configured freshness limit; stale
    /// tracks must be drawn differently (§5.2) and never used for allocation.
    pub is_stale: bool,
}

impl Default for Quality {
    /// A track with no quality information yet: zero confidence, zero latency,
    /// not stale. Producers must overwrite `association_confidence` before display.
    fn default() -> Self {
        Self {
            association_confidence: 0.0,
            latency_s: 0.0,
            is_stale: false,
        }
    }
}

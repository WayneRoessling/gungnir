// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! ui/: 2D egui dashboard panels + theme. Panels read from `AppState` (owned by
//! `gungnir-app`) through `gungnir-model` views, never hold their own duplicate
//! copies of business data (rust-ui-architecture-coding-standards.md §2), and do no
//! allocation-heavy work inside `render_*` beyond the per-row labels egui itself
//! needs.

/// A headless render probe, so a test can assert on what a panel drew rather than on
/// the view struct it was handed. Enabled by this crate's own tests and by
/// `gungnir-app`'s; off in an ordinary build.
#[cfg(any(test, feature = "harness"))]
pub mod harness;

pub mod panels;
pub mod theme;

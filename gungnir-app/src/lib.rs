// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The desktop's non-render half, exposed as a library so it can be measured.
//!
//! `main.rs` remains eframe bootstrap only, per
//! rust-ui-architecture-coding-standards.md §1: this crate is wiring, not logic. The
//! library target exists because `docs/performance-budgets.md` names
//! `gungnir-app::update::tick` as the thing the desktop budgets are measured around
//! -- the per-frame `update()` budget, the snapshot-call budget, and the journal
//! append budget are all properties of that function -- and a crate with only a
//! `[[bin]]` target cannot be reached by a `criterion` harness or an integration
//! test. The harness is `benches/app_tick.rs` (GAP-056).
//!
//! Nothing here is new behaviour: [`state`] and [`update`] are the same two modules
//! the binary has always had, and `main.rs` now uses them through this target rather
//! than declaring them itself, so there is exactly one copy of each.

pub mod anomaly;
pub mod audit;
pub mod cooperative;
pub mod decisions;
pub mod deliveries;
pub mod dock;
pub mod engagements;
pub mod failover;
pub mod geofences;
pub mod governance;
pub mod handoffs;
pub mod hazards;
pub mod identity;
pub mod keystore;
pub mod node_tasks;
pub mod peers;
pub mod prediction;
pub mod radar;
pub mod rehearsal;
pub mod requirements;
pub mod review;
pub mod rhythm;
pub mod session;
pub mod state;
pub mod status;
pub mod sustainment;
pub mod terrain;
pub mod update;
pub mod warnings;
pub mod workspace;

pub use state::{AppError, AppState, CONFIG_ENV_VAR};
pub use update::tick;

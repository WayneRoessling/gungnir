// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Protocol adapters. Live adapters (radar, EO/IR, ADS-B/AIS, lidar, external C2
//! via `gungnir-interop` codecs) are deployment-specific and added here as they are
//! integrated. `asterix` is the first live one (GAP-001, radar half, 2026-09-06);
//! `recorded` and `simulated` exist so the full gateway path can be exercised with
//! no real sensor connected.

/// A 1090ES receiver: the codec has been decoded and gated since 2026-09-06 (GAP-010)
/// and had no adapter of its own until this one (2026-09-07).
pub mod adsb;
pub mod ais;
pub mod asterix;
pub mod peer;
pub mod recorded;
/// The SAPIENT spotter feed: a person is an edge node (GAP-001's human half, GAP-004's
/// sensor half; `docs/design/external-standards.md` §7 and
/// `docs/design/DN-27-bearing-only-detections.md`).
pub mod sapient;
pub mod simulated;

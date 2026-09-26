// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The journal's lossless line for an envelope carrying a non-finite float (GAP-126,
//! D-77), re-exported from `gungnir-eventing/src/nonfinite.rs`.
//!
//! **It lived here until GAP-153** (D-96), when the v3 wire took the same form: the node's
//! transport and the desktop's link had to reach the codec, and neither may depend on this
//! crate, while all three depend on `gungnir-eventing`. One codec in one place, so the
//! journal and the wire cannot drift into two spellings of the same float.
pub use gungnir_eventing::nonfinite::*;

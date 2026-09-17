// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! What the node's binary does that a test has to be able to drive.
//!
//! `gungnir-node` was a binary and nothing else until GAP-132. DN-31 §9 rows 3 to 6 are
//! about what a *node* does -- two clients racing on one item over the real transport,
//! each role against each item state, the offering rule, expiry and escalation on the
//! node's clock -- and a test cannot reach inside a binary. `gungnir-app` has had a
//! `[lib]` beside its `[[bin]]` since it was written, for exactly this reason.
//!
//! So this crate is a library the binary uses, rather than a second copy of the loop kept
//! in step with `main.rs` by hand. It holds only what a test must drive:
//! [`approval`], the node's side of `gungnir-approval` (edge (y), D-57). Everything else
//! -- the services, the journal, the transport's start-up, the account command -- stays in
//! `main.rs`, because moving code here that nothing outside the binary calls would make
//! the crate's surface a list of things nobody uses.

pub mod approval;

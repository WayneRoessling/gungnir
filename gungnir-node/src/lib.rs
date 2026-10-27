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
//! [`approval`], the node's side of `gungnir-approval` (edge (y), D-57), and since
//! GAP-120 [`picture`], the construction of the services that make the node's picture
//! and what goes out with it, which the backend-switching row's test runs against an
//! embedded desktop. Since GAP-163 and GAP-152, [`audit_record`]: the audit log's head in
//! the journal, its verification at start and its retention, which
//! `gungnir-node/tests/node_audit_record.rs` drives against a real journal. Since GAP-194,
//! [`gap_acceptance`]: the coverage gap acceptances the node holds for its linked
//! desktops (D-131), whose rules its unit tests hold. Since GAP-203, [`control_status`]:
//! the live weapons-control-status changes the node applies, journals and folds back at
//! start (DN-09 §10), which `gungnir-node/tests/control_status.rs` drives over the real
//! transport. Everything
//! else -- the journal, the transport's start-up, the feeds, the account command -- stays
//! in `main.rs`, because moving code here that nothing outside the binary calls would
//! make the crate's surface a list of things nobody uses.

pub mod approval;
pub mod audit_record;
pub mod clock_discipline;
pub mod control_status;
pub mod gap_acceptance;
pub mod picture;

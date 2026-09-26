// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! One audit entry per gated action (GAP-059, contract C-04).
//!
//! The log existed and the binaries wrote to it from two places: applying a baseline and
//! signing in. Every other act a person takes -- deciding a plan, commanding a sensor,
//! stating or tasking a requirement, conducting a review -- now writes a row at the point
//! it is wired, attributed to the verified operator when there is one and to nobody when
//! there is not (DN-23 §5 rule 1: attribution is never invented).

use crate::state::AppState;
use gungnir_model::MissionTime;
use gungnir_security::{AuditEntry, AuditLog, OperatorId};

/// Record an action the operator just took, at this frame's mission time.
pub fn record(state: &mut AppState, action: &str, detail: impl Into<String>) {
    let entry = entry(
        state.attributed_operator(),
        state.clock.now(),
        action,
        detail,
    );
    state.audit.record(entry);
}

/// One entry, built from the attribution and the time the caller has already read.
///
/// Split out for `gungnir-approval`'s host (GAP-131): the decision path writes its audit
/// entries through a trait rather than through [`AppState`], and two constructions of an
/// entry would be two places the attribution rule could drift.
pub(crate) fn entry(
    operator: Option<OperatorId>,
    now: MissionTime,
    action: &str,
    detail: impl Into<String>,
) -> AuditEntry {
    AuditEntry::new(operator, action, now.0, detail)
}

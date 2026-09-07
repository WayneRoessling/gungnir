//! One audit entry per gated action (GAP-059, contract C-04).
//!
//! The log existed and the binaries wrote to it from two places: applying a baseline and
//! signing in. Every other act a person takes -- deciding a plan, commanding a sensor,
//! stating or tasking a requirement, conducting a review -- now writes a row at the point
//! it is wired, attributed to the verified operator when there is one and to nobody when
//! there is not (DN-23 §5 rule 1: attribution is never invented).

use crate::state::AppState;
use gungnir_security::{AuditEntry, AuditLog};

/// Record an action the operator just took, at this frame's mission time.
pub fn record(state: &mut AppState, action: &str, detail: impl Into<String>) {
    let entry = AuditEntry {
        operator: state.attributed_operator(),
        action: action.to_owned(),
        mission_time: state.clock.now().0,
        detail: detail.into(),
    };
    state.audit.record(entry);
}

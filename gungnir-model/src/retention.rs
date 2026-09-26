// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! How long a deployment keeps its record (GAP-122, decision D-78).
//!
//! Owned here rather than by `gungnir-store`, which applies it, because the
//! configuration baseline declares it and `gungnir-config` may not depend on the store.
//! Two crates share it, so it lives in the lowest one both reach
//! (agentic-coding-standards.md §1.2), the treatment `SessionId` had; `gungnir-store`
//! re-exports it, so the path it always had still resolves.
//!
//! **Declared, never assumed.** `ConfigBaseline::retention` is optional and absent means
//! nothing is purged: the data architecture leaves retention periods to the customer's
//! record-keeping obligation (`docs/architecture/togaf/phase-c-information-systems/data-architecture.md`
//! §4), and a product that began deleting mission records on an upgrade because of a
//! default nobody chose would be the worse failure of the two.

/// The ages past which a deployment's record is purged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RetentionPolicy {
    /// A session is purged once its journal has not been written for **more than** this
    /// many days. Measured from the last write, not the first, so a long session is kept
    /// for the full period after it ended.
    pub max_session_age_days: u32,
    /// The audit log's age limit.
    ///
    /// **Governs nothing yet**: the audit log is held in memory for the life of the
    /// process (`gungnir_security::InMemoryAuditLog`), so nothing durable holds one to
    /// purge. GAP-152 is the durable log this would apply to.
    pub max_audit_log_age_days: u32,
}

impl Default for RetentionPolicy {
    /// The values the configuration editor offers: sessions for 90 days, audit logs for a
    /// year. **Offered, not applied**: a baseline that does not declare a policy purges
    /// nothing (D-78).
    fn default() -> Self {
        Self {
            max_session_age_days: 90,
            max_audit_log_age_days: 365,
        }
    }
}

impl RetentionPolicy {
    /// True when a session last written `age_days` ago is past the limit. **Strictly
    /// after**: a session exactly at the limit is kept.
    #[must_use]
    pub fn session_expired(&self, age_days: f64) -> bool {
        age_days > f64::from(self.max_session_age_days)
    }

    /// True when an audit log `age_days` old is past the limit, strictly after it.
    #[must_use]
    pub fn audit_log_expired(&self, age_days: f64) -> bool {
        age_days > f64::from(self.max_audit_log_age_days)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_is_strictly_after_the_limit() {
        let p = RetentionPolicy {
            max_session_age_days: 10,
            max_audit_log_age_days: 20,
        };
        assert!(!p.session_expired(10.0));
        assert!(p.session_expired(10.5));
        assert!(!p.audit_log_expired(20.0));
        assert!(p.audit_log_expired(21.0));
    }
}

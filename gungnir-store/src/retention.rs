// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Retention/deletion policy -- how long sessions, replay artifacts, and audit
//! logs are kept before being purged, per docs/gungnir-capabilities.md §5.1.
//! Applying the policy (deleting files) is a deliberate operator or scheduled
//! action, never a side effect of opening the journal.

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RetentionPolicy {
    pub max_session_age_days: u32,
    pub max_audit_log_age_days: u32,
}

impl Default for RetentionPolicy {
    /// Sessions for 90 days, audit logs for a year; deployments override in
    /// `gungnir-config`.
    fn default() -> Self {
        Self {
            max_session_age_days: 90,
            max_audit_log_age_days: 365,
        }
    }
}

impl RetentionPolicy {
    pub fn session_expired(&self, age_days: f64) -> bool {
        age_days > f64::from(self.max_session_age_days)
    }

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

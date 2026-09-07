// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Audit logging for configuration/algorithm/plan changes -- every
//! `gungnir_command::DecisionRecord` and `gungnir-config` baseline apply is written
//! here, independent of `gungnir-store`'s mission-session recording. Append-only.

use crate::OperatorId;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AuditEntry {
    pub operator: Option<OperatorId>,
    /// One of `crate::actions`, or a free-form description for events without an
    /// actor (e.g. an automatic fallback).
    pub action: String,
    /// Mission time, seconds (`gungnir_model::MissionTime` without the dependency).
    pub mission_time: f64,
    pub detail: String,
}

pub trait AuditLog: Send + Sync {
    fn record(&mut self, entry: AuditEntry);
    fn entries(&self) -> &[AuditEntry];
}

#[derive(Debug, Default)]
pub struct InMemoryAuditLog {
    entries: Vec<AuditEntry>,
}

impl InMemoryAuditLog {
    pub fn new() -> Self {
        Self::default()
    }
}

impl AuditLog for InMemoryAuditLog {
    fn record(&mut self, entry: AuditEntry) {
        self.entries.push(entry);
    }

    fn entries(&self) -> &[AuditEntry] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_kept_in_order() {
        let mut log = InMemoryAuditLog::new();
        for i in 0..3 {
            log.record(AuditEntry {
                operator: Some(OperatorId(1)),
                action: "plan.decide".into(),
                mission_time: f64::from(i),
                detail: String::new(),
            });
        }
        assert_eq!(log.entries().len(), 3);
        assert!((log.entries()[2].mission_time - 2.0).abs() < f64::EPSILON);
    }
}

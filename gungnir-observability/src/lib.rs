// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Health, observability & operational diagnostics, per
//! docs/gungnir-capabilities.md §5.5. `tracing` (per agentic-coding-standards.md
//! §2.8) answers "help a developer debug a specific test failure." This crate
//! answers a different question for a different audience: "is this deployed system
//! healthy right now." On a service node it also backs the health endpoint
//! (ARCHITECTURE.md §8.3).

use gungnir_model::{SystemHealth, TrackId};

/// Thresholds the host binaries watch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WatchdogConfig {
    pub max_ingest_gap_s: f32,
    pub max_tracking_pipeline_latency_s: f32,
}

pub trait HealthMonitor: Send + Sync {
    fn current_health(&self) -> SystemHealth;
    /// Correlates related alerts (e.g. a sensor dropout *and* the resulting
    /// coasting tracks it caused) into one operator-facing incident rather than
    /// a flood of individually-true but redundant alerts.
    fn correlated_alerts(&self) -> Vec<Alert>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alert {
    pub severity: AlertSeverity,
    pub summary: String,
    pub related_track_ids: Vec<TrackId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

/// Holds the latest health snapshot the host reported plus raw alerts, and
/// correlates alerts with the same summary into one, keeping the highest severity
/// and the union of related tracks.
#[derive(Debug, Default)]
pub struct SnapshotHealthMonitor {
    health: SystemHealth,
    alerts: Vec<Alert>,
}

impl SnapshotHealthMonitor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update_health(&mut self, health: SystemHealth) {
        self.health = health;
    }

    pub fn raise(&mut self, alert: Alert) {
        self.alerts.push(alert);
    }

    pub fn clear(&mut self) {
        self.alerts.clear();
    }

    pub fn raw_alert_count(&self) -> usize {
        self.alerts.len()
    }
}

impl HealthMonitor for SnapshotHealthMonitor {
    fn current_health(&self) -> SystemHealth {
        self.health
    }

    fn correlated_alerts(&self) -> Vec<Alert> {
        let mut merged: Vec<Alert> = Vec::new();
        for alert in &self.alerts {
            if let Some(existing) = merged.iter_mut().find(|a| a.summary == alert.summary) {
                existing.severity = existing.severity.max(alert.severity);
                for id in &alert.related_track_ids {
                    if !existing.related_track_ids.contains(id) {
                        existing.related_track_ids.push(*id);
                    }
                }
            } else {
                merged.push(alert.clone());
            }
        }
        merged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_summaries_are_correlated_with_highest_severity() {
        let mut m = SnapshotHealthMonitor::new();
        m.raise(Alert {
            severity: AlertSeverity::Warning,
            summary: "sensor 3 dropout".into(),
            related_track_ids: vec![TrackId(1)],
        });
        m.raise(Alert {
            severity: AlertSeverity::Critical,
            summary: "sensor 3 dropout".into(),
            related_track_ids: vec![TrackId(2), TrackId(1)],
        });
        m.raise(Alert {
            severity: AlertSeverity::Info,
            summary: "plan superseded".into(),
            related_track_ids: vec![],
        });
        let correlated = m.correlated_alerts();
        assert_eq!(correlated.len(), 2);
        assert!(correlated.len() <= m.raw_alert_count());
        assert_eq!(correlated[0].severity, AlertSeverity::Critical);
        assert_eq!(
            correlated[0].related_track_ids,
            vec![TrackId(1), TrackId(2)]
        );
    }

    #[test]
    fn health_mirrors_what_the_host_reported() {
        let mut m = SnapshotHealthMonitor::new();
        let h = SystemHealth {
            tracking_healthy: true,
            intercept_healthy: false,
            ingest_healthy: true,
        };
        m.update_health(h);
        assert_eq!(m.current_health(), h);
        assert!(!m.current_health().all_healthy());
    }
}

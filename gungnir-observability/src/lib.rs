// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Health, observability & operational diagnostics, per
//! docs/gungnir-capabilities.md §5.5. `tracing` (per agentic-coding-standards.md
//! §2.8) answers "help a developer debug a specific test failure." This crate
//! answers a different question for a different audience: "is this deployed system
//! healthy right now." On a service node it also backs the health endpoint
//! (ARCHITECTURE.md §8.3).

use gungnir_model::events::HealthEvent;
use gungnir_model::{MissionTime, SystemHealth, TrackId};

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

/// Holds the latest health the host's services reported plus raw alerts, says when
/// the health changed, and correlates alerts with the same summary into one, keeping
/// the highest severity and the union of related tracks.
///
/// **Both binaries report through this** (GAP-125, D-100): the desktop's tick
/// (`gungnir-app/src/update.rs`, step 4) and the node's loop (`gungnir_node::picture::
/// Announcer::health`). Until then each kept its own copy of "what was said last" and
/// its own comparison, and neither used this type, so the one test of it set a health
/// once and never saw one change. The flags themselves are still read by each binary
/// from its own services, because they are that binary's services; what is shared is
/// what a report means -- the value on screen, and whether the record needs a
/// transition.
#[derive(Debug, Default)]
pub struct SnapshotHealthMonitor {
    health: SystemHealth,
    /// Whether anything has been reported yet. The first report is always a
    /// transition: the record must say what the health was when the session began,
    /// even if every service was down from the start, which is the same value as
    /// `SystemHealth::default()` and would otherwise look like no change at all.
    reported: bool,
    alerts: Vec<Alert>,
}

impl SnapshotHealthMonitor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Take what the services report now. Returns the transition to put on the record
    /// (MOE-06) when it differs from the last report, or when it is the first, and
    /// `None` for a repeat: the change is the fact, and a record of every tick would
    /// bury it.
    ///
    /// The health is never inferred here. It is exactly what the host passed, and
    /// [`HealthMonitor::current_health`] returns it until the next report.
    #[must_use]
    pub fn report(&mut self, health: SystemHealth, at: MissionTime) -> Option<HealthEvent> {
        let changed = !self.reported || self.health != health;
        self.health = health;
        self.reported = true;
        changed.then_some(HealthEvent::Changed {
            tracking_healthy: health.tracking_healthy,
            intercept_healthy: health.intercept_healthy,
            ingest_healthy: health.ingest_healthy,
            at,
        })
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
        let _ = m.report(h, MissionTime(1.0));
        assert_eq!(m.current_health(), h);
        assert!(!m.current_health().all_healthy());
    }

    /// The health of one service, with the other two healthy.
    fn with(flag: usize, value: bool) -> SystemHealth {
        let mut h = SystemHealth {
            tracking_healthy: true,
            intercept_healthy: true,
            ingest_healthy: true,
        };
        match flag {
            0 => h.tracking_healthy = value,
            1 => h.intercept_healthy = value,
            _ => h.ingest_healthy = value,
        }
        h
    }

    /// **The health follows every transition of every flag, and only a transition goes
    /// on the record** (GAP-125). Each service's flag is taken true, false, true; after
    /// each report the current health is exactly what was reported, a change yields the
    /// event naming the new flags, and a repeat yields nothing. A monitor that stuck on
    /// its first value, or on the worst it had seen, would fail the second and third
    /// steps; one that published every tick would fail the repeat.
    #[test]
    fn health_follows_each_flag_true_false_true_and_records_only_the_changes() {
        let mut m = SnapshotHealthMonitor::new();
        let mut t = 0.0;
        let mut step = |m: &mut SnapshotHealthMonitor, h: SystemHealth, expect_event: bool| {
            t += 0.1;
            let event = m.report(h, MissionTime(t));
            assert_eq!(m.current_health(), h);
            match event {
                Some(HealthEvent::Changed {
                    tracking_healthy,
                    intercept_healthy,
                    ingest_healthy,
                    at,
                }) => {
                    assert!(expect_event, "a repeat of {h:?} was put on the record");
                    assert_eq!(
                        (tracking_healthy, intercept_healthy, ingest_healthy),
                        (h.tracking_healthy, h.intercept_healthy, h.ingest_healthy)
                    );
                    assert_eq!(at, MissionTime(t));
                }
                None => assert!(!expect_event, "the change to {h:?} left no record"),
            }
        };
        // The first report is a transition, even from nothing.
        step(&mut m, with(0, true), true);
        step(&mut m, with(0, true), false);
        for flag in 0..3 {
            step(&mut m, with(flag, false), true);
            step(&mut m, with(flag, false), false);
            step(&mut m, with(flag, true), true);
            step(&mut m, with(flag, true), false);
        }
    }

    /// A session that starts with every service down still records that it did: the
    /// first report equals `SystemHealth::default()`, and is a transition all the same.
    #[test]
    fn the_first_report_is_recorded_even_when_it_is_all_down() {
        let mut m = SnapshotHealthMonitor::new();
        assert!(m
            .report(SystemHealth::default(), MissionTime(0.1))
            .is_some_and(|e| e.is_degraded()));
        assert!(m
            .report(SystemHealth::default(), MissionTime(0.2))
            .is_none());
    }
}

// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Reporting, after-action review & export (report half), per
//! docs/gungnir-capabilities.md §5.6. The tracking core's RTS/fixed-lag smoothing
//! already implies post-hoc analysis is a valued use case; this crate packages
//! that analysis (plus gungnir-metrics' MOTA/MOTP scoring, where ground truth is
//! available) into something a stakeholder outside engineering can read. Every
//! figure in a report is recomputed from the journal, never from live state.

pub mod measures;
pub mod order_of_battle;
pub mod rhythm;

pub use measures::{Measure, MeasureValue};

pub use order_of_battle::{
    assemble, pattern_of_life, EntityEvidence, EntityMovement, ObservedLocation, OrderOfBattle,
    OrderOfBattleEntry, PatternOfLife, PatternSettings, ProductError, Traversal,
};
pub use rhythm::{
    absence_is_planned, HandoverSummary, MaintenanceState, MaintenanceWindow, ProductKind,
    RhythmError, Schedule, ScheduledProduct,
};

use gungnir_eventing::Event;
use gungnir_metrics::TrackingMetrics;
use gungnir_model::events::{
    CommandEvent, GovernanceEvent, IngestEvent, InterceptEvent, RequirementEvent, RhythmEvent,
    SensorEvent, SensorTaskEvent, TrackingEvent,
};
use gungnir_model::MissionTime;
use gungnir_store::{EventJournal, SessionId, StoreError};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MissionReport {
    pub session: SessionId,
    pub summary: String,
    pub counts: EventCounts,
    /// The measures catalogue for this session (GAP-047), each figure with its basis or
    /// the reason it could not be computed. **Counts are not measures**, and the two are
    /// carried apart.
    pub measures: Vec<Measure>,
    /// The report's own marking (GAP-062, DN-17 §5 rule 5): the combination of every
    /// marked item the journal holds, so a report containing one restricted track is
    /// restricted as a whole. **Internal when nothing marked was journaled**, which is
    /// the default and the safe reading, not a computed release.
    #[serde(default)]
    pub releasability: gungnir_model::Releasability,
    /// What produced the marking: how many journaled items carried each level.
    #[serde(default)]
    pub marking_inputs: MarkingInputs,
    pub first_event: Option<MissionTime>,
    pub last_event: Option<MissionTime>,
    pub metrics: Option<TrackingMetricsSummary>,
}

/// How many journaled items carried each marking level, so the combined marking on a
/// report can be checked against its inputs (DN-17 §7, PN-13).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MarkingInputs {
    pub internal: u64,
    pub parties: u64,
    pub all_peers: u64,
}

impl MarkingInputs {
    #[must_use]
    pub fn total(self) -> u64 {
        self.internal + self.parties + self.all_peers
    }
}

/// The combined marking of every marked item in the journal, and the tally behind it.
///
/// Tracks (initiated or updated) and proposed plans are the marked things the journal
/// carries. `combine` is DN-17's rule: the most restrictive input wins, and two party
/// lists with nothing in common collapse to internal.
#[must_use]
pub fn combined_marking(
    envelopes: &[gungnir_eventing::Envelope],
) -> (gungnir_model::Releasability, MarkingInputs) {
    use gungnir_model::events::{InterceptEvent, TrackingEvent};
    use gungnir_model::Releasability;
    let mut inputs = MarkingInputs::default();
    let mut markings = Vec::new();
    for env in envelopes {
        let marking = match &env.event {
            Event::Tracking(TrackingEvent::TrackInitiated(t) | TrackingEvent::TrackUpdated(t)) => {
                &t.releasability
            }
            Event::Intercept(InterceptEvent::PlanProposed(p)) => &p.releasability,
            _ => continue,
        };
        match marking {
            Releasability::Internal => inputs.internal += 1,
            Releasability::Parties { .. } => inputs.parties += 1,
            Releasability::AllPeers => inputs.all_peers += 1,
        }
        markings.push(marking.clone());
    }
    (Releasability::combine(markings), inputs)
}

/// Serializable copy of `gungnir_metrics::TrackingMetrics`.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TrackingMetricsSummary {
    pub mota: f64,
    pub motp: f64,
    pub purity: f64,
    pub fragmentation: f64,
}

impl From<TrackingMetrics> for TrackingMetricsSummary {
    fn from(m: TrackingMetrics) -> Self {
        Self {
            mota: m.mota,
            motp: m.motp,
            purity: m.purity,
            fragmentation: m.fragmentation,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EventCounts {
    pub tracks_initiated: u64,
    pub tracks_deleted: u64,
    pub plans_proposed: u64,
    pub plans_approved: u64,
    pub detections_accepted: u64,
    pub detections_quarantined: u64,
    pub decisions: u64,
    /// Pending decisions whose window closed with nobody deciding. Counted apart
    /// from `decisions` because an expiry and a rejection mean different things to
    /// an after-action review (docs/design/DN-10-queue-expiry-and-escalation.md).
    pub decisions_expired: u64,
    /// Items offered upward for want of authority or attention.
    pub decisions_escalated: u64,
    /// Sensor mode changes (GAP-003). Counted because a coverage answer that changed
    /// during a session is the thing an after-action review asks about, and the mode
    /// changes are what changed it.
    pub sensor_mode_changes: u64,
    /// Commands issued to sensors (GAP-004).
    pub sensor_tasks_issued: u64,
    /// Commands a sensor confirmed. The gap between this and `sensor_tasks_issued` is
    /// the interesting figure, which is why the two are counted apart.
    pub sensor_tasks_acknowledged: u64,
    /// Commands a sensor refused, or an adapter could not deliver.
    pub sensor_tasks_failed: u64,
    /// Commands whose acknowledgement window closed with nothing back. Counted apart
    /// from failures because nobody refused these; nobody answered.
    pub sensor_tasks_unacknowledged: u64,
    /// Collection requirements stated during the session (GAP-005).
    pub requirements_stated: u64,
    /// Requirements a sensor manager concurred with tasking.
    pub requirements_tasked: u64,
    /// Requirements answered, each naming its evidence.
    pub requirements_satisfied: u64,
    /// Requirements declined by a sensor manager, with a reason.
    pub requirements_declined: u64,
    /// Requirements whose needed-by time passed with nobody deciding either way.
    /// Counted apart from declines for the reason expiries are counted apart from
    /// rejections: nobody refused these.
    pub requirements_lapsed: u64,
    /// Handovers the incoming watch acknowledged by name (GAP-054, DN-21 §5).
    ///
    /// **This is MOE-13.** There is deliberately no counter for unacknowledged handovers:
    /// an unacknowledged one leaves no event, so it is counted by its absence against the
    /// number of scheduled handover products that came due -- which is
    /// `handover_products_due` below. A counter that guessed at them would report a number
    /// nobody could check.
    pub handovers_acknowledged: u64,
    /// Handover summaries the schedule brought due, whether anyone took them or not.
    pub handover_products_due: u64,
    /// Products with a configured endpoint that nothing carried there.
    ///
    /// Counted because a deployment that believed it was reporting to higher command and
    /// was not is the failure DN-21 §5's delivery rule exists to prevent.
    pub products_undelivered: u64,
    /// Valid detections the tracking pipeline refused (GAP-066).
    ///
    /// Counted apart from `detections_quarantined`, which is the sensor's fault, and apart
    /// from `detections_accepted`, which claims the detection is being tracked. **The
    /// gateway used to count these as accepted**, so an after-action review would have
    /// read an ingest rate that never reached the tracker.
    pub detections_not_accepted: u64,
    /// Promotions and rollbacks made during the session (GAP-086).
    ///
    /// Counted apart from what was in force at the start, because a session that changed
    /// its algorithm configuration part-way through is a different thing to review from
    /// one that ran on the baseline it opened with.
    pub governance_changes: u64,
    /// Promotions that were refused, and why is in the journal beside this.
    pub promotions_refused: u64,
    /// Engagements opened on accepted decisions (GAP-043, DN-06).
    pub engagements_opened: u64,
    /// Outcomes by evidence source, **never summed across the two sources**: a
    /// deployment with no effector reporting must not read track deletions as
    /// confirmed effect (DN-06 §5).
    pub engagements_effective_corroborated: u64,
    pub engagements_effective_track_inferred: u64,
    pub engagements_ineffective_corroborated: u64,
    pub engagements_ineffective_track_inferred: u64,
    /// Windows that closed with nothing observed. "We do not know" is a result.
    pub engagements_indeterminate: u64,
    pub engagements_aborted: u64,
    /// After-action reviews opened (GAP-049, DN-20), and what they recorded. MOE-12
    /// counts reviews and their findings; it does not grade them.
    pub reviews_opened: u64,
    pub findings_recorded: u64,
    /// Findings promoted into the gap register: the loop from an observed failure to a
    /// tracked engineering item, made visible.
    pub findings_promoted: u64,
    /// Maintenance windows that closed with the sensor still down (GAP-054).
    ///
    /// Neither an expected absence nor an ordinary failure; the case somebody planned and
    /// has not finished.
    pub maintenance_overruns: u64,
    pub total: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("export failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("encoding failed: {0}")]
    Encoding(#[from] serde_json::Error),
}

pub trait ReportGenerator: Send + Sync {
    fn generate(&self, session: SessionId) -> Result<MissionReport, ReportError>;
    /// Export with provenance preserved -- the report names the session so every
    /// figure can be recomputed from that journal.
    fn export(&self, report: &MissionReport, path: &Path) -> Result<(), ReportError>;
}

/// Computes a report by folding over the session's journal.
pub struct JournalReportGenerator<'a> {
    pub journal: &'a dyn EventJournal,
    /// Ground-truth-derived metrics, when a scenario with truth was replayed.
    pub metrics: Option<TrackingMetrics>,
}

/// Fold the session's envelopes into the counts a report is built from.
///
/// Split out of `generate` because three gaps' worth of counters (GAP-054's rhythm,
/// GAP-066's refusals, GAP-086's governance) pushed one function past the line limit, and
/// a fold that long stops being readable as the exhaustive list it needs to be.
/// DN-06 §8: the two evidence sources are counted apart, and an outcome string the
/// vocabulary does not know is counted nowhere rather than as either.
fn count_engagement(counts: &mut EventCounts, event: &gungnir_model::events::EngagementEvent) {
    use gungnir_model::events::engagement_outcome as outcome;
    use gungnir_model::events::EngagementEvent;
    match event {
        EngagementEvent::Opened { .. } => counts.engagements_opened += 1,
        EngagementEvent::Executing { .. } => {}
        EngagementEvent::Closed { outcome, .. } => match outcome.as_str() {
            outcome::EFFECTIVE_CORROBORATED => counts.engagements_effective_corroborated += 1,
            outcome::EFFECTIVE_TRACK_INFERRED => {
                counts.engagements_effective_track_inferred += 1;
            }
            outcome::INEFFECTIVE_CORROBORATED => {
                counts.engagements_ineffective_corroborated += 1;
            }
            outcome::INEFFECTIVE_TRACK_INFERRED => {
                counts.engagements_ineffective_track_inferred += 1;
            }
            outcome::INDETERMINATE => counts.engagements_indeterminate += 1,
            outcome::ABORTED => counts.engagements_aborted += 1,
            _ => {}
        },
    }
}

fn count_events(envelopes: &[gungnir_eventing::Envelope]) -> EventCounts {
    let mut counts = EventCounts::default();
    for env in envelopes {
        counts.total += 1;
        match &env.event {
            Event::Tracking(TrackingEvent::TrackInitiated(_)) => counts.tracks_initiated += 1,
            Event::Tracking(TrackingEvent::TrackDeleted(_)) => counts.tracks_deleted += 1,
            Event::Intercept(InterceptEvent::PlanProposed(_)) => counts.plans_proposed += 1,
            Event::Intercept(InterceptEvent::PlanApproved(_)) => counts.plans_approved += 1,
            Event::Ingest(IngestEvent::Accepted(_)) => counts.detections_accepted += 1,
            Event::Ingest(IngestEvent::Quarantined { .. }) => {
                counts.detections_quarantined += 1;
            }
            Event::Ingest(IngestEvent::NotAccepted { .. }) => {
                counts.detections_not_accepted += 1;
            }
            Event::Command(CommandEvent::Decided { .. }) => counts.decisions += 1,
            Event::Command(CommandEvent::Expired { .. }) => counts.decisions_expired += 1,
            Event::Command(CommandEvent::Escalated { .. }) => counts.decisions_escalated += 1,
            Event::Sensor(SensorEvent::ModeChanged { .. }) => {
                counts.sensor_mode_changes += 1;
            }
            Event::SensorTask(SensorTaskEvent::Issued { .. }) => {
                counts.sensor_tasks_issued += 1;
            }
            Event::SensorTask(SensorTaskEvent::Acknowledged { .. }) => {
                counts.sensor_tasks_acknowledged += 1;
            }
            Event::SensorTask(SensorTaskEvent::Failed { .. }) => {
                counts.sensor_tasks_failed += 1;
            }
            Event::SensorTask(SensorTaskEvent::Unacknowledged { .. }) => {
                counts.sensor_tasks_unacknowledged += 1;
            }
            Event::Requirement(RequirementEvent::Stated { .. }) => {
                counts.requirements_stated += 1;
            }
            Event::Requirement(RequirementEvent::Tasked { .. }) => {
                counts.requirements_tasked += 1;
            }
            Event::Requirement(RequirementEvent::Satisfied { .. }) => {
                counts.requirements_satisfied += 1;
            }
            Event::Requirement(RequirementEvent::Declined { .. }) => {
                counts.requirements_declined += 1;
            }
            Event::Requirement(RequirementEvent::Lapsed { .. }) => {
                counts.requirements_lapsed += 1;
            }
            Event::Rhythm(RhythmEvent::HandoverAcknowledged { .. }) => {
                counts.handovers_acknowledged += 1;
            }
            Event::Rhythm(RhythmEvent::ProductDue {
                kind: gungnir_model::ProductKind::HandoverSummary,
                ..
            }) => counts.handover_products_due += 1,
            Event::Rhythm(RhythmEvent::ProductUndelivered { .. }) => {
                counts.products_undelivered += 1;
            }
            Event::Rhythm(RhythmEvent::MaintenanceOverrun { .. }) => {
                counts.maintenance_overruns += 1;
            }
            Event::Governance(
                GovernanceEvent::Promoted { .. } | GovernanceEvent::RolledBack { .. },
            ) => counts.governance_changes += 1,
            Event::Governance(GovernanceEvent::PromotionRefused { .. }) => {
                counts.promotions_refused += 1;
            }
            Event::Review(gungnir_model::events::ReviewEvent::Opened { .. }) => {
                counts.reviews_opened += 1;
            }
            Event::Review(gungnir_model::events::ReviewEvent::FindingRecorded { .. }) => {
                counts.findings_recorded += 1;
            }
            Event::Review(gungnir_model::events::ReviewEvent::FindingPromoted { .. }) => {
                counts.findings_promoted += 1;
            }
            Event::Tracking(_)
            | Event::Intercept(
                InterceptEvent::PlanSuperseded(_) | InterceptEvent::PlanEvaluated { .. },
            )
            | Event::Command(CommandEvent::ApprovalRequested(_))
            | Event::Rhythm(_)
            | Event::Governance(_)
            | Event::Review(_)
            | Event::Health(_)
            | Event::Replay(_)
            | Event::Handoff(_)
            | Event::Warning(_)
            // GAP-009: a peer's launch warning is on the record and is not one of this
            // deployment's own counts. Counting it beside the warnings we owe would
            // read as work this watch did, which is what MOE-01 measures.
            | Event::LaunchWarning(_)
            | Event::Rehearsal(_)
            // What a track was taken to be is not one of this watch's counts: an entity
            // correlated once and seen for six hours would otherwise inflate a figure
            // MOE-01 reads as work done.
            | Event::Identity(_)
            | Event::Link(_) => {}
            Event::Engagement(e) => count_engagement(&mut counts, e),
        }
    }
    counts
}

impl ReportGenerator for JournalReportGenerator<'_> {
    fn generate(&self, session: SessionId) -> Result<MissionReport, ReportError> {
        let envelopes = self.journal.read_session(session)?;
        let counts = count_events(&envelopes);
        let measures = measures::measures(&envelopes);
        let (releasability, marking_inputs) = combined_marking(&envelopes);
        let first_event = envelopes.first().map(|e| e.mission_time);
        let last_event = envelopes.last().map(|e| e.mission_time);
        let duration_s = match (first_event, last_event) {
            (Some(a), Some(b)) => b.seconds_since(a),
            _ => 0.0,
        };
        let summary = format!(
            "Session {}: {} events over {:.1} s; {} tracks initiated, {} deleted; {} plans proposed, {} approved; {} detections accepted, {} quarantined; {} operator decisions.",
            session.0,
            counts.total,
            duration_s,
            counts.tracks_initiated,
            counts.tracks_deleted,
            counts.plans_proposed,
            counts.plans_approved,
            counts.detections_accepted,
            counts.detections_quarantined,
            counts.decisions
        );
        Ok(MissionReport {
            session,
            summary,
            counts,
            measures,
            releasability,
            marking_inputs,
            first_event,
            last_event,
            metrics: self.metrics.map(Into::into),
        })
    }

    fn export(&self, report: &MissionReport, path: &Path) -> Result<(), ReportError> {
        std::fs::write(path, serde_json::to_string_pretty(report)?)?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::cast_precision_loss)]
mod tests {
    use super::*;
    use gungnir_eventing::Envelope;
    use gungnir_model::events::{engagement_outcome, EngagementEvent, HealthEvent, VerdictSummary};
    use gungnir_model::{
        Classification, DecisionId, DetectionView, Measurement, PlanId, PlanView, ProductKind,
        Provenance, Quality, Releasability, SensorId, TrackId, TrackStatus, TrackView,
    };
    use gungnir_store::{DurabilityPolicy, FileEventJournal};

    #[test]
    fn report_counts_are_recomputed_from_the_journal() {
        let root = std::env::temp_dir().join(format!("gungnir-reporting-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut journal = FileEventJournal::open(&root).expect("open");
        let session = SessionId(3);
        let events = [
            Event::Tracking(TrackingEvent::TrackDeleted(TrackId(1))),
            Event::Intercept(InterceptEvent::PlanProposed(PlanView {
                id: PlanId(1),
                ..PlanView::default()
            })),
            Event::Command(CommandEvent::Decided {
                plan: PlanId(1),
                decision: gungnir_model::DecisionId(1),
                accepted: true,
                operator: None,
                role: None,
                verdict: gungnir_model::events::VerdictSummary::RequiresHumanApproval,
                rationale: None,
            }),
        ];
        for (seq, event) in events.into_iter().enumerate() {
            journal
                .append(
                    session,
                    &Envelope {
                        seq: seq as u64,
                        mission_time: MissionTime(seq as f64 * 2.0),
                        event,
                    },
                )
                .expect("append");
        }
        let gen = JournalReportGenerator {
            journal: &journal,
            metrics: None,
        };
        let report = gen.generate(session).expect("generate");
        assert_eq!(report.counts.total, 3);
        assert_eq!(report.counts.tracks_deleted, 1);
        assert_eq!(report.counts.plans_proposed, 1);
        assert_eq!(report.counts.decisions, 1);
        assert_eq!(report.last_event, Some(MissionTime(4.0)));
        let path = root.join("report.json");
        gen.export(&report, &path).expect("export");
        let back: MissionReport =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("parse");
        assert_eq!(back, report);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A marked track with the pipeline's shape of state and covariance, every nonzero float
    /// non-dyadic.
    fn marked_track(
        id: u64,
        classification: Classification,
        releasability: Releasability,
        at: MissionTime,
    ) -> TrackView {
        let mut covariance = [[0.0_f64; 6]; 6];
        for axis in 0..3 {
            covariance[axis][axis] = 25.1;
            covariance[axis][axis + 3] = 0.402_5;
            covariance[axis + 3][axis] = 0.402_5;
            covariance[axis + 3][axis + 3] = 4.05;
        }
        TrackView {
            id: TrackId(id),
            status: TrackStatus::Confirmed,
            state: [
                21_406.337_190_62,
                -3_120.441_278_9,
                152.3,
                -41.17,
                12.903_4,
                0.1 + 0.2,
            ]
            .into(),
            covariance: covariance.into(),
            classification,
            provenance: Provenance {
                source_sensor_ids: vec![3],
                algorithm_version: "cv-ekf 1.4".into(),
                ..Provenance::default()
            },
            quality: Quality {
                association_confidence: 0.87,
                latency_s: 0.042,
                is_stale: false,
            },
            mission_time: at,
            releasability,
        }
    }

    /// A session that fills every part of a report: a detection accepted and one
    /// quarantined, marked tracks and a marked plan, a decision with its rationale, an
    /// engagement of a track later carried as friendly, and scheduled products one of which
    /// went undelivered. Envelope `k` is at 57744.670227102644 + k/3 s, so the time span is
    /// non-dyadic and starts on the double `serde_json` without `float_roundtrip` read back one
    /// ULP off.
    fn traceability_journal() -> Vec<Envelope> {
        let at = |k: u32| MissionTime(57_744.670_227_102_644 + f64::from(k) / 3.0);
        let partners = || Releasability::parties(["partner-a", "partner-b"]);
        let plan = PlanView {
            id: PlanId(1),
            mission_time: at(5),
            policy_value: 2.0 / 3.0,
            releasability: Releasability::parties(["partner-a"]),
            ..PlanView::default()
        };
        let due = |k: u32| {
            Event::Rhythm(RhythmEvent::ProductDue {
                name: "sitrep".into(),
                kind: ProductKind::SituationReport,
                due: at(k),
            })
        };
        let events = vec![
            Event::Health(HealthEvent::Changed {
                tracking_healthy: true,
                intercept_healthy: true,
                ingest_healthy: true,
                at: at(0),
            }),
            Event::Ingest(IngestEvent::Accepted(DetectionView {
                sensor: SensorId(3),
                source_time: MissionTime(at(1).0 - 0.05),
                receipt_time: at(1),
                measurement: Measurement::Position {
                    enu: [21_406.337_190_62, -3_120.441_278_9, 152.3].into(),
                    variance_m2: [25.1, 25.1, 64.3],
                },
                provenance: Provenance::default(),
            })),
            Event::Ingest(IngestEvent::Quarantined {
                sensor: SensorId(9),
                reason: "position not finite".into(),
            }),
            Event::Tracking(TrackingEvent::TrackInitiated(marked_track(
                7,
                Classification::Hostile,
                partners(),
                at(3),
            ))),
            Event::Tracking(TrackingEvent::TrackInitiated(marked_track(
                8,
                Classification::Unknown,
                Releasability::AllPeers,
                at(4),
            ))),
            Event::Intercept(InterceptEvent::PlanProposed(plan.clone())),
            Event::Command(CommandEvent::Decided {
                plan: PlanId(1),
                decision: DecisionId(1),
                accepted: true,
                operator: Some("watch officer".into()),
                role: Some("Supervisor".into()),
                verdict: VerdictSummary::RequiresHumanApproval,
                rationale: Some("inbound on the northern approach".into()),
            }),
            Event::Intercept(InterceptEvent::PlanApproved(plan)),
            Event::Engagement(EngagementEvent::Opened {
                decision: DecisionId(1),
                plan: PlanId(1),
                track: TrackId(7),
            }),
            Event::Tracking(TrackingEvent::TrackUpdated(marked_track(
                7,
                Classification::Friendly,
                partners(),
                at(9),
            ))),
            Event::Engagement(EngagementEvent::Closed {
                decision: DecisionId(1),
                outcome: engagement_outcome::EFFECTIVE_TRACK_INFERRED.into(),
                at: at(10),
            }),
            Event::Tracking(TrackingEvent::TrackDeleted(TrackId(8))),
            due(12),
            due(13),
            due(14),
            Event::Rhythm(RhythmEvent::ProductUndelivered {
                name: "sitrep".into(),
                endpoint: "higher".into(),
                reason: "no delivery path".into(),
                at: at(15),
            }),
        ];
        events
            .into_iter()
            .zip(0_u32..)
            .map(|(event, k)| Envelope {
                seq: u64::from(k),
                mission_time: at(k),
                event,
            })
            .collect()
    }

    /// The `gungnir-reporting` Traceability row of `docs/verification-capability-table.md`
    /// §2 ("Recompute every figure in a report from the journal"; "Exact"), over a
    /// synthetic journal.
    ///
    /// `report_counts_are_recomputed_from_the_journal` reads its export back against the
    /// report it wrote, through the store that wrote it, and checks four counts and the last
    /// event. This exports a report the way the desktop does (a buffered journal, the file
    /// under the journal's directory), drops that store, opens a **fresh** one over the same
    /// directory and regenerates: the regenerated `MissionReport` must equal the exported
    /// file **whole** -- marking, tally, first and last event, summary, and every count and
    /// measure. The journal leaves none of those empty or at a default, and what can be
    /// derived by hand from it is pinned in `assert_by_hand`, so two reports that were wrong
    /// the same way cannot pass by agreeing. `metrics` is `None` on both sides: tracking metrics
    /// come from a scenario's ground truth, not from the journal, and a live session has none.
    #[test]
    fn an_exported_report_is_regenerated_whole_from_the_reopened_journal() {
        let root = std::env::temp_dir().join(format!(
            "gungnir-reporting-traceability-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let session = SessionId(5);
        let journaled = traceability_journal();
        // Where the desktop writes it: `reports/session-<id>.json` under the data directory,
        // which is also the journal's.
        let exported_path = root.join("reports").join("session-5.json");

        let exported = {
            let mut journal =
                FileEventJournal::open_with_policy(&root, DurabilityPolicy::desktop())
                    .expect("open");
            for env in &journaled {
                journal.append(session, env).expect("append");
            }
            let generator = JournalReportGenerator {
                journal: &journal,
                metrics: None,
            };
            let report = generator.generate(session).expect("generate");
            std::fs::create_dir_all(root.join("reports")).expect("report directory");
            generator.export(&report, &exported_path).expect("export");
            // The store drops at the end of this block and syncs what it buffered.
            report
        };

        let reopened = FileEventJournal::open(&root).expect("a fresh store over the directory");
        let regenerated = JournalReportGenerator {
            journal: &reopened,
            metrics: None,
        }
        .generate(session)
        .expect("regenerate");
        let from_file: MissionReport = serde_json::from_str(
            &std::fs::read_to_string(&exported_path).expect("read the export"),
        )
        .expect("parse the export");

        assert_eq!(
            regenerated, from_file,
            "the regenerated report is not the exported file"
        );
        assert_eq!(
            regenerated, exported,
            "the regenerated report is not the report that was exported"
        );
        assert_by_hand(&regenerated, &journaled);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// What `traceability_journal` gives, derived by hand rather than by the fold. Sixteen
    /// envelopes. The marked items are track 7 twice for partners a and b, track 8 for all
    /// peers and the plan for partner a, which DN-17's most-restrictive rule combines to
    /// partner a alone.
    fn assert_by_hand(regenerated: &MissionReport, journaled: &[Envelope]) {
        assert_eq!(
            regenerated.counts,
            EventCounts {
                tracks_initiated: 2,
                tracks_deleted: 1,
                plans_proposed: 1,
                plans_approved: 1,
                detections_accepted: 1,
                detections_quarantined: 1,
                decisions: 1,
                products_undelivered: 1,
                engagements_opened: 1,
                engagements_effective_track_inferred: 1,
                total: 16,
                ..EventCounts::default()
            }
        );
        assert_eq!(
            regenerated.releasability,
            Releasability::parties(["partner-a"])
        );
        assert_eq!(
            regenerated.marking_inputs,
            MarkingInputs {
                internal: 0,
                parties: 3,
                all_peers: 1
            }
        );
        // The span is the journaled times to the bit.
        let bits = |t: Option<MissionTime>| t.map(|t| t.0.to_bits());
        assert_eq!(
            bits(regenerated.first_event),
            bits(journaled.first().map(|e| e.mission_time))
        );
        assert_eq!(
            bits(regenerated.last_event),
            bits(journaled.last().map(|e| e.mission_time))
        );
        // Envelope 15 is 15/3 = 5 s after envelope 0.
        assert_eq!(
            regenerated.summary,
            "Session 5: 16 events over 5.0 s; 2 tracks initiated, 1 deleted; 1 plans \
             proposed, 1 approved; 1 detections accepted, 1 quarantined; 1 operator decisions."
        );
        // MOE-02: track 7 was engaged at envelope 8 and carried as friendly at envelope 9.
        // MOE-13: three products came due and one went undelivered.
        let value = |id: &str| {
            regenerated
                .measures
                .iter()
                .find(|m| m.id == id)
                .map(|m| m.value.clone())
        };
        assert_eq!(value("MOE-02"), Some(MeasureValue::Count(1)));
        assert_eq!(
            value("MOE-13"),
            Some(MeasureValue::Fraction {
                value: 2.0 / 3.0,
                numerator: 2,
                denominator: 3
            })
        );
    }
}

#[cfg(test)]
mod marking_tests {
    use super::*;
    use gungnir_model::events::InterceptEvent;
    use gungnir_model::{MissionTime, PlanId, PlanView, Releasability};

    fn plan(id: u64, marking: Releasability) -> gungnir_eventing::Envelope {
        gungnir_eventing::Envelope {
            seq: id,
            mission_time: MissionTime(0.0),
            event: Event::Intercept(InterceptEvent::PlanProposed(PlanView {
                id: PlanId(id),
                releasability: marking,
                ..PlanView::default()
            })),
        }
    }

    /// **A report containing one restricted item is restricted as a whole** (DN-17 §5
    /// rule 5), and the inputs say what was combined. Tracks fold the same way; the
    /// desktop's report test covers them, since this crate builds no `TrackView`.
    #[test]
    fn one_internal_item_marks_the_whole_report_internal() {
        let journal = vec![
            plan(1, Releasability::AllPeers),
            plan(2, Releasability::parties(["partner-a"])),
            plan(3, Releasability::Internal),
        ];
        let (marking, inputs) = combined_marking(&journal);
        assert_eq!(marking, Releasability::Internal);
        assert_eq!(
            inputs,
            MarkingInputs {
                internal: 1,
                parties: 1,
                all_peers: 1
            }
        );
        let (marking, _) = combined_marking(&journal[..2]);
        assert_eq!(marking, Releasability::parties(["partner-a"]));
    }

    /// Nothing marked journaled: internal by default, with a zero tally that says so.
    #[test]
    fn an_empty_journal_is_internal_with_no_inputs() {
        let (marking, inputs) = combined_marking(&[]);
        assert_eq!(marking, Releasability::Internal);
        assert_eq!(inputs.total(), 0);
    }

    /// The exported form carries the marking inside the file.
    #[test]
    fn the_serialised_report_carries_its_marking() {
        let report = MissionReport {
            session: gungnir_model::SessionId(1),
            summary: String::new(),
            counts: EventCounts::default(),
            measures: Vec::new(),
            releasability: Releasability::parties(["partner-a"]),
            marking_inputs: MarkingInputs {
                internal: 0,
                parties: 2,
                all_peers: 0,
            },
            first_event: None,
            last_event: None,
            metrics: None,
        };
        let json = serde_json::to_string(&report).expect("serialises");
        assert!(json.contains("\"releasability\""), "{json}");
        assert!(json.contains("partner-a"), "{json}");
    }
}

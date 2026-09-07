// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The measures catalogue, computed from the journal (GAP-047, `docs/mission/measures.md`).
//!
//! **Every figure is a fold over the envelopes and carries its basis**, so an analyst can
//! recompute it from the same journal (the traceability row in
//! `docs/verification-capability-table.md` §2). **Every figure the journal cannot answer
//! says why**, naming what is missing and the entry that owns it -- `measures.md` §3 puts
//! MOE-01 to MOE-04 and MOE-07 to MOE-09 behind ground truth, and this module does not
//! quietly leave those rows out. A catalogue with only the easy rows in it reads as a
//! catalogue with nothing wrong.
//!
//! The performance measures (MOP-xx) are not here: `measures.md` §3 puts them on
//! `tracing` spans, the verification table and the benchmark harnesses, none of which is
//! the journal.

use gungnir_eventing::{Envelope, Event};
use gungnir_model::events::{
    CommandEvent, EngagementEvent, HealthEvent, ReplayEvent, ReviewEvent, RhythmEvent,
    TrackingEvent,
};
use gungnir_model::{Classification, DecisionId, TrackId};
use std::collections::{HashMap, HashSet};

/// One row of the catalogue, as computed for a session.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Measure {
    pub id: String,
    pub name: String,
    /// The agreed target, in the catalogue's words (D-16).
    pub target: String,
    pub value: MeasureValue,
    /// What the figure rests on when the number alone would mislead: a basis the
    /// definition does not make obvious, or the part of the definition the record
    /// cannot yet see.
    pub note: Option<String>,
}

/// What the fold produced, with its basis.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum MeasureValue {
    /// `numerator / denominator`, both kept so the figure can be checked.
    Fraction {
        value: f64,
        numerator: u64,
        denominator: u64,
    },
    Count(u64),
    /// The journal holds nothing the measure applies to. Not zero and not a pass: a
    /// session with no engagements has a fratricide count of nothing, not of none.
    NoInstances {
        of: String,
    },
    /// The journal cannot answer this one, and this is what it lacks.
    NotComputable {
        reason: String,
    },
}

fn measure(id: &str, name: &str, target: &str, value: MeasureValue) -> Measure {
    Measure {
        id: id.into(),
        name: name.into(),
        target: target.into(),
        value,
        note: None,
    }
}

fn noted(mut m: Measure, note: String) -> Measure {
    m.note = Some(note);
    m
}

fn fraction(numerator: u64, denominator: u64, of: &str) -> MeasureValue {
    if denominator == 0 {
        return MeasureValue::NoInstances { of: of.into() };
    }
    #[allow(clippy::cast_precision_loss)]
    let value = numerator as f64 / denominator as f64;
    MeasureValue::Fraction {
        value,
        numerator,
        denominator,
    }
}

fn not_computable(reason: &str) -> MeasureValue {
    MeasureValue::NotComputable {
        reason: reason.into(),
    }
}

/// The catalogue for one session's journal, in catalogue order.
#[must_use]
pub fn measures(envelopes: &[Envelope]) -> Vec<Measure> {
    let truth = "needs the scenario's ground truth beside the journal (GAP-045); a live \
                 session has only the analyst's reconstruction";
    vec![
        measure(
            "MOE-01",
            "Defended-asset protection",
            "0.95 for priority-1 and priority-2 assets",
            not_computable(truth),
        ),
        measure(
            "MOE-02",
            "No fratricide, no civil engagement",
            "0",
            moe_02(envelopes),
        ),
        measure(
            "MOE-03",
            "Cost discipline",
            "at least 0.9",
            not_computable(
                "a track carries no threat class finer than Hostile, so a propeller-drone \
                 engagement cannot be told from any other (GAP-018)",
            ),
        ),
        measure(
            "MOE-04",
            "Decision timeliness",
            "p95 under 0.3",
            not_computable(
                "needs each engaged track's remaining time to impact, which is prediction \
                 (GAP-020); the decision latency alone is not the measure",
            ),
        ),
        {
            let (value, note) = moe_05(envelopes);
            let m = measure("MOE-05", "Decision completeness", "1.0", value);
            note.map_or_else(|| m.clone(), |n| noted(m.clone(), n))
        },
        {
            let (value, note) = moe_06(envelopes);
            noted(measure("MOE-06", "Picture honesty", "0", value), note)
        },
        measure(
            "MOE-07",
            "Surface picture completeness",
            "0.98",
            not_computable(truth),
        ),
        measure(
            "MOE-08",
            "Fires timeliness",
            "0.9",
            not_computable(
                "a fires task carries no decision window on the bus, so 'within the \
                 target's window' has nothing to be measured against (DN-05)",
            ),
        ),
        measure(
            "MOE-09",
            "Identity continuity",
            "0.9",
            not_computable(truth),
        ),
        measure(
            "MOE-10",
            "Degradation recovery",
            "30 s to show; battle rhythm to accept",
            not_computable(
                "nothing publishes when coverage was recomputed and shown, so the time \
                 from sensor loss to the map changing is not in the record",
            ),
        ),
        measure(
            "MOE-11",
            "Continuity under disconnection",
            "1.0 reached; 1.0 resolved",
            not_computable(
                "no reconciliation events exist: the failover and reconciliation gate is \
                 GAP-050",
            ),
        ),
        {
            let (value, note) = moe_12(envelopes);
            noted(
                measure(
                    "MOE-12",
                    "Rehearsal effect",
                    "rehearse every plan change; ratio observed without a target",
                    value,
                ),
                note,
            )
        },
        measure(
            "MOE-13",
            "Intelligence product timeliness",
            "0.95 of scheduled",
            moe_13(envelopes),
        ),
    ]
}

/// MOE-02: engagements whose track was later carried as friendly or neutral.
///
/// "Later shown" is read from the picture itself: a `TrackUpdated` after the engagement
/// opened with the classification changed. Ground truth would be stronger evidence and
/// is not required by the definition.
fn moe_02(envelopes: &[Envelope]) -> MeasureValue {
    // Engaged tracks by the sequence the engagement opened at.
    let mut engaged: HashMap<TrackId, Vec<u64>> = HashMap::new();
    let mut engagements = 0_u64;
    for env in envelopes {
        if let Event::Engagement(EngagementEvent::Opened { track, .. }) = &env.event {
            engaged.entry(*track).or_default().push(env.seq);
            engagements += 1;
        }
    }
    if engagements == 0 {
        return MeasureValue::NoInstances {
            of: "engagements".into(),
        };
    }
    let mut count = 0_u64;
    for env in envelopes {
        let Event::Tracking(TrackingEvent::TrackUpdated(view)) = &env.event else {
            continue;
        };
        if !matches!(
            view.classification,
            Classification::Friendly | Classification::Neutral
        ) {
            continue;
        }
        if let Some(opened) = engaged.get_mut(&view.id) {
            // Each engagement counts once, at the first later reclassification.
            let before = opened.len();
            opened.retain(|seq| *seq > env.seq);
            count += (before - opened.len()) as u64;
        }
    }
    MeasureValue::Count(count)
}

/// MOE-05: the fraction of engagements whose decision is on the record with a verdict
/// and a rationale.
///
/// Every engagement opens from a decision, and `Decided` now carries the verdict; what
/// can be missing is the rationale, and today it always is for an acceptance -- the
/// record holds the operator's reason only on a rejection, and the course-of-action
/// rationale reaches the record with GAP-032. The fraction says so rather than counting
/// the two parts that always pass.
fn moe_05(envelopes: &[Envelope]) -> (MeasureValue, Option<String>) {
    let mut decided: HashMap<DecisionId, bool> = HashMap::new();
    for env in envelopes {
        if let Event::Command(CommandEvent::Decided {
            decision,
            rationale,
            ..
        }) = &env.event
        {
            decided.insert(*decision, rationale.is_some());
        }
    }
    let mut opened = 0_u64;
    let mut complete = 0_u64;
    let mut no_record = 0_u64;
    for env in envelopes {
        if let Event::Engagement(EngagementEvent::Opened { decision, .. }) = &env.event {
            opened += 1;
            match decided.get(decision) {
                Some(true) => complete += 1,
                Some(false) => {}
                None => no_record += 1,
            }
        }
    }
    if opened == 0 {
        return (
            MeasureValue::NoInstances {
                of: "engagements".into(),
            },
            None,
        );
    }
    let note = if complete < opened {
        Some(format!(
            "{} of {opened} engagements lack a rationale on the record: an acceptance \
             records none until the course of action reaches the record (GAP-032); {no_record} \
             had no decision record at all",
            opened - complete
        ))
    } else {
        None
    };
    (fraction(complete, opened, "engagements"), note)
}

/// MOE-06: decisions taken on a backend whose degraded state was not shown.
///
/// The record says when health changed (`HealthEvent`), and PN-01 draws the flags every
/// frame, so a decision taken after a degradation was journaled was taken with it shown.
/// The violation this counts is a decision taken **with no health on the record at all**
/// -- nothing says what the picture showed -- which is the conservative reading; a
/// decision under a journaled degradation is counted in the note, not the figure.
fn moe_06(envelopes: &[Envelope]) -> (MeasureValue, String) {
    let mut latest: Option<bool> = None;
    let mut decisions = 0_u64;
    let mut under_degraded = 0_u64;
    let mut unrecorded = 0_u64;
    for env in envelopes {
        match &env.event {
            Event::Health(h @ HealthEvent::Changed { .. }) => latest = Some(h.is_degraded()),
            Event::Command(CommandEvent::Decided { .. }) => {
                decisions += 1;
                match latest {
                    None => unrecorded += 1,
                    Some(true) => under_degraded += 1,
                    Some(false) => {}
                }
            }
            _ => {}
        }
    }
    let note = format!(
        "{decisions} decisions; {under_degraded} taken under a journaled degradation, with \
         the state on the record (and on PN-01) before the decision; {unrecorded} taken \
         before any health was on the record"
    );
    (MeasureValue::Count(unrecorded), note)
}

/// MOE-12: the fraction of shifts that rehearsed, and findings in rehearsal against
/// findings in action.
///
/// A shift is a handover period (`HandoverAcknowledged` carries it); a rehearsal is a
/// replay opened inside it. A finding recorded under replay carries the moment it refers
/// to; one recorded live does not, and that is the split the catalogue observes without a
/// target.
fn moe_12(envelopes: &[Envelope]) -> (MeasureValue, String) {
    let mut periods = Vec::new();
    let mut rehearsals = Vec::new();
    let mut in_rehearsal = 0_u64;
    let mut in_action = 0_u64;
    for env in envelopes {
        match &env.event {
            Event::Rhythm(RhythmEvent::HandoverAcknowledged { period, .. }) => {
                periods.push(*period);
            }
            Event::Replay(ReplayEvent::Opened { .. }) => rehearsals.push(env.mission_time),
            Event::Review(ReviewEvent::FindingRecorded { refers_to, .. }) => {
                if refers_to.is_some() {
                    in_rehearsal += 1;
                } else {
                    in_action += 1;
                }
            }
            _ => {}
        }
    }
    let note = format!(
        "{} rehearsals; findings: {in_rehearsal} recorded under replay, {in_action} in action",
        rehearsals.len()
    );
    if periods.is_empty() {
        return (
            MeasureValue::NoInstances {
                of: "shifts (no handover acknowledged)".into(),
            },
            note,
        );
    }
    let rehearsed: HashSet<usize> = periods
        .iter()
        .enumerate()
        .filter(|(_, (start, end))| rehearsals.iter().any(|t| *t >= *start && *t <= *end))
        .map(|(i, _)| i)
        .collect();
    let shifts = u64::try_from(periods.len()).unwrap_or(u64::MAX);
    let with = u64::try_from(rehearsed.len()).unwrap_or(u64::MAX);
    (fraction(with, shifts, "shifts"), note)
}

/// MOE-13: scheduled products delivered within the rhythm.
///
/// A product came due (`ProductDue`) and either went out, was held for a person because
/// no endpoint is configured (on time, and a configuration rather than a failure, DN-21
/// §5), or was recorded undelivered. Undelivered is the only miss.
fn moe_13(envelopes: &[Envelope]) -> MeasureValue {
    let mut due = 0_u64;
    let mut undelivered = 0_u64;
    for env in envelopes {
        match &env.event {
            Event::Rhythm(RhythmEvent::ProductDue { .. }) => due += 1,
            Event::Rhythm(RhythmEvent::ProductUndelivered { .. }) => undelivered += 1,
            _ => {}
        }
    }
    fraction(due.saturating_sub(undelivered), due, "scheduled products")
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::{MissionTime, ProductKind};

    fn env(seq: u32, event: Event) -> Envelope {
        Envelope {
            seq: u64::from(seq),
            mission_time: MissionTime(f64::from(seq)),
            event,
        }
    }

    fn due(seq: u32) -> Envelope {
        env(
            seq,
            Event::Rhythm(RhythmEvent::ProductDue {
                name: "sitrep".into(),
                kind: ProductKind::SituationReport,
                due: MissionTime(f64::from(seq)),
            }),
        )
    }

    /// Every row of the catalogue is present whether or not it can be computed, and in
    /// the catalogue's order: a report that dropped the rows it could not answer would
    /// read as a catalogue with nothing wrong.
    #[test]
    fn every_moe_row_is_present_in_order() {
        let ids: Vec<String> = measures(&[]).into_iter().map(|m| m.id).collect();
        let expected: Vec<String> = (1..=13).map(|n| format!("MOE-{n:02}")).collect();
        assert_eq!(ids, expected);
    }

    /// An empty journal has no instances, which is neither a pass nor a zero.
    #[test]
    fn no_instances_is_not_zero() {
        let all = measures(&[]);
        let moe_02 = all.iter().find(|m| m.id == "MOE-02").expect("row");
        assert!(matches!(moe_02.value, MeasureValue::NoInstances { .. }));
        let moe_13 = all.iter().find(|m| m.id == "MOE-13").expect("row");
        assert!(matches!(moe_13.value, MeasureValue::NoInstances { .. }));
    }

    #[test]
    fn moe_13_counts_an_undelivered_product_as_the_only_miss() {
        let journal = vec![
            due(1),
            due(2),
            env(
                3,
                Event::Rhythm(RhythmEvent::ProductHeld {
                    name: "sitrep".into(),
                    at: MissionTime(3.0),
                }),
            ),
            due(4),
            env(
                5,
                Event::Rhythm(RhythmEvent::ProductUndelivered {
                    name: "sitrep".into(),
                    endpoint: "higher".into(),
                    reason: "no delivery path".into(),
                    at: MissionTime(5.0),
                }),
            ),
        ];
        let value = moe_13(&journal);
        assert_eq!(
            value,
            MeasureValue::Fraction {
                value: 2.0 / 3.0,
                numerator: 2,
                denominator: 3
            }
        );
    }

    fn decided(seq: u32, decision: u64, rationale: Option<&str>) -> Envelope {
        use gungnir_model::events::VerdictSummary;
        use gungnir_model::PlanId;
        env(
            seq,
            Event::Command(CommandEvent::Decided {
                plan: PlanId(decision),
                decision: DecisionId(decision),
                accepted: true,
                operator: None,
                verdict: VerdictSummary::RequiresHumanApproval,
                rationale: rationale.map(str::to_string),
            }),
        )
    }

    fn opened(seq: u32, decision: u64) -> Envelope {
        use gungnir_model::PlanId;
        env(
            seq,
            Event::Engagement(EngagementEvent::Opened {
                decision: DecisionId(decision),
                plan: PlanId(decision),
                track: TrackId(decision),
            }),
        )
    }

    /// MOE-05 counts the rationale's absence rather than passing on the parts that
    /// always hold, and says why it is absent.
    #[test]
    fn moe_05_counts_the_missing_rationale() {
        let journal = vec![
            decided(1, 1, None),
            opened(2, 1),
            decided(3, 2, Some("second engagement of the same track")),
            opened(4, 2),
        ];
        let (value, note) = moe_05(&journal);
        assert_eq!(
            value,
            MeasureValue::Fraction {
                value: 0.5,
                numerator: 1,
                denominator: 2
            }
        );
        assert!(note.is_some_and(|n| n.contains("GAP-032")));
    }

    /// MOE-06: a decision with no health on the record counts; one under a journaled
    /// degradation is in the note, because the strip had it shown.
    #[test]
    fn moe_06_counts_decisions_with_no_health_on_the_record() {
        let health = |seq: u32, ok: bool| {
            env(
                seq,
                Event::Health(HealthEvent::Changed {
                    tracking_healthy: ok,
                    intercept_healthy: true,
                    ingest_healthy: true,
                    at: MissionTime(f64::from(seq)),
                }),
            )
        };
        let journal = vec![
            decided(1, 1, None),
            health(2, false),
            decided(3, 2, None),
            health(4, true),
            decided(5, 3, None),
        ];
        let (value, note) = moe_06(&journal);
        assert_eq!(value, MeasureValue::Count(1));
        assert!(
            note.contains("1 taken under a journaled degradation"),
            "{note}"
        );
        assert!(note.contains("1 taken before any health"), "{note}");
    }

    /// MOE-12: a shift rehearsed is a handover period with a replay opened inside it.
    #[test]
    fn moe_12_counts_shifts_that_rehearsed() {
        use gungnir_model::SessionId;
        let handover = |seq: u32, start: f64, end: f64| {
            env(
                seq,
                Event::Rhythm(RhythmEvent::HandoverAcknowledged {
                    by: "watch".into(),
                    period: (MissionTime(start), MissionTime(end)),
                    outstanding: false,
                    at: MissionTime(end),
                }),
            )
        };
        let journal = vec![
            env(
                5,
                Event::Replay(ReplayEvent::Opened {
                    session: SessionId(1),
                    at: MissionTime(5.0),
                }),
            ),
            handover(10, 0.0, 10.0),
            handover(20, 10.0, 20.0),
        ];
        let (value, note) = moe_12(&journal);
        assert_eq!(
            value,
            MeasureValue::Fraction {
                value: 0.5,
                numerator: 1,
                denominator: 2
            }
        );
        assert!(note.contains("1 rehearsals"), "{note}");
        assert!(matches!(moe_12(&[]).0, MeasureValue::NoInstances { .. }));
    }
}

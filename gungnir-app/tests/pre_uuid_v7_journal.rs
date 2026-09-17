// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! DN-31 §9 row 1, its last clause (GAP-130; D-56, D-60): "a pre-change journal replays and
//! reports unchanged".
//!
//! `testdata/journals/pre-uuid-v7/` holds a desktop session written before the identifiers
//! changed, whose plans and decision are the old `u64` counters as JSON numbers, and the
//! report `JournalReportGenerator` exported from it then (its `SOURCE.md` says how). This
//! reads that journal with today's code: it replays through `gungnir-replay`, its
//! identifiers read as the numbers that were written, and the report regenerated through
//! `gungnir-reporting` equals the committed one, as a value and byte for byte. Here rather
//! than in either crate because it needs both, and `gungnir-app` is the crate that already
//! depends on both.

use gungnir_eventing::{Envelope, Event};
use gungnir_model::events::{CommandEvent, EngagementEvent, HandoffEvent, InterceptEvent};
use gungnir_model::{DecisionId, MissionTime, PlanId};
use gungnir_replay::ReplaySession;
use gungnir_reporting::{JournalReportGenerator, MissionReport, ReportGenerator};
use gungnir_store::{journal, EventJournal, FileEventJournal, SessionId};
use std::path::{Path, PathBuf};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata/journals/pre-uuid-v7")
}

/// Every identifier the journal names, in the order it names them.
fn identifiers(envelopes: &[Envelope]) -> Vec<String> {
    envelopes
        .iter()
        .filter_map(|e| match &e.event {
            Event::Intercept(InterceptEvent::PlanProposed(p)) => Some(format!("proposed {}", p.id)),
            Event::Intercept(InterceptEvent::PlanEvaluated { plan, .. }) => {
                Some(format!("evaluated {plan}"))
            }
            Event::Command(CommandEvent::Decided { plan, decision, .. }) => {
                Some(format!("decided {plan} as {decision}"))
            }
            Event::Command(CommandEvent::Escalated { plan, .. }) => {
                Some(format!("escalated {plan}"))
            }
            Event::Command(CommandEvent::Expired { plan, .. }) => Some(format!("expired {plan}")),
            Event::Engagement(EngagementEvent::Opened { decision, plan, .. }) => {
                Some(format!("opened {decision} for {plan}"))
            }
            Event::Engagement(EngagementEvent::Closed { decision, .. }) => {
                Some(format!("closed {decision}"))
            }
            Event::Handoff(HandoffEvent::Issued { decision, .. }) => {
                Some(format!("issued {decision}"))
            }
            Event::Handoff(HandoffEvent::Undelivered { decision, .. }) => {
                Some(format!("undelivered {decision}"))
            }
            Event::Handoff(HandoffEvent::Reported { decision, .. }) => {
                Some(format!("reported {decision}"))
            }
            _ => None,
        })
        .collect()
}

/// **DN-31 §9 row 1**: the journal written before GAP-130 replays whole, two replays agree
/// with each other and with the record, every identifier reads as the number that was
/// written, and the report regenerated from it is the report exported from it then.
#[test]
fn a_journal_written_before_gap_130_replays_and_reports_unchanged() {
    let journal = FileEventJournal::open(fixture()).expect("the fixture opens");
    let session = SessionId(1);
    let recorded = journal.read_session(session).expect("the journal reads");
    assert_eq!(recorded.len(), 16, "the fixture is 16 envelopes");

    let replay = || {
        let mut playback = ReplaySession::open(&journal, session).expect("the replay opens");
        let stepped: Vec<Envelope> = std::iter::from_fn(|| playback.step().cloned()).collect();
        (stepped, playback.clock().now())
    };
    let (first, clock) = replay();
    let (second, _) = replay();
    assert_eq!(first, second, "two replays of one journal differ");
    assert_eq!(first, recorded, "the replay is not what was journaled");
    assert_eq!(clock, MissionTime(101.0));

    assert_eq!(
        identifiers(&first),
        [
            "proposed 1",
            "evaluated 1",
            "decided 1 as 1",
            "issued 1",
            "undelivered 1",
            "opened 1 for 1",
            "reported 1",
            "proposed 2",
            "evaluated 2",
            "closed 1",
            "proposed 3",
            "evaluated 3",
            "escalated 3",
            "expired 3",
        ],
        "an identifier did not read as the counter that wrote it"
    );

    let generator = JournalReportGenerator {
        journal: &journal,
        metrics: None,
    };
    let regenerated = generator.generate(session).expect("the report generates");
    let committed_text =
        std::fs::read_to_string(fixture().join("report.json")).expect("the committed report");
    let committed: MissionReport =
        serde_json::from_str(&committed_text).expect("the committed report reads");
    assert_eq!(
        regenerated, committed,
        "the report regenerated from the journal differs from the one it produced then"
    );
    let scratch = std::env::temp_dir().join(format!(
        "gungnir-pre-uuid-v7-report-{}.json",
        std::process::id()
    ));
    generator
        .export(&regenerated, &scratch)
        .expect("the report exports");
    let exported = std::fs::read_to_string(&scratch).expect("the export reads");
    let _ = std::fs::remove_file(&scratch);
    assert_eq!(
        exported, committed_text,
        "the regenerated export is not byte for byte the committed one"
    );
}

/// A line written before GAP-130 reads, and what it reads writes in the new form (D-60):
/// every identifier becomes the hyphenated string, no identifier number is left, and the
/// new line reads back to the same envelope. `PlanId(1)` and `DecisionId(1)` are the values
/// the old counters gave, so the new form of one is its value as a UUID.
#[test]
fn every_pre_change_line_reads_and_rewrites_in_the_new_form_without_loss() {
    let journal = FileEventJournal::open(fixture()).expect("the fixture opens");
    let recorded = journal
        .read_session(SessionId(1))
        .expect("the journal reads");
    for envelope in &recorded {
        let line = journal::encode_line(envelope).expect("encodes");
        for field in ["\"plan\":", "\"decision\":", "\"id\":"] {
            for (at, _) in line.match_indices(field) {
                let value = &line[at + field.len()..];
                assert!(
                    !value.starts_with(|c: char| c.is_ascii_digit()),
                    "an identifier was written as a number: {line}"
                );
            }
        }
        let back = journal::decode_line(&line).expect("the new form reads");
        assert_eq!(&back, envelope);
    }
    let decided = recorded
        .iter()
        .find_map(|e| match &e.event {
            Event::Command(CommandEvent::Decided { plan, decision, .. }) => {
                Some((*plan, *decision))
            }
            _ => None,
        })
        .expect("the fixture holds a decision");
    assert_eq!(decided, (PlanId(1), DecisionId(1)));
    let written = serde_json::to_string(&decided.1).expect("encodes");
    assert_eq!(written, "\"00000000-0000-0000-0000-000000000001\"");
}

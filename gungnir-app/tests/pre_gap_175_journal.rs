// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! GAP-175, D-101: a global entity identity is written as RFC 9562 text, and a journal
//! that wrote it as a 128-bit JSON number still reads, to the exact identity.
//!
//! `testdata/journals/pre-gap-175/` holds two desktop sessions written by the code before
//! the change (its `SOURCE.md` says how): one object minted in session 1 and joined again
//! after a restart in session 2, and a second object minted in session 2. Each identity is
//! a UUID v7 written as a JSON number that a double cannot hold. Here in `gungnir-app`
//! because the last test starts a desktop over those files, which is the reader that
//! matters: the identities a restart restores.

use gungnir_app::state::AppState;
use gungnir_config::ConfigBaseline;
use gungnir_eventing::{Envelope, Event};
use gungnir_model::events::IdentityEvent;
use gungnir_model::identity::GlobalEntityId;
use gungnir_model::TrackId;
use gungnir_store::{journal, EventJournal, FileEventJournal, SessionId};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata/journals/pre-gap-175")
}

/// Every identity number the fixture's raw lines hold, read from the digits: the values
/// the test holds the reader to, found without going through any reader under test.
fn written_numbers() -> Vec<u128> {
    let mut found = Vec::new();
    for session in ["session-000000000001.jsonl", "session-000000000002.jsonl"] {
        let text = std::fs::read_to_string(fixture().join(session)).expect("the fixture");
        for (at, _) in text.match_indices("\"entity\":") {
            let digits: String = text[at + "\"entity\":".len()..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            found.push(digits.parse::<u128>().expect("a number, as it was written"));
        }
    }
    found
}

/// The identity events of a session, in order: track and entity.
fn identities(envelopes: &[Envelope]) -> Vec<(TrackId, GlobalEntityId)> {
    envelopes
        .iter()
        .filter_map(|e| match &e.event {
            Event::Identity(
                IdentityEvent::Minted { track, entity, .. }
                | IdentityEvent::Correlated { track, entity, .. },
            ) => Some((*track, *entity)),
            _ => None,
        })
        .collect()
}

/// **The fixture is what it claims to be**: three identities written as numbers, each
/// wider than 64 bits and each changed by a double, so a reader that went through a float
/// could not pass the tests below.
#[test]
fn the_fixture_holds_identities_a_double_cannot_carry() {
    let numbers = written_numbers();
    assert_eq!(numbers.len(), 3, "{numbers:?}");
    for n in numbers {
        assert!(n > u128::from(u64::MAX), "{n} fits in 64 bits");
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        let through_a_double = (n as f64) as u128;
        assert_ne!(through_a_double, n, "{n} survives a double");
    }
}

/// **Every old line reads to the exact identity it wrote**, and what it reads is written in
/// the new form: text, no wide number left, reading back to the same envelope, and now
/// accepted by `serde_json::to_value`, which refused the old form outright.
#[test]
fn a_journal_written_before_gap_175_reads_to_the_exact_identities() {
    let journal = FileEventJournal::open(fixture()).expect("the fixture opens");
    let first = journal.read_session(SessionId(1)).expect("session 1 reads");
    let second = journal.read_session(SessionId(2)).expect("session 2 reads");
    assert_eq!((first.len(), second.len()), (4, 6));

    let numbers = written_numbers();
    let read: Vec<(TrackId, GlobalEntityId)> = identities(&first)
        .into_iter()
        .chain(identities(&second))
        .collect();
    assert_eq!(
        read,
        vec![
            (TrackId(1), GlobalEntityId(numbers[0])),
            (TrackId(0), GlobalEntityId(numbers[1])),
            (TrackId(2), GlobalEntityId(numbers[2])),
        ],
        "an identity did not read as the number that wrote it"
    );
    assert_eq!(
        numbers[0], numbers[1],
        "the restart's join names the same entity"
    );
    assert_eq!(
        read[0].1.to_string(),
        "01a0df82-6bb1-75d1-9e86-4109de36bbde"
    );

    for envelope in first.iter().chain(&second) {
        let line = journal::encode_line(envelope).expect("encodes");
        if let Event::Identity(event) = &envelope.event {
            let entity = match event {
                IdentityEvent::Minted { entity, .. } | IdentityEvent::Correlated { entity, .. } => {
                    entity
                }
            };
            assert!(
                line.contains(&format!("\"entity\":\"{entity}\"")),
                "the identity was not written as its text: {line}"
            );
            let value = serde_json::to_value(event).expect("to_value accepts the event now");
            assert_eq!(
                serde_json::from_value::<IdentityEvent>(value).expect("and reads it back"),
                *event
            );
        }
        assert!(
            !written_numbers()
                .iter()
                .any(|n| line.contains(&n.to_string())),
            "a wide number is still written: {line}"
        );
        assert_eq!(
            &journal::decode_line(&line).expect("the new form reads"),
            envelope
        );
    }
}

/// **A desktop restarted over the old journal keeps its entities** (CAP-2.7, GAP-123's
/// continuity): it restores both identities exactly, reading nothing it cannot read.
#[test]
fn a_desktop_restarted_over_the_old_journal_restores_the_same_entities() {
    let dir = std::env::temp_dir().join(format!("gungnir-pre-gap-175-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    for session in ["session-000000000001.jsonl", "session-000000000002.jsonl"] {
        std::fs::copy(fixture().join(session), dir.join(session)).expect("copied");
    }
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    };
    let state = AppState::with_config(config).expect("the desktop starts");
    assert!(
        state.identity.unreadable.is_none(),
        "{:?}",
        state.identity.unreadable
    );
    let restored: BTreeSet<GlobalEntityId> = state
        .identity
        .resolver
        .lineages()
        .map(|l| l.global_id)
        .collect();
    let numbers = written_numbers();
    assert_eq!(
        restored,
        [GlobalEntityId(numbers[0]), GlobalEntityId(numbers[2])]
            .into_iter()
            .collect(),
        "the restart restored different entities from the ones the journal names"
    );
    drop(state);
    let _ = std::fs::remove_dir_all(dir);
}

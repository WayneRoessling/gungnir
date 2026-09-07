// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Journal encryption at rest, against the real cipher (GAP-060, DN-22).
//!
//! `gungnir-store` declares [`JournalSealer`] and knows nothing about keys;
//! `gungnir-security` owns custody and knows nothing about journals. **The binary wires
//! them together**, which is the arrangement DN-22 §4 describes and the reason it adds no
//! dependency edge. This test lives in the node because the node is where that wiring
//! belongs, and because nothing else can see both halves.
//!
//! The property worth protecting: **a written journal must not contain the plaintext.**
//! Every other assertion here is about not losing the ability to read it back.

use gungnir_eventing::{Envelope, Event};
use gungnir_model::events::SensorEvent;
use gungnir_model::{MissionTime, SensorId, SensorMode};
use gungnir_security::{InProcessKeyProvider, KeyId, KeyProvider, KeyPurpose};
use gungnir_store::sealing::JournalSealer;
use gungnir_store::{EventJournal, FileEventJournal, SessionId, StoreError};
use std::sync::Arc;

/// The binary's adapter: a `KeyProvider` seen through the journal's narrow trait.
///
/// This is the whole of the wiring DN-22 §4 anticipates. It carries the `KeyId` so the
/// provider seals under the active key; the sealed bytes then carry it themselves, which
/// is what lets a rotation leave old lines readable (amendment 1 b).
struct ProviderSealer {
    provider: Arc<InProcessKeyProvider>,
    key: KeyId,
}

impl JournalSealer for ProviderSealer {
    fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, StoreError> {
        self.provider
            .seal(&self.key, plaintext)
            .map_err(|e| StoreError::Sealing(e.to_string()))
    }

    fn unseal(&self, sealed: &[u8]) -> Result<Vec<u8>, StoreError> {
        self.provider
            .unseal(&self.key, sealed)
            .map_err(|e| StoreError::Sealing(e.to_string()))
    }
}

fn envelope(seq: u64) -> Envelope {
    Envelope {
        seq,
        mission_time: MissionTime(1.0),
        event: Event::Sensor(SensorEvent::ModeChanged {
            sensor: SensorId(1),
            from: SensorMode::Standby,
            to: SensorMode::Search,
            at: MissionTime(1.0),
        }),
    }
}

/// A temporary directory unique to this test *process*, not just to this test.
///
/// The name carries the process id. Without it two concurrent `cargo test` runs -- which
/// happen whenever more than one person or agent is running the gate, and which is how
/// this was found -- name the same directory, and the second run's `remove_dir_all`
/// deletes the first run's journal underneath it. The failure surfaces as `Io(NotFound)`
/// from an unrelated line and does not reproduce in isolation, which is the worst kind of
/// flake to chase.
fn dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("gungnir-at-rest-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Everything written under the data directory, as one string.
fn on_disk(root: &std::path::Path) -> String {
    let mut all = String::new();
    for entry in std::fs::read_dir(root).expect("readable") {
        let path = entry.expect("entry").path();
        if path.is_file() {
            all.push_str(&std::fs::read_to_string(&path).unwrap_or_default());
        }
    }
    all
}

fn sealer(provider: &Arc<InProcessKeyProvider>, key: KeyId) -> Box<dyn JournalSealer> {
    Box::new(ProviderSealer {
        provider: Arc::clone(provider),
        key,
    })
}

/// **The property.** A sealed journal does not contain what it recorded, and still reads
/// back exactly.
#[test]
fn a_sealed_journal_does_not_contain_its_plaintext() {
    let root = dir("sealed");
    let mut provider = InProcessKeyProvider::new();
    let key = provider.generate(KeyPurpose::JournalAtRest);
    let provider = Arc::new(provider);

    let session = SessionId(1);
    {
        let mut journal = FileEventJournal::open(&root).expect("opened");
        journal.seal_with(sealer(&provider, key));
        assert!(journal.is_sealing());
        journal.append(session, &envelope(1)).expect("appended");
        journal.append(session, &envelope(2)).expect("appended");
        journal.sync().expect("synced");
    }

    let raw = on_disk(&root);
    assert!(!raw.is_empty(), "nothing was written");
    // The event's own vocabulary must not be sitting in the file.
    for leaked in ["ModeChanged", "Search", "Standby", "mission_time"] {
        assert!(
            !raw.contains(leaked),
            "{leaked} is in the journal in the clear"
        );
    }

    let mut journal = FileEventJournal::open(&root).expect("reopened");
    journal.seal_with(sealer(&provider, key));
    let read = journal.read_session(session).expect("read back");
    assert_eq!(read.len(), 2);
    assert_eq!(read[0], envelope(1));
    let _ = std::fs::remove_dir_all(root);
}

/// Without the key the journal does not open, and says that rather than looking corrupt.
/// An operator seeing "corrupt" would go looking in the wrong place.
#[test]
fn a_sealed_journal_without_its_key_says_the_key_is_missing() {
    let root = dir("no-key");
    let mut provider = InProcessKeyProvider::new();
    let key = provider.generate(KeyPurpose::JournalAtRest);
    let provider = Arc::new(provider);

    let session = SessionId(1);
    {
        let mut journal = FileEventJournal::open(&root).expect("opened");
        journal.seal_with(sealer(&provider, key));
        journal.append(session, &envelope(1)).expect("appended");
        journal.sync().expect("synced");
    }

    let journal = FileEventJournal::open(&root).expect("reopened");
    assert!(!journal.is_sealing());
    let err = journal.read_session(session).expect_err("cannot read");
    assert!(err.to_string().contains("no key is configured"), "{err}");
    let _ = std::fs::remove_dir_all(root);
}

/// **Rotation does not orphan a journal.** DN-22 §5 requires that a retired key still
/// reads what it protected, and amendment 1 (b) is what makes it possible: the sealed
/// bytes carry the key that sealed them, so a reader holding only the new key still opens
/// the old lines, and nothing on disk is rewritten.
#[test]
fn a_rotation_leaves_an_existing_journal_readable() {
    let root = dir("rotation");
    let mut provider = InProcessKeyProvider::new();
    let first = provider.generate(KeyPurpose::JournalAtRest);

    let session = SessionId(1);
    let shared = Arc::new(provider);
    {
        let mut journal = FileEventJournal::open(&root).expect("opened");
        journal.seal_with(sealer(&shared, first));
        journal.append(session, &envelope(1)).expect("appended");
        journal.sync().expect("synced");
    }
    // The journal held a clone of the provider; it is dropped above, so this is now the
    // only owner and the rotation can take `&mut`.
    let mut provider = Arc::into_inner(shared).expect("sole owner");
    let before = on_disk(&root);

    let second = provider.rotate(KeyPurpose::JournalAtRest).expect("rotated");
    let provider = Arc::new(provider);

    let mut journal = FileEventJournal::open(&root).expect("reopened");
    // Sealing under the *new* key, which is all a caller normally holds.
    journal.seal_with(sealer(&provider, second));
    let read = journal.read_session(session).expect("still readable");
    assert_eq!(read.len(), 1);
    assert_eq!(on_disk(&root), before, "the rotation rewrote the journal");
    let _ = std::fs::remove_dir_all(root);
}

/// Turning encryption on does not orphan what was already recorded in the clear, because
/// AP-08 forbids rewriting an append-only record to migrate it.
#[test]
fn switching_encryption_on_leaves_earlier_sessions_readable() {
    let root = dir("switch-on");
    let session = SessionId(1);
    {
        let mut journal = FileEventJournal::open(&root).expect("opened");
        journal.append(session, &envelope(1)).expect("appended");
        journal.sync().expect("synced");
    }

    let mut provider = InProcessKeyProvider::new();
    let key = provider.generate(KeyPurpose::JournalAtRest);
    let provider = Arc::new(provider);

    let mut journal = FileEventJournal::open(&root).expect("reopened");
    journal.seal_with(sealer(&provider, key));
    let read = journal
        .read_session(session)
        .expect("plaintext still reads");
    assert_eq!(read.len(), 1);

    // And new lines in that same session are sealed from here on. The first line is
    // still in the clear, which is the point -- so the count must be exactly one, not
    // zero: asserting zero would be asserting that the old record had been rewritten.
    journal.append(session, &envelope(2)).expect("appended");
    journal.sync().expect("synced");
    assert_eq!(
        on_disk(&root).matches("ModeChanged").count(),
        1,
        "a line written after encryption was switched on is in the clear, or the \
         earlier plaintext line was rewritten"
    );
    assert_eq!(journal.read_session(session).expect("mixed").len(), 2);
    let _ = std::fs::remove_dir_all(root);
}

/// An unencrypted journal is unchanged by any of this: the default deployment writes
/// exactly what it always wrote.
#[test]
fn an_unsealed_journal_is_unchanged() {
    let root = dir("plain");
    let session = SessionId(1);
    let mut journal = FileEventJournal::open(&root).expect("opened");
    assert!(!journal.is_sealing());
    journal.append(session, &envelope(1)).expect("appended");
    journal.sync().expect("synced");

    assert!(on_disk(&root).contains("ModeChanged"));
    assert_eq!(journal.read_session(session).expect("read").len(), 1);
    let _ = std::fs::remove_dir_all(root);
}

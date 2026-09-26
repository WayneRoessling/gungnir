// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A NaN and an infinity through `append` and `read_session` (GAP-126, decision D-77).
//!
//! Before D-77 `serde_json` wrote both as `null`: a line mid-session then failed the
//! whole session, and as the last line it was dropped as torn with a log line to say
//! so. These tests put non-finite floats where real producers can put them -- a
//! requirement's deadline typed as "inf" (the one live producer the GAP-126 survey
//! found), a covariance that has overflowed, an internally tagged extent's radius, the
//! envelope's own mission time -- both mid-session and as the last line, under both D-04
//! profiles, sealed and in the clear, and read them back **bit for bit** from a fresh
//! journal, as a restarted process would.

use gungnir_eventing::{Envelope, Event, TrackingEvent};
use gungnir_model::events::RequirementEvent;
use gungnir_model::{
    AssetExtent, AssetPriority, Classification, CollectionRequirement, Geodetic, MissionTime,
    Provenance, Quality, Releasability, RequirementId, RequirementState, TrackId, TrackStatus,
    TrackView,
};
use gungnir_store::sealing::JournalSealer;
use gungnir_store::{
    journal, DurabilityPolicy, EventJournal, FileEventJournal, SessionId, StoreError,
};
use std::path::PathBuf;

fn temp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-store-nonfinite-{}-{}",
        tag,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// A reversible stand-in for the binary's AES-GCM sealer: this crate knows nothing
/// about ciphers, and what is under test is that a marked line survives sealing.
struct Reverse;

impl JournalSealer for Reverse {
    fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, StoreError> {
        Ok(plaintext.iter().rev().copied().collect())
    }
    fn unseal(&self, sealed: &[u8]) -> Result<Vec<u8>, StoreError> {
        Ok(sealed.iter().rev().copied().collect())
    }
}

fn deleted(seq: u64) -> Envelope {
    Envelope {
        seq,
        mission_time: MissionTime(12.5 + f64::from(u32::try_from(seq).unwrap_or(0))),
        event: Event::Tracking(TrackingEvent::TrackDeleted(TrackId(seq))),
    }
}

/// A track whose filter has diverged: an infinite variance, a NaN in the state, and a
/// NaN latency in the `f32` quality block.
fn diverged_track(seq: u64) -> Envelope {
    let mut covariance = [[0.0_f64; 6]; 6];
    for (i, row) in covariance.iter_mut().enumerate() {
        row[i] = 25.0;
    }
    covariance[0][0] = f64::INFINITY;
    covariance[3][0] = f64::NEG_INFINITY;
    let view = TrackView {
        id: TrackId(4127),
        status: TrackStatus::Confirmed,
        state: [
            21_406.337_190_62,
            f64::NAN,
            152.3,
            -41.17,
            12.903_4,
            0.1 + 0.2,
        ]
        .into(),
        covariance: covariance.into(),
        classification: Classification::Hostile,
        provenance: Provenance::default(),
        quality: Quality {
            association_confidence: 0.87,
            latency_s: f32::NAN,
            is_stale: false,
        },
        mission_time: MissionTime(57_744.670_227_102_644),
        releasability: Releasability::default(),
    };
    Envelope {
        seq,
        mission_time: MissionTime(57_744.670_227_102_644),
        event: Event::Tracking(TrackingEvent::TrackUpdated(view)),
    }
}

/// The live producer: a deadline typed as "inf" into PN-10, inside a circle whose radius
/// is itself non-finite, which sits in an internally tagged enum serde buffers.
fn requirement_with_an_infinite_deadline(seq: u64) -> Envelope {
    Envelope {
        seq,
        mission_time: MissionTime(900.0),
        event: Event::Requirement(RequirementEvent::Stated {
            requirement: CollectionRequirement {
                id: RequirementId(3),
                title: "\0 a title that begins with NUL".into(),
                priority: AssetPriority::default(),
                area: AssetExtent::Circle {
                    center: Geodetic {
                        lat_rad: 0.9,
                        lon_rad: -0.02,
                        alt_m: 0.0,
                    },
                    radius_m: f64::INFINITY,
                },
                needed_by: Some(MissionTime(f64::INFINITY)),
                state: RequirementState::Stated,
            },
            at: MissionTime(900.0),
        }),
    }
}

/// An envelope stamped with a NaN mission time: a quiet NaN with a payload and its sign
/// bit set, so a journal that kept only "NaN" would be caught.
fn nan_stamped(seq: u64) -> Envelope {
    Envelope {
        seq,
        mission_time: MissionTime(f64::from_bits(0xfff8_0000_0000_beef)),
        event: Event::Tracking(TrackingEvent::TrackCoasting(TrackId(seq))),
    }
}

/// The bits of everything an envelope carries, by way of the journal's own encoding,
/// which writes every non-finite float as its bits. `==` cannot do this: NaN is unequal
/// to itself, and `0.0 == -0.0`.
fn fingerprint(envelope: &Envelope) -> String {
    journal::encode_line(envelope).expect("encodes")
}

fn assert_same(written: &[Envelope], read: &[Envelope], what: &str) {
    assert_eq!(
        read.len(),
        written.len(),
        "{what}: envelopes lost or gained"
    );
    for (w, r) in written.iter().zip(read) {
        assert_eq!(
            fingerprint(r),
            fingerprint(w),
            "{what}: seq {} changed",
            w.seq
        );
    }
}

/// Mid-session and last, both profiles, sealed and not, read back by the journal that
/// wrote it and by a fresh one opened over the same directory.
#[test]
fn a_nan_and_an_infinity_come_back_bit_for_bit_mid_session_and_as_the_last_line() {
    let written = vec![
        deleted(0),
        diverged_track(1),
        requirement_with_an_infinite_deadline(2),
        deleted(3),
        nan_stamped(4),
    ];
    for (profile, policy) in [
        ("node", DurabilityPolicy::node()),
        ("desktop", DurabilityPolicy::desktop()),
    ] {
        for sealed in [false, true] {
            let what = format!("{profile}, sealed {sealed}");
            let root = temp_root(&format!("{profile}-{sealed}"));
            let session = SessionId(11);
            {
                let mut journal = FileEventJournal::open_with_policy(&root, policy).expect("open");
                if sealed {
                    journal.seal_with(Box::new(Reverse));
                }
                for envelope in &written {
                    journal.append(session, envelope).expect("appended");
                }
                assert_same(
                    &written,
                    &journal.read_session(session).expect("read while open"),
                    &what,
                );
                journal.sync().expect("synced");
            }
            let mut reopened = FileEventJournal::open_with_policy(&root, policy).expect("reopen");
            if sealed {
                reopened.seal_with(Box::new(Reverse));
            }
            let read = reopened.read_session(session).expect("read after restart");
            assert_same(&written, &read, &format!("{what}, after restart"));

            // The finite envelopes are still equal as values, not only as encodings.
            assert_eq!(read[0], written[0]);
            assert_eq!(read[3], written[3]);
            let _ = std::fs::remove_dir_all(&root);
        }
    }
}

/// A non-finite envelope as the **only** and therefore last line: the case the torn-tail
/// tolerance used to swallow, leaving a session that read back empty.
#[test]
fn a_session_whose_only_line_is_non_finite_is_not_read_as_torn() {
    let root = temp_root("only-line");
    let session = SessionId(12);
    let mut journal = FileEventJournal::open(&root).expect("open");
    let envelope = requirement_with_an_infinite_deadline(0);
    journal.append(session, &envelope).expect("appended");
    let read = journal.read_session(session).expect("read");
    assert_same(std::slice::from_ref(&envelope), &read, "only line");
    let Event::Requirement(RequirementEvent::Stated { requirement, .. }) = &read[0].event else {
        panic!("the event changed kind");
    };
    // Not `None`: the old defect read an infinite deadline back as no deadline at all.
    assert_eq!(requirement.needed_by.map(|t| t.0), Some(f64::INFINITY));
    let _ = std::fs::remove_dir_all(&root);
}

/// Byte parity: a finite envelope is on disk exactly as `serde_json` writes it, so every
/// journal written before D-77, and every finite line after it, is the same file.
#[test]
fn finite_envelopes_are_written_byte_for_byte_as_before() {
    let root = temp_root("parity");
    let session = SessionId(13);
    let mut journal = FileEventJournal::open(&root).expect("open");
    let finite = [deleted(0), deleted(1)];
    for envelope in &finite {
        journal.append(session, envelope).expect("appended");
    }
    journal.append(session, &nan_stamped(2)).expect("appended");
    journal.sync().expect("synced");
    let text =
        std::fs::read_to_string(root.join(journal::session_file_name(session))).expect("the file");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3);
    for (line, envelope) in lines.iter().zip(&finite) {
        assert_eq!(*line, serde_json::to_string(envelope).expect("serde_json"));
    }
    assert!(
        lines[2].starts_with(gungnir_store::nonfinite::MARKER),
        "only the non-finite envelope is marked: {}",
        lines[2]
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A journal written before D-77 still reads as it did: a `null` where a deadline was is
/// no deadline, and nothing in an unmarked line is treated as an escape.
#[test]
fn a_journal_written_before_the_change_reads_as_it_did() {
    let root = temp_root("legacy");
    let session = SessionId(14);
    let mut legacy = requirement_with_an_infinite_deadline(0);
    if let Event::Requirement(RequirementEvent::Stated { requirement, .. }) = &mut legacy.event {
        requirement.needed_by = None;
        requirement.area = AssetExtent::Point {
            position: Geodetic {
                lat_rad: 0.9,
                lon_rad: -0.02,
                alt_m: 0.0,
            },
        };
    }
    let old_line = serde_json::to_string(&legacy).expect("serde_json");
    std::fs::create_dir_all(&root).expect("dir");
    std::fs::write(
        root.join(journal::session_file_name(session)),
        format!("{old_line}\n"),
    )
    .expect("an old journal");
    let journal = FileEventJournal::open(&root).expect("open");
    assert_eq!(journal.read_session(session).expect("read"), vec![legacy]);
    let _ = std::fs::remove_dir_all(&root);
}

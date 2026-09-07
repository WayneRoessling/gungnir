// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The D-04 durability policy (GAP-085): what each profile promises, and that the
//! buffering does not cost a reader anything.
//!
//! `ARCHITECTURE.md` §10 item 19 settles the policy per deployment profile: the
//! service node fsyncs every envelope, desktop journals are buffered with fsync on
//! session save and every 5 s. These tests pin the observable half of that -- what a
//! reader sees, when a file is switched, and that the default is the strong profile.
//!
//! What they deliberately do **not** claim is that an fsync reached the platter. No
//! portable test can observe that from inside the process; `sync_data` returning `Ok`
//! is the strongest statement available, and the tests assert we call it, not that the
//! hardware honoured it.

use gungnir_eventing::{Envelope, Event, TrackingEvent};
use gungnir_model::{MissionTime, TrackId};
use gungnir_store::{
    DurabilityPolicy, EventJournal, FileEventJournal, SessionId, DESKTOP_FSYNC_INTERVAL,
};
use std::path::PathBuf;
use std::time::Duration;

fn temp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-store-durability-{}-{}",
        tag,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[allow(clippy::cast_precision_loss)]
fn envelope(seq: u64) -> Envelope {
    Envelope {
        seq,
        mission_time: MissionTime(seq as f64 * 0.5),
        event: Event::Tracking(TrackingEvent::TrackDeleted(TrackId(seq))),
    }
}

/// The interval in the code is the interval D-04 states. If someone tunes this for
/// speed, the decision has to move with it.
#[test]
fn desktop_interval_is_the_one_d04_states() {
    assert_eq!(DESKTOP_FSYNC_INTERVAL, Duration::from_secs(5));
    assert_eq!(
        DurabilityPolicy::desktop().max_unsynced(),
        Some(Duration::from_secs(5))
    );
}

/// A journal opened without naming a policy is the strong one. This is the test that
/// stops the buffered profile leaking into the node by default.
#[test]
fn default_policy_is_sync_every_envelope() {
    let root = temp_root("default");
    let journal = FileEventJournal::open(&root).expect("open");
    assert_eq!(journal.policy(), DurabilityPolicy::node());
    assert!(journal.policy().syncs_every_envelope());
    let _ = std::fs::remove_dir_all(&root);
}

/// Buffered writes must still be visible to a reader on the same journal. A replay or
/// a report that silently missed the last few seconds of a session would be a worse
/// bug than the one the buffering fixes.
#[test]
fn buffered_appends_are_visible_to_read_session() {
    let root = temp_root("visible");
    let mut journal =
        FileEventJournal::open_with_policy(&root, DurabilityPolicy::desktop()).expect("open");
    let session = SessionId(11);
    let written: Vec<Envelope> = (0..25).map(envelope).collect();
    for env in &written {
        journal.append(session, env).expect("append");
    }
    // No sync() call: the 5 s interval has certainly not elapsed, so these are still
    // in the buffer.
    assert_eq!(journal.read_session(session).expect("read"), written);
    let _ = std::fs::remove_dir_all(&root);
}

/// And to `sessions()`, which lists what is on disk.
#[test]
fn buffered_appends_are_visible_to_sessions() {
    let root = temp_root("sessions");
    let mut journal =
        FileEventJournal::open_with_policy(&root, DurabilityPolicy::desktop()).expect("open");
    journal.append(SessionId(3), &envelope(0)).expect("append");
    assert_eq!(journal.sessions().expect("sessions"), vec![SessionId(3)]);
    let _ = std::fs::remove_dir_all(&root);
}

/// `sync` is the "session save" half of D-04 and must be safe to call at any time,
/// including when nothing is owed.
#[test]
fn sync_is_idempotent_and_safe_when_empty() {
    let root = temp_root("sync");
    let mut journal =
        FileEventJournal::open_with_policy(&root, DurabilityPolicy::desktop()).expect("open");
    journal.sync().expect("sync with nothing open");
    let session = SessionId(5);
    journal.append(session, &envelope(0)).expect("append");
    journal.sync().expect("first sync");
    journal.sync().expect("second sync");
    assert_eq!(journal.read_session(session).expect("read").len(), 1);
    let _ = std::fs::remove_dir_all(&root);
}

/// Switching sessions must not lose the one being left, whatever the policy: a
/// finished session is exactly the "session save" D-04 names.
#[test]
fn switching_sessions_preserves_the_previous_one() {
    let root = temp_root("switch");
    let mut journal =
        FileEventJournal::open_with_policy(&root, DurabilityPolicy::desktop()).expect("open");
    let (a, b) = (SessionId(1), SessionId(2));
    journal.append(a, &envelope(0)).expect("append a0");
    journal.append(a, &envelope(1)).expect("append a1");
    journal.append(b, &envelope(2)).expect("append b0");
    journal
        .append(a, &envelope(3))
        .expect("append a2 after switching back");

    assert_eq!(
        journal.read_session(a).expect("read a"),
        vec![envelope(0), envelope(1), envelope(3)]
    );
    assert_eq!(journal.read_session(b).expect("read b"), vec![envelope(2)]);
    assert_eq!(journal.sessions().expect("sessions"), vec![a, b]);
    let _ = std::fs::remove_dir_all(&root);
}

/// The bytes must actually reach the file, not merely a buffer this process can read
/// back: a second journal over the same directory is a different process's view.
#[test]
fn a_synced_session_is_readable_by_another_journal() {
    let root = temp_root("crossread");
    let session = SessionId(9);
    let written: Vec<Envelope> = (0..4).map(envelope).collect();
    {
        let mut journal =
            FileEventJournal::open_with_policy(&root, DurabilityPolicy::desktop()).expect("open");
        for env in &written {
            journal.append(session, env).expect("append");
        }
        journal.sync().expect("sync");
        let reader = FileEventJournal::open(&root).expect("second journal");
        assert_eq!(reader.read_session(session).expect("read"), written);
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// Dropping a journal without calling `sync` must not lose the buffer. The host is
/// expected to sync on session save; this covers the path where it does not.
#[test]
fn drop_flushes_the_buffer() {
    let root = temp_root("drop");
    let session = SessionId(13);
    let written: Vec<Envelope> = (0..7).map(envelope).collect();
    {
        let mut journal =
            FileEventJournal::open_with_policy(&root, DurabilityPolicy::desktop()).expect("open");
        for env in &written {
            journal.append(session, env).expect("append");
        }
        // No sync, no read: just drop.
    }
    let reader = FileEventJournal::open(&root).expect("reopen");
    assert_eq!(reader.read_session(session).expect("read"), written);
    let _ = std::fs::remove_dir_all(&root);
}

/// A zero interval degenerates to fsync-per-append, which is the boundary between the
/// two profiles and must not deadlock or double-sync.
#[test]
fn zero_interval_syncs_every_append() {
    let root = temp_root("zero");
    let mut journal = FileEventJournal::open_with_policy(
        &root,
        DurabilityPolicy::Buffered {
            fsync_interval: Duration::ZERO,
        },
    )
    .expect("open");
    let session = SessionId(21);
    for i in 0..5 {
        journal.append(session, &envelope(i)).expect("append");
    }
    let reader = FileEventJournal::open(&root).expect("reopen");
    assert_eq!(reader.read_session(session).expect("read").len(), 5);
    let _ = std::fs::remove_dir_all(&root);
}

/// `sync_if_due` is what makes the interval a wall-clock bound rather than a
/// write-triggered one. With a zero interval it must sync a dirty buffer; under the
/// node profile it must be a no-op, because nothing is ever owed.
#[test]
fn sync_if_due_honours_the_interval() {
    let root = temp_root("due");
    let session = SessionId(31);

    let mut buffered = FileEventJournal::open_with_policy(
        root.join("buffered"),
        DurabilityPolicy::Buffered {
            fsync_interval: Duration::ZERO,
        },
    )
    .expect("open buffered");
    buffered.append(session, &envelope(0)).expect("append");
    buffered.sync_if_due().expect("due sync");

    let mut long = FileEventJournal::open_with_policy(
        root.join("long"),
        DurabilityPolicy::Buffered {
            fsync_interval: Duration::from_secs(3600),
        },
    )
    .expect("open long");
    long.append(session, &envelope(0)).expect("append");
    // Nothing is due for an hour; this must return without syncing and without error.
    long.sync_if_due().expect("not due");
    assert_eq!(long.read_session(session).expect("read").len(), 1);

    let mut node = FileEventJournal::open(root.join("node")).expect("open node");
    node.append(session, &envelope(0)).expect("append");
    node.sync_if_due().expect("no-op under the node profile");

    let _ = std::fs::remove_dir_all(&root);
}

/// The torn-tail tolerance the journal already had must survive the rewrite: a
/// process killed mid-append leaves a partial final line, and reading must drop it
/// rather than failing the whole session.
#[test]
fn torn_final_line_still_tolerated_after_buffering() {
    use std::io::Write;
    let root = temp_root("torn");
    let session = SessionId(41);
    {
        let mut journal =
            FileEventJournal::open_with_policy(&root, DurabilityPolicy::desktop()).expect("open");
        journal.append(session, &envelope(0)).expect("append");
        journal.sync().expect("sync");
    }
    let path = root.join(gungnir_store::journal::session_file_name(session));
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("open for torn write");
    file.write_all(b"{\"seq\":1,\"mission_ti")
        .expect("torn write");
    drop(file);

    let journal = FileEventJournal::open(&root).expect("reopen");
    assert_eq!(
        journal
            .read_session(session)
            .expect("read tolerates torn tail"),
        vec![envelope(0)]
    );
    let _ = std::fs::remove_dir_all(&root);
}

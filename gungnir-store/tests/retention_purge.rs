// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The retention purge (GAP-122, decision D-78), against the `gungnir-store` Retention
//! row of `docs/verification-capability-table.md` §2: "only sessions past the policy age
//! purged".
//!
//! A journal holds sessions aged below, at and above `max_session_age_days`, plus expired
//! sessions that must survive anyway: the one open for appending, one the caller protects
//! and one under a hold. Only the expired, unprotected ones go, and every other session
//! reads back exactly. A purge interrupted halfway leaves the journal readable and the
//! next one finishes it.
//!
//! **Ages are set, not waited for.** Each session file's modification time is set to a
//! whole second and `now` is passed in, so "exactly at the limit" is exact on every
//! filesystem rather than a race against the clock.

use gungnir_eventing::{Envelope, Event, TrackingEvent};
use gungnir_model::{MissionTime, TrackId};
use gungnir_store::retention::{KeptBecause, RetentionPolicy};
use gungnir_store::{journal, EventJournal, FileEventJournal, SessionId, StoreError};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DAY: u64 = 86_400;
const LIMIT_DAYS: u32 = 90;

fn temp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-store-retention-{}-{}",
        tag,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn policy() -> RetentionPolicy {
    RetentionPolicy {
        max_session_age_days: LIMIT_DAYS,
        max_audit_log_age_days: 365,
    }
}

/// A fixed "now", on a whole second.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_790_000_000)
}

/// Three envelopes per session, each different from every other session's, with
/// non-dyadic times so an exact read-back means something.
fn envelopes(session: SessionId) -> Vec<Envelope> {
    (0..3)
        .map(|i| {
            let seq = session.0 * 10 + i;
            Envelope {
                seq,
                mission_time: MissionTime(
                    0.1 * f64::from(u32::try_from(seq).unwrap_or(0)) + 1.0 / 3.0,
                ),
                event: Event::Tracking(TrackingEvent::TrackDeleted(TrackId(seq))),
            }
        })
        .collect()
}

/// Set a session file's last-written time to `idle` before [`now`].
fn age(root: &Path, session: SessionId, idle: Duration) {
    let path = root.join(journal::session_file_name(session));
    let file = std::fs::File::options()
        .write(true)
        .open(&path)
        .expect("the session file");
    file.set_modified(now() - idle).expect("mtime set");
}

fn days(d: u64) -> Duration {
    Duration::from_secs(d * DAY)
}

/// Below, at and above the limit, plus the three kinds of expired session that stay.
#[test]
fn only_sessions_past_the_limit_are_purged_and_the_rest_read_back_exactly() {
    let root = temp_root("ages");
    let mut journal = FileEventJournal::open(&root).expect("open");

    let below = SessionId(1);
    let at = SessionId(2);
    let just_above = SessionId(3);
    let far_above = SessionId(4);
    let held = SessionId(5);
    let protected = SessionId(6);
    let open = SessionId(7);
    let all = [below, at, just_above, far_above, held, protected, open];
    // The open session is appended last, so it is the one the journal holds open.
    for session in all {
        for envelope in envelopes(session) {
            journal.append(session, &envelope).expect("appended");
        }
    }
    age(
        &root,
        below,
        Duration::from_secs(u64::from(LIMIT_DAYS) * DAY - 1),
    );
    age(&root, at, days(u64::from(LIMIT_DAYS)));
    age(
        &root,
        just_above,
        days(u64::from(LIMIT_DAYS)) + Duration::from_secs(1),
    );
    age(&root, far_above, days(400));
    age(&root, held, days(200));
    age(&root, protected, days(200));
    age(&root, open, days(200));
    journal
        .hold(
            held,
            "after-action review of session 5, opened by operator 7",
        )
        .expect("held");

    let mut retired = Vec::new();
    let report = journal
        .purge_expired(&policy(), now(), &BTreeSet::from([protected]), &mut |s| {
            retired.push(s);
            Ok(())
        })
        .expect("purged");

    let purged: Vec<SessionId> = report.purged.iter().map(|p| p.session).collect();
    assert_eq!(purged, vec![just_above, far_above], "{report:?}");
    assert_eq!(retired, purged, "retire is called once per purged session");
    assert!(report.completed.is_empty());
    assert!((report.purged[0].idle_days - (90.0 + 1.0 / 86_400.0)).abs() < 1e-9);
    assert!(report.purged.iter().all(|p| p.bytes > 0));

    let kept: Vec<(SessionId, KeptBecause)> = report
        .kept
        .iter()
        .map(|k| (k.session, k.because.clone()))
        .collect();
    assert_eq!(
        kept,
        vec![
            (
                held,
                KeptBecause::Held("after-action review of session 5, opened by operator 7".into())
            ),
            (protected, KeptBecause::Protected),
            (open, KeptBecause::Open),
        ]
    );

    // Gone, and said to be gone.
    for session in [just_above, far_above] {
        assert!(matches!(
            journal.read_session(session),
            Err(StoreError::UnknownSession(_))
        ));
        assert!(!root.join(journal::purging_file_name(session)).exists());
    }
    // Everything else reads back exactly, from this journal and from a fresh one.
    let survivors = [below, at, held, protected, open];
    assert_eq!(journal.sessions().expect("listed"), survivors.to_vec());
    for session in survivors {
        assert_eq!(
            journal.read_session(session).expect("read"),
            envelopes(session),
            "session {} changed",
            session.0
        );
    }
    // The open session still takes appends after the purge.
    let later = Envelope {
        seq: 999,
        mission_time: MissionTime(99.9),
        event: Event::Tracking(TrackingEvent::TrackCoasting(TrackId(1))),
    };
    journal
        .append(open, &later)
        .expect("appended after the purge");
    drop(journal);
    let reopened = FileEventJournal::open(&root).expect("reopen");
    for session in survivors {
        let mut expected = envelopes(session);
        if session == open {
            expected.push(later.clone());
        }
        assert_eq!(reopened.read_session(session).expect("read"), expected);
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// Releasing a hold is what lets the session go; a second purge with nothing expired
/// removes nothing and says so.
#[test]
fn a_released_hold_no_longer_protects_and_a_quiet_purge_removes_nothing() {
    let root = temp_root("release");
    let journal_dir = root.clone();
    let mut journal = FileEventJournal::open(&journal_dir).expect("open");
    let session = SessionId(3);
    let live = SessionId(4);
    for envelope in envelopes(session) {
        journal.append(session, &envelope).expect("appended");
    }
    for envelope in envelopes(live) {
        journal.append(live, &envelope).expect("appended");
    }
    age(&root, session, days(365));
    journal.hold(session, "legal hold").expect("held");
    assert_eq!(
        journal.holds().expect("holds"),
        vec![(session, "legal hold".to_string())]
    );

    let none = BTreeSet::new();
    let report = journal
        .purge_expired(&policy(), now(), &none, &mut |_| Ok(()))
        .expect("purged");
    assert!(!report.removed_anything());
    assert_eq!(journal.read_session(session).expect("still there").len(), 3);

    assert!(journal.release(session).expect("released"));
    assert!(!journal.release(session).expect("nothing to release"));
    let report = journal
        .purge_expired(&policy(), now(), &none, &mut |_| Ok(()))
        .expect("purged");
    assert_eq!(report.purged.len(), 1);
    assert_eq!(journal.sessions().expect("listed"), vec![live]);

    let report = journal
        .purge_expired(&policy(), now(), &none, &mut |_| Ok(()))
        .expect("purged");
    assert!(!report.removed_anything());
    assert!(report.kept.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

/// A purge interrupted between taking a session out of the listing and deleting it --
/// here by `retire` failing, as it would if the process died there -- leaves every
/// session either whole or gone, and the next purge finishes the one it began.
#[test]
fn an_interrupted_purge_leaves_the_journal_readable_and_the_next_one_finishes_it() {
    let root = temp_root("interrupted");
    let mut journal = FileEventJournal::open(&root).expect("open");
    let first = SessionId(1);
    let second = SessionId(2);
    let keeper = SessionId(3);
    for session in [first, second, keeper] {
        for envelope in envelopes(session) {
            journal.append(session, &envelope).expect("appended");
        }
    }
    journal.sync().expect("synced");
    age(&root, first, days(100));
    age(&root, second, days(100));
    age(&root, keeper, days(10));

    // The "crash": retire fails for the second session, after the first is done.
    let result = journal.purge_expired(&policy(), now(), &BTreeSet::new(), &mut |s| {
        if s == second {
            Err(StoreError::Unavailable("the process died here".into()))
        } else {
            Ok(())
        }
    });
    assert!(result.is_err());

    // Readable: the first is gone, the second is out of the listing, the keeper is whole.
    let reopened = FileEventJournal::open(&root).expect("reopen after the crash");
    assert_eq!(reopened.sessions().expect("listed"), vec![keeper]);
    assert_eq!(
        reopened.read_session(keeper).expect("read"),
        envelopes(keeper)
    );
    assert!(root.join(journal::purging_file_name(second)).exists());

    let mut retired = Vec::new();
    let report = reopened
        .purge_expired(&policy(), now(), &BTreeSet::new(), &mut |s| {
            retired.push(s);
            Ok(())
        })
        .expect("finished");
    assert_eq!(report.completed, vec![second]);
    assert!(report.purged.is_empty());
    assert_eq!(
        retired,
        vec![second],
        "retire runs again for the finished session"
    );
    assert!(!root.join(journal::purging_file_name(second)).exists());
    assert_eq!(
        reopened.read_session(keeper).expect("read"),
        envelopes(keeper)
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A modification time in the future is no age: a clock that jumped forward while a
/// session was written never makes it expire early.
#[test]
fn a_session_written_in_the_future_is_not_old() {
    let root = temp_root("future");
    let mut journal = FileEventJournal::open(&root).expect("open");
    let session = SessionId(1);
    for envelope in envelopes(session) {
        journal.append(session, &envelope).expect("appended");
    }
    journal
        .append(SessionId(2), &envelopes(SessionId(2))[0])
        .expect("appended");
    let path = root.join(journal::session_file_name(session));
    std::fs::File::options()
        .write(true)
        .open(&path)
        .expect("file")
        .set_modified(now() + days(30))
        .expect("mtime");
    let report = journal
        .purge_expired(&policy(), now(), &BTreeSet::new(), &mut |_| Ok(()))
        .expect("purged");
    assert!(!report.removed_anything());
    assert_eq!(
        journal.read_session(session).expect("read"),
        envelopes(session)
    );
    let _ = std::fs::remove_dir_all(&root);
}

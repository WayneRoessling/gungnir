// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The retention purge: removing sessions past the deployment's policy age, per
//! docs/gungnir-capabilities.md §5.1 (GAP-122, decision D-78).
//!
//! # What "age" is
//!
//! **Days since the session's journal was last written** -- its file's modification
//! time. The last write, not the first, so a session that ran for a week is kept for the
//! whole period after it ended. The file's own time rather than anything inside it,
//! because a purge must not need to read a session to decide about it: a journal sealed
//! under an ephemeral key (DN-22 §5) cannot be read by a later process at all, and a
//! policy that could not reach those sessions would keep exactly the ones nobody can use.
//! A time in the future counts as no age, so a clock that jumped forward never makes
//! anything older.
//!
//! # What is never purged
//!
//! - **The session open for appending**, whatever its age.
//! - **A session the caller protects**: the live session before its first append, a
//!   session being replayed, a session holding state still in force.
//! - **A session under a hold** (`session-<id>.hold`, whose text is the reason). The
//!   desktop places one when an after-action review opens and removes it when the review
//!   closes; an administrator may place one by hand on any node. The administrator's task
//!   analysis names "purging a session under review" as the error to design out
//!   (`docs/ux/task-analysis/administrator.md` T-ad-3.2), and a hold is how.
//!
//! Each is reported in [`PurgeReport::kept`] with its reason, so an expired session that
//! stays is never a silent one either.
//!
//! # Crash safety
//!
//! Per session: the journal is **renamed** to `session-<id>.jsonl.purging`, which takes it
//! out of [`crate::EventJournal::sessions`] in one atomic step; the caller's `retire` then
//! removes whatever sits beside it (the mission record); only then is the file deleted.
//! A purge interrupted anywhere leaves every session either whole and listed or out of
//! the listing, never half of one, and the next purge finishes what the last one began
//! ([`PurgeReport::completed`]). Nothing here ever rewrites a line: a purge removes whole
//! sessions, which AP-08's append-only rule permits and a partial rewrite would not.
//!
//! # When it runs
//!
//! Never as a side effect of opening the journal. Both binaries call it deliberately:
//! once at start, after the live session exists, and hourly after that. The hour is far
//! below the policy's granularity of days, and cheap: a purge that removes nothing reads
//! one directory and stats its files.

pub use gungnir_model::RetentionPolicy;

use crate::{journal, FileEventJournal, SessionId, StoreError};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

const SECONDS_PER_DAY: f64 = 86_400.0;

/// A session removed by a purge.
#[derive(Debug, Clone, PartialEq)]
pub struct PurgedSession {
    pub session: SessionId,
    /// Days since its journal was last written.
    pub idle_days: f64,
    /// The size of the journal removed.
    pub bytes: u64,
}

/// Why an expired session was kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeptBecause {
    /// It is the session being appended to.
    Open,
    /// The caller protected it (live, replayed, or holding state still in force).
    Protected,
    /// A hold names it; the hold's text.
    Held(String),
}

/// An expired session a purge did not remove, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct KeptSession {
    pub session: SessionId,
    pub idle_days: f64,
    pub because: KeptBecause,
}

/// What one purge did.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PurgeReport {
    /// Sessions removed by this purge.
    pub purged: Vec<PurgedSession>,
    /// Purges an interrupted earlier run began and this one finished.
    pub completed: Vec<SessionId>,
    /// Sessions past the limit that were kept, each with its reason.
    pub kept: Vec<KeptSession>,
}

impl PurgeReport {
    /// True when the purge removed or finished removing anything.
    #[must_use]
    pub fn removed_anything(&self) -> bool {
        !self.purged.is_empty() || !self.completed.is_empty()
    }
}

impl FileEventJournal {
    /// Place a retention hold on a session: it is not purged, whatever its age, until
    /// the hold is released. `reason` is kept as the hold's text and reported with it.
    ///
    /// Written to a temporary name and renamed, so a hold is either there whole or not
    /// at all. Placing a hold on a session that already has one replaces its reason.
    ///
    /// # Errors
    ///
    /// When the hold cannot be written, which the caller must treat as the session not
    /// being protected.
    pub fn hold(&self, session: SessionId, reason: &str) -> Result<(), StoreError> {
        let path = self.root.join(journal::hold_file_name(session));
        let temporary = path.with_extension("hold.tmp");
        fs::write(&temporary, reason.as_bytes())?;
        fs::rename(&temporary, &path)?;
        sync_directory(&self.root);
        Ok(())
    }

    /// Release a session's hold. `Ok(false)` when there was none.
    ///
    /// # Errors
    ///
    /// When the hold exists and cannot be removed.
    pub fn release(&self, session: SessionId) -> Result<bool, StoreError> {
        match fs::remove_file(self.root.join(journal::hold_file_name(session))) {
            Ok(()) => {
                sync_directory(&self.root);
                Ok(true)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    /// Every hold in the journal directory, with its reason, ascending by session.
    ///
    /// # Errors
    ///
    /// When the directory cannot be read.
    pub fn holds(&self) -> Result<Vec<(SessionId, String)>, StoreError> {
        let mut holds = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if let Some(session) =
                journal::parse_hold_file_name(&entry.file_name().to_string_lossy())
            {
                // A hold that cannot be read still holds: its existence is the protection,
                // and a reason that could not be read is said rather than invented.
                let reason = fs::read_to_string(entry.path())
                    .unwrap_or_else(|err| format!("a hold whose reason could not be read: {err}"));
                holds.push((session, reason));
            }
        }
        holds.sort_by_key(|(s, _)| *s);
        Ok(holds)
    }

    /// Remove every session past `policy`'s age, except those this module's doc names.
    ///
    /// `now` is the wall clock the ages are measured against, passed in so a test can
    /// age a journal without waiting. `protect` is the caller's set of sessions never to
    /// remove. `retire` is called once per removed session after it has left the
    /// listing and before its file is deleted, to remove what sits beside it; it must be
    /// idempotent, because a purge that is interrupted calls it again on the next run.
    ///
    /// # Errors
    ///
    /// When the directory cannot be read, a session cannot be taken out of the listing,
    /// or `retire` fails. Sessions removed before the error stay removed and every other
    /// session stays whole; the next purge carries on from there.
    pub fn purge_expired(
        &self,
        policy: &RetentionPolicy,
        now: SystemTime,
        protect: &BTreeSet<SessionId>,
        retire: &mut dyn FnMut(SessionId) -> Result<(), StoreError>,
    ) -> Result<PurgeReport, StoreError> {
        let mut report = PurgeReport::default();

        // Finish what an interrupted purge began, before anything new.
        for (session, path) in self.files_named(journal::parse_purging_file_name)? {
            retire(session)?;
            remove_if_present(&path)?;
            tracing::info!(
                session = session.0,
                "finished purging a session an interrupted purge had begun"
            );
            report.completed.push(session);
        }

        let open = self.slot().as_ref().map(|o| o.session);
        let holds: std::collections::BTreeMap<SessionId, String> =
            self.holds()?.into_iter().collect();

        for (session, path) in self.files_named(journal::parse_session_file_name)? {
            let metadata = fs::metadata(&path)?;
            let idle_days = metadata
                .modified()
                .ok()
                .and_then(|written| now.duration_since(written).ok())
                .map_or(0.0, |idle| idle.as_secs_f64() / SECONDS_PER_DAY);
            if !policy.session_expired(idle_days) {
                continue;
            }
            let because = if open == Some(session) {
                Some(KeptBecause::Open)
            } else if protect.contains(&session) {
                Some(KeptBecause::Protected)
            } else {
                holds.get(&session).cloned().map(KeptBecause::Held)
            };
            if let Some(because) = because {
                tracing::info!(
                    session = session.0,
                    idle_days,
                    ?because,
                    "a session past the retention limit was kept"
                );
                report.kept.push(KeptSession {
                    session,
                    idle_days,
                    because,
                });
                continue;
            }

            let purging = self.root.join(journal::purging_file_name(session));
            fs::rename(&path, &purging)?;
            sync_directory(&self.root);
            retire(session)?;
            remove_if_present(&purging)?;
            tracing::info!(
                session = session.0,
                idle_days,
                max_session_age_days = policy.max_session_age_days,
                bytes = metadata.len(),
                "purged a session past the retention limit"
            );
            report.purged.push(PurgedSession {
                session,
                idle_days,
                bytes: metadata.len(),
            });
        }
        sync_directory(&self.root);
        Ok(report)
    }

    /// Every file in the journal directory whose name `parse` reads, ascending.
    fn files_named(
        &self,
        parse: fn(&str) -> Option<SessionId>,
    ) -> Result<Vec<(SessionId, std::path::PathBuf)>, StoreError> {
        let mut found = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if let Some(session) = parse(&entry.file_name().to_string_lossy()) {
                found.push((session, entry.path()));
            }
        }
        found.sort_by_key(|(s, _)| *s);
        Ok(found)
    }
}

fn remove_if_present(path: &Path) -> Result<(), StoreError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

/// Make a rename or a removal in `dir` durable where the platform allows it.
///
/// On Unix a directory entry reaches the disk only when the directory is synced. Windows
/// has no portable way to open a directory for that and commits the entry with the
/// operation. Best effort either way: failing to sync the directory leaves a rename that
/// a crash could undo, which the next purge completes or repeats, never a corrupt
/// journal.
fn sync_directory(dir: &Path) {
    #[cfg(unix)]
    if let Ok(handle) = fs::File::open(dir) {
        if let Err(err) = handle.sync_all() {
            tracing::warn!(%err, dir = %dir.display(), "could not sync the journal directory");
        }
    }
    #[cfg(not(unix))]
    let _ = dir;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_is_strictly_after_the_limit() {
        let p = RetentionPolicy {
            max_session_age_days: 10,
            max_audit_log_age_days: 20,
        };
        assert!(!p.session_expired(10.0));
        assert!(p.session_expired(10.5));
        assert!(!p.audit_log_expired(20.0));
        assert!(p.audit_log_expired(21.0));
    }
}

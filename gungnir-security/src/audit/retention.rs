// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Retention of the audit log, and reading an earlier run's segment back (GAP-152,
//! D-105, D-106; `docs/design/DN-23-operator-authentication.md` §14).
//!
//! # Declared, never assumed
//!
//! The age is the baseline's `RetentionPolicy::max_audit_log_age_days`, applied only when
//! the baseline declares a policy at all, exactly as D-78 applies the session age: a
//! product that began deleting its audit record on an upgrade because of a default
//! nobody chose would be the worse failure. The host passes the policy's own rule
//! (`RetentionPolicy::audit_log_expired`) as `expired`, so "strictly after the limit" is
//! said in one place.
//!
//! # What "age" is
//!
//! **Days since the segment was last written**, from the file's own time, as D-78 measures
//! a session: a segment written for a week is kept for the whole period after its last
//! entry. A time in the future counts as no age.
//!
//! # Oldest first, and only a prefix
//!
//! Segments are removed oldest first and the purge **stops at the first one it must
//! keep**. The segments left are therefore always one unbroken chain whose oldest first
//! line is taken as its start, so a purge never reads as a break, and a removal in the
//! middle -- which only somebody other than retention makes -- still does. What is kept:
//!
//! - **The newest segment**, which holds the chain's head: the next run continues it and
//!   numbers its own segment after it.
//! - **This run's segment**, whatever its age.
//! - **A segment under a hold**: `audit-NNNNNN.hold` beside it, whose text is the reason,
//!   placed by an administrator by hand, as `session-<id>.hold` is beside the journal.
//! - **What the host protects**: the segments a run under a session hold anchored in that
//!   session, so the audit record of a session under after-action review stays with it.
//!
//! An expired segment that is kept is reported with its reason, never silently.
//!
//! # Crash safety, and the record of every removal
//!
//! Per segment: it is renamed `audit-NNNNNN.jsonl.purging`, which takes it out of the
//! listing in one step; the host's `record` then puts the removal on the record -- the
//! event journal, made durable, and this log -- and only then is the file deleted. A purge
//! interrupted before the record leaves a `.purging` file the next purge records and
//! deletes; one interrupted after it records it a second time, which the verifier reads
//! the same way. **A removal is never on the disk without being on the record**, which is
//! what lets the verifier tell a purge from a deletion (GAP-163). A `.purging` file the
//! policy would not have purged was not left by a purge: it is restored and said.

use super::{
    file_name, scan_segment, segment_number, segments, split_line, AuditEntry, ChainBreak,
    SEGMENT_SUFFIX,
};
use crate::SecurityError;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const PURGING_SUFFIX: &str = ".purging";
const HOLD_SUFFIX: &str = ".hold";
const SECONDS_PER_DAY: f64 = 86_400.0;

/// A segment a purge removed.
#[derive(Debug, Clone, PartialEq)]
pub struct PurgedSegment {
    /// The file name it had, `audit-NNNNNN.jsonl`.
    pub segment: String,
    /// Complete lines it held.
    pub entries: u64,
    pub bytes: u64,
    /// Days since it was last written.
    pub idle_days: f64,
    /// True when an earlier purge began this removal and was interrupted.
    pub completed: bool,
}

/// Why an expired segment was kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SegmentKeptBecause {
    /// It holds the chain's head.
    Newest,
    /// This run is writing it.
    Current,
    /// A hold names it; the hold's text.
    Held(String),
    /// The host protected it: a run under a session hold wrote it.
    Protected,
}

/// An expired segment a purge kept, and why. The purge stops there.
#[derive(Debug, Clone, PartialEq)]
pub struct KeptSegment {
    pub segment: String,
    pub idle_days: f64,
    pub because: SegmentKeptBecause,
}

/// What one purge did.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SegmentPurgeReport {
    pub purged: Vec<PurgedSegment>,
    /// The expired segment the purge stopped at, if it stopped at one.
    pub kept: Vec<KeptSegment>,
    /// `.purging` files the policy would not have purged, put back in the listing.
    pub restored: Vec<String>,
}

/// Remove every audit segment in `dir` past the host's policy, oldest first, stopping at
/// the first one this module's documentation says to keep.
///
/// `expired` is the policy's rule over days since last written; `now` the wall clock
/// ages are measured against. `current` is the segment this run writes; `protect` the
/// segment names the host keeps. `record` is called once per removal after the segment
/// has left the listing and before its file is deleted; it must make the removal durable
/// on the record, and an error from it stops the purge with the segment still on the
/// disk under its `.purging` name, for the next purge to finish.
///
/// # Errors
///
/// When the directory cannot be read, a segment cannot be renamed or deleted, or
/// `record` fails. Segments removed before the error stay removed and are on the record.
pub fn purge_expired_segments(
    dir: &Path,
    expired: &dyn Fn(f64) -> bool,
    now: SystemTime,
    current: Option<&Path>,
    protect: &BTreeSet<String>,
    record: &mut dyn FnMut(&PurgedSegment) -> Result<(), SecurityError>,
) -> Result<SegmentPurgeReport, SecurityError> {
    let mut report = SegmentPurgeReport::default();
    if !dir.exists() {
        return Ok(report);
    }

    // Finish what an interrupted purge began, before anything new.
    for (name, path) in purging(dir)? {
        let idle_days = idle_days(&path, now)?;
        if dir.join(&name).exists() {
            tracing::error!(segment = %name, "a segment and a purge of it are both on the disk; neither is removed");
            continue;
        }
        if !expired(idle_days) {
            rename(&path, &dir.join(&name))?;
            tracing::warn!(
                segment = %name,
                idle_days,
                "a segment marked for purging is not past the audit-log age: no purge left it, so it is restored"
            );
            report.restored.push(name);
            continue;
        }
        let purged = PurgedSegment {
            entries: scan_segment(&path, None, None)?.entries,
            bytes: size(&path)?,
            segment: name,
            idle_days,
            completed: true,
        };
        record(&purged)?;
        remove_if_present(&path)?;
        tracing::info!(segment = %purged.segment, "finished purging an audit segment an interrupted purge had begun");
        report.purged.push(purged);
    }

    let holds: BTreeMap<String, String> = segment_holds(dir)?.into_iter().collect();
    let current = current.map(file_name);
    let listed = segments(dir)?;
    let newest = listed.len().saturating_sub(1);
    for (index, (_, path)) in listed.iter().enumerate() {
        let name = file_name(path);
        let idle_days = idle_days(path, now)?;
        if !expired(idle_days) {
            break;
        }
        let because = if current.as_deref() == Some(name.as_str()) {
            Some(SegmentKeptBecause::Current)
        } else if index == newest {
            Some(SegmentKeptBecause::Newest)
        } else if let Some(reason) = holds.get(&name) {
            Some(SegmentKeptBecause::Held(reason.clone()))
        } else if protect.contains(&name) {
            Some(SegmentKeptBecause::Protected)
        } else {
            None
        };
        if let Some(because) = because {
            tracing::info!(segment = %name, idle_days, ?because, "an audit segment past the retention limit was kept; the purge stops there");
            report.kept.push(KeptSegment {
                segment: name,
                idle_days,
                because,
            });
            break;
        }
        let purged = PurgedSegment {
            entries: scan_segment(path, None, None)?.entries,
            bytes: size(path)?,
            segment: name,
            idle_days,
            completed: false,
        };
        let purging = dir.join(format!("{}{PURGING_SUFFIX}", purged.segment));
        rename(path, &purging)?;
        sync_directory(dir);
        record(&purged)?;
        remove_if_present(&purging)?;
        tracing::info!(
            segment = %purged.segment,
            idle_days,
            entries = purged.entries,
            "purged an audit segment past the retention limit"
        );
        report.purged.push(purged);
    }
    sync_directory(dir);
    Ok(report)
}

/// Every `.purging` file in `dir`, by the segment name it had, ascending.
pub(super) fn purging(dir: &Path) -> Result<Vec<(String, PathBuf)>, SecurityError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut found: Vec<(u64, String, PathBuf)> = read_dir(dir)?
        .into_iter()
        .filter_map(|path| {
            let name = file_name(&path);
            let segment = name.strip_suffix(PURGING_SUFFIX)?.to_owned();
            Some((segment_number(&segment)?, segment, path))
        })
        .collect();
    found.sort();
    Ok(found.into_iter().map(|(_, s, p)| (s, p)).collect())
}

fn hold_path(dir: &Path, segment: &str) -> Result<PathBuf, SecurityError> {
    segment_number(segment).ok_or_else(|| {
        SecurityError::AuditUnavailable(format!("{segment} is not the name of an audit segment"))
    })?;
    let stem = segment.strip_suffix(SEGMENT_SUFFIX).unwrap_or(segment);
    Ok(dir.join(format!("{stem}{HOLD_SUFFIX}")))
}

/// Place a hold on a segment: retention keeps it, and every segment after it, whatever
/// their age, until the hold is released. Written whole or not at all.
///
/// # Errors
///
/// When the name is not a segment's or the hold cannot be written, which the caller must
/// treat as the segment not being held.
pub fn hold_segment(dir: &Path, segment: &str, reason: &str) -> Result<(), SecurityError> {
    let path = hold_path(dir, segment)?;
    let temporary = path.with_extension("hold.tmp");
    let io = |e: std::io::Error| SecurityError::AuditUnavailable(format!("holding {segment}: {e}"));
    fs::write(&temporary, reason.as_bytes()).map_err(io)?;
    fs::rename(&temporary, &path).map_err(io)?;
    sync_directory(dir);
    Ok(())
}

/// Release a segment's hold. `Ok(false)` when there was none.
///
/// # Errors
///
/// When the name is not a segment's, or the hold exists and cannot be removed.
pub fn release_segment(dir: &Path, segment: &str) -> Result<bool, SecurityError> {
    match fs::remove_file(hold_path(dir, segment)?) {
        Ok(()) => {
            sync_directory(dir);
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(SecurityError::AuditUnavailable(format!(
            "releasing {segment}: {e}"
        ))),
    }
}

/// Every hold in `dir`: the segment it names and its reason, ascending. A hold whose
/// reason cannot be read still holds, and says so.
///
/// # Errors
///
/// When the directory cannot be read.
pub fn segment_holds(dir: &Path) -> Result<Vec<(String, String)>, SecurityError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut holds: Vec<(u64, String, String)> = read_dir(dir)?
        .into_iter()
        .filter_map(|path| {
            let stem = file_name(&path).strip_suffix(HOLD_SUFFIX)?.to_owned();
            let segment = format!("{stem}{SEGMENT_SUFFIX}");
            let number = segment_number(&segment)?;
            let reason = fs::read_to_string(&path)
                .unwrap_or_else(|e| format!("a hold whose reason could not be read: {e}"));
            Some((number, segment, reason))
        })
        .collect();
    holds.sort();
    Ok(holds.into_iter().map(|(_, s, r)| (s, r)).collect())
}

/// One earlier run's segment, read back for the operator's audit panel (GAP-152, D-106).
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentEntries {
    pub segment: String,
    /// Every entry that reads, in the order written.
    pub entries: Vec<AuditEntry>,
    /// Lines that are not entries of this format; each is also a break below.
    pub unreadable: usize,
    /// The segment's own chain, checked as it was read. Whether it reaches the journal's
    /// head is the last verification's [`super::SegmentReport`].
    pub breaks: Vec<ChainBreak>,
}

/// Read a segment's entries back, checking its chain on the way. Read only.
///
/// # Errors
///
/// When `segment` is not the name of a segment in `dir` -- nothing outside the audit
/// directory is ever opened -- or the file cannot be read.
pub fn read_segment(dir: &Path, segment: &str) -> Result<SegmentEntries, SecurityError> {
    if segment_number(segment).is_none() || file_name(Path::new(segment)) != segment {
        return Err(SecurityError::AuditUnavailable(format!(
            "{segment} is not the name of an audit segment"
        )));
    }
    let path = dir.join(segment);
    let breaks = scan_segment(&path, None, None)?.breaks;
    let text = fs::read_to_string(&path)
        .map_err(|e| SecurityError::AuditUnavailable(format!("reading {}: {e}", path.display())))?;
    let mut entries = Vec::new();
    let mut unreadable = 0;
    for line in text.lines() {
        match split_line(line).and_then(|(_, json)| serde_json::from_str::<AuditEntry>(json).ok()) {
            Some(entry) => entries.push(entry),
            None => unreadable += 1,
        }
    }
    Ok(SegmentEntries {
        segment: segment.to_owned(),
        entries,
        unreadable,
        breaks,
    })
}

fn read_dir(dir: &Path) -> Result<Vec<PathBuf>, SecurityError> {
    Ok(fs::read_dir(dir)
        .map_err(|e| SecurityError::AuditUnavailable(format!("reading {}: {e}", dir.display())))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect())
}

fn idle_days(path: &Path, now: SystemTime) -> Result<f64, SecurityError> {
    let metadata = fs::metadata(path)
        .map_err(|e| SecurityError::AuditUnavailable(format!("reading {}: {e}", path.display())))?;
    Ok(metadata
        .modified()
        .ok()
        .and_then(|written| now.duration_since(written).ok())
        .map_or(0.0, |idle| idle.as_secs_f64() / SECONDS_PER_DAY))
}

fn size(path: &Path) -> Result<u64, SecurityError> {
    fs::metadata(path)
        .map(|m| m.len())
        .map_err(|e| SecurityError::AuditUnavailable(format!("reading {}: {e}", path.display())))
}

fn rename(from: &Path, to: &Path) -> Result<(), SecurityError> {
    fs::rename(from, to).map_err(|e| {
        SecurityError::AuditUnavailable(format!(
            "renaming {} to {}: {e}",
            from.display(),
            to.display()
        ))
    })
}

fn remove_if_present(path: &Path) -> Result<(), SecurityError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(SecurityError::AuditUnavailable(format!(
            "removing {}: {e}",
            path.display()
        ))),
    }
}

/// Make a rename or a removal in `dir` durable where the platform allows it; best
/// effort, as `gungnir_store::retention` does it. A rename a crash undoes is one the next
/// purge repeats, never a damaged record.
fn sync_directory(dir: &Path) {
    #[cfg(unix)]
    if let Ok(handle) = fs::File::open(dir) {
        if let Err(err) = handle.sync_all() {
            tracing::warn!(%err, dir = %dir.display(), "could not sync the audit directory");
        }
    }
    #[cfg(not(unix))]
    let _ = dir;
}

#[cfg(test)]
mod tests {
    use super::super::{AuditLog, AuditSync, FileAuditLog};
    use super::*;
    use crate::OperatorId;
    use std::time::Duration;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gungnir-audit-retention-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn runs(dir: &Path, n: usize) -> Vec<String> {
        (0..n)
            .map(|run| {
                let mut log = FileAuditLog::open(dir, AuditSync::EveryEntry, 0.0).expect("opened");
                #[allow(clippy::cast_precision_loss)]
                log.record(AuditEntry::new(
                    Some(OperatorId(1)),
                    "plan.decide",
                    run as f64,
                    format!("run {run}"),
                ));
                log.segment().map(file_name).expect("written")
            })
            .collect()
    }

    /// Every segment is "past the limit" 40 days from now.
    fn later() -> SystemTime {
        SystemTime::now() + Duration::from_hours(40 * 24)
    }

    fn older_than_30(days: f64) -> bool {
        days > 30.0
    }

    #[test]
    fn expired_segments_go_oldest_first_and_the_newest_is_kept() {
        let dir = temp("prefix");
        let names = runs(&dir, 3);
        let mut recorded = Vec::new();
        let report = purge_expired_segments(
            &dir,
            &older_than_30,
            later(),
            None,
            &BTreeSet::new(),
            &mut |p| {
                // On the record while the file is out of the listing and not yet gone.
                assert!(dir.join(format!("{}{PURGING_SUFFIX}", p.segment)).exists());
                recorded.push(p.segment.clone());
                Ok(())
            },
        )
        .expect("purged");
        assert_eq!(recorded, names[..2].to_vec());
        assert_eq!(report.kept[0].because, SegmentKeptBecause::Newest);
        let left = super::super::verify_audit_dir(&dir).expect("read");
        assert!(left.intact(), "a purge never reads as a break");
        assert_eq!(left.segments, 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn nothing_is_purged_that_the_policy_does_not_reach() {
        let dir = temp("young");
        runs(&dir, 3);
        let report = purge_expired_segments(
            &dir,
            &older_than_30,
            SystemTime::now(),
            None,
            &BTreeSet::new(),
            &mut |_| panic!("nothing is due"),
        )
        .expect("purged");
        assert_eq!(report, SegmentPurgeReport::default());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_hold_or_a_protected_segment_stops_the_purge_there() {
        let dir = temp("held");
        let names = runs(&dir, 4);
        hold_segment(&dir, &names[1], "after-action review 7").expect("held");
        let report = purge_expired_segments(
            &dir,
            &older_than_30,
            later(),
            None,
            &BTreeSet::new(),
            &mut |_| Ok(()),
        )
        .expect("purged");
        assert_eq!(report.purged.len(), 1);
        assert_eq!(
            report.kept[0].because,
            SegmentKeptBecause::Held("after-action review 7".into())
        );
        assert!(release_segment(&dir, &names[1]).expect("released"));
        let protect = BTreeSet::from([names[2].clone()]);
        let report =
            purge_expired_segments(&dir, &older_than_30, later(), None, &protect, &mut |_| {
                Ok(())
            })
            .expect("purged");
        assert_eq!(report.purged[0].segment, names[1]);
        assert_eq!(report.kept[0].because, SegmentKeptBecause::Protected);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_purge_the_record_refused_is_finished_by_the_next_and_recorded_then() {
        let dir = temp("interrupted");
        let names = runs(&dir, 2);
        let failed = purge_expired_segments(
            &dir,
            &older_than_30,
            later(),
            None,
            &BTreeSet::new(),
            &mut |_| {
                Err(SecurityError::AuditUnavailable(
                    "the journal is full".into(),
                ))
            },
        );
        assert!(failed.is_err());
        assert!(dir.join(format!("{}{PURGING_SUFFIX}", names[0])).exists());
        // Out of the listing, not on the record: the verifier sees a purge in progress.
        let mut recorded = Vec::new();
        let report = purge_expired_segments(
            &dir,
            &older_than_30,
            later(),
            None,
            &BTreeSet::new(),
            &mut |p| {
                recorded.push((p.segment.clone(), p.completed));
                Ok(())
            },
        )
        .expect("purged");
        assert_eq!(recorded, vec![(names[0].clone(), true)]);
        assert_eq!(report.purged.len(), 1);
        assert!(purging(&dir).expect("listed").is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_purging_file_no_purge_left_is_restored() {
        let dir = temp("renamed");
        let names = runs(&dir, 2);
        fs::rename(
            dir.join(&names[0]),
            dir.join(format!("{}{PURGING_SUFFIX}", names[0])),
        )
        .expect("renamed");
        let report = purge_expired_segments(
            &dir,
            &older_than_30,
            SystemTime::now(),
            None,
            &BTreeSet::new(),
            &mut |_| panic!("not a purge"),
        )
        .expect("purged");
        assert_eq!(report.restored, vec![names[0].clone()]);
        assert!(dir.join(&names[0]).exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_segment_reads_back_with_its_chain_checked_and_nothing_outside_is_opened() {
        let dir = temp("read");
        let names = runs(&dir, 1);
        let read = read_segment(&dir, &names[0]).expect("read");
        assert_eq!(read.entries.len(), 1);
        assert_eq!(read.entries[0].detail, "run 0");
        assert!(read.breaks.is_empty());
        assert!(read_segment(&dir, "../audit-000001.jsonl").is_err());
        assert!(read_segment(&dir, "journal.jsonl").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}

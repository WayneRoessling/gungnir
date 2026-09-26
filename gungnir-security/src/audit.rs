// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Audit logging for configuration/algorithm/plan changes -- every
//! `gungnir_command::DecisionRecord` and `gungnir-config` baseline apply is written
//! here, independent of `gungnir-store`'s mission-session recording. Append-only.
//!
//! # The durable log (GAP-111, D-87, `docs/design/DN-23-operator-authentication.md` §13)
//!
//! [`FileAuditLog`] is the log both binaries keep: the node beside its journal and the
//! desktop beside its own, under `<data dir>/audit/`. It is **one format for both**, so
//! an auditor reads a desktop's record and a node's with the same tool,
//! [`verify_audit_dir`].
//!
//! - **One file per run.** Each process that records anything writes a segment of its
//!   own, `audit-NNNNNN.jsonl`, created with `create_new` on the first entry. Two
//!   processes pointed at one directory therefore never interleave lines inside a file;
//!   they fork the chain instead, which [`verify_audit_dir`] reports rather than
//!   hiding.
//! - **Hash-chained.** Every line carries the SHA-256 of the line before it and its
//!   own, over a domain tag, the previous hash, its sequence number and the entry's
//!   exact JSON text. An edited, removed or inserted line breaks the chain where it
//!   happened. **A cut tail does not**: the last lines of the newest segment can be
//!   removed and what is left still verifies, because nothing outside the file holds
//!   the head. That limit is stated in DN-23 §13 and filed (GAP-163) rather than
//!   implied away.
//! - **Not sealed.** The journal is encrypted at rest and this file is not, on purpose:
//!   an investigation needs the audit record most when the journal key has been lost,
//!   which is DN-22 §11's reason for writing the escrow recovery's own audit row "into a
//!   journal of its own". No entry carries key material, a passphrase or a token;
//!   the details name operators, roles, actions and outcomes only.
//! - **A write failure is kept, counted and said.** An entry that could not be
//!   written stays in [`AuditLog::entries`], the status says so, and the first entry
//!   written after the fault is an `audit.write_failed` line counting what the file is
//!   missing, so the gap is inside the record rather than beside it.

use crate::{OperatorId, SecurityError};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

/// The names of audit entries that are not authorization actions (GAP-111).
///
/// **Not in [`crate::actions::ALL`]** and never nameable by an authority rule: nobody is
/// granted "sign in". These name what happened to a caller rather than what a caller
/// was allowed to do, so an auditor looking for authentication failures finds them
/// under `session.*` and `access.*`, and one looking for refused decisions finds those
/// under the action refused.
pub mod events {
    /// A credential verified and a session began (DN-23 §5 rule 7).
    pub const SIGN_IN: &str = "session.sign_in";
    /// A credential was refused, or nobody could be signed in at all. The entry never
    /// names an operator: the identifier tried is in the detail, because it was
    /// claimed, not verified (DN-23 §5 rule 1).
    pub const SIGN_IN_REJECTED: &str = "session.rejected";
    /// A signed-in operator signed out.
    pub const SIGN_OUT: &str = "session.sign_out";
    /// A request refused for **who** was asking rather than for what it asked: no valid
    /// token, a machine on a route internal to the deployment, or a party whose
    /// agreement does not cover what it asked for. A refusal of a verified operator for
    /// want of a permission is recorded under the permission instead.
    pub const ACCESS_REFUSED: &str = "access.refused";
    /// Entries a rate limit counted and did not record one by one (D-87).
    pub const OVERFLOW: &str = "audit.overflow";
    /// The segment an audit log continued from did not verify when it was opened.
    pub const CHAIN_BROKEN: &str = "audit.chain_broken";
    /// Entries the file could not take, counted when writing resumed.
    pub const WRITE_FAILED: &str = "audit.write_failed";
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AuditEntry {
    pub operator: Option<OperatorId>,
    /// The machine the transport verified, by the common name its certificate carries
    /// (D-02; D-67 for a desktop's name), when the act arrived over a connection that
    /// proved one. `None` for an act on the console itself and for a plaintext caller.
    ///
    /// Beside `operator` rather than folded into it: a desktop acting for a signed-in
    /// operator names both, and an effector reporting under its own certificate names
    /// a party and no operator (GAP-111).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub party: Option<String>,
    /// One of `crate::actions`, one of [`events`], or a free-form description for events
    /// without an actor (e.g. an automatic fallback).
    pub action: String,
    /// Mission time, seconds (`gungnir_model::MissionTime` without the dependency).
    pub mission_time: f64,
    pub detail: String,
}

impl AuditEntry {
    /// An entry with no verified machine behind it.
    #[must_use]
    pub fn new(
        operator: Option<OperatorId>,
        action: impl Into<String>,
        mission_time: f64,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            operator,
            party: None,
            action: action.into(),
            mission_time,
            detail: detail.into(),
        }
    }

    /// The same entry, naming the machine the connection was verified as.
    #[must_use]
    pub fn by_party(mut self, party: Option<String>) -> Self {
        self.party = party;
        self
    }

    /// Whether anything verified stands behind this entry: an operator's session or a
    /// machine's certificate. The rate limit keeps these apart from the rest (D-87).
    #[must_use]
    pub fn is_attributed(&self) -> bool {
        self.operator.is_some() || self.party.is_some()
    }
}

pub trait AuditLog: Send + Sync + std::fmt::Debug {
    fn record(&mut self, entry: AuditEntry);
    /// What this log holds in memory: for [`InMemoryAuditLog`] everything, and for
    /// [`FileAuditLog`] this run's entries, the most recent [`RECENT_CAPACITY`] of them.
    fn entries(&self) -> &[AuditEntry];

    /// Make everything recorded so far durable, where the log has anywhere durable to
    /// put it. A log held only in memory has nothing to do.
    ///
    /// # Errors
    ///
    /// `SecurityError::AuditUnavailable` when the storage refused.
    fn flush(&mut self) -> Result<(), SecurityError> {
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct InMemoryAuditLog {
    entries: Vec<AuditEntry>,
}

impl InMemoryAuditLog {
    pub fn new() -> Self {
        Self::default()
    }
}

impl AuditLog for InMemoryAuditLog {
    fn record(&mut self, entry: AuditEntry) {
        self.entries.push(entry);
    }

    fn entries(&self) -> &[AuditEntry] {
        &self.entries
    }
}

// ---------------------------------------------------------------------------------
// The durable log
// ---------------------------------------------------------------------------------

/// The directory, under a binary's data directory, its audit segments live in.
pub const AUDIT_DIR: &str = "audit";

/// How many of this run's entries [`FileAuditLog::entries`] keeps in memory. The file
/// keeps all of them; this bounds what a long-running node holds (D-87).
pub const RECENT_CAPACITY: usize = 10_000;

const SEGMENT_PREFIX: &str = "audit-";
const SEGMENT_SUFFIX: &str = ".jsonl";
/// The hash a chain's first line continues from.
const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";
/// Bound into every hash, so a line from any other hash-chained format cannot verify as
/// one of these.
const DOMAIN: &[u8] = b"gungnir-audit-v1\n";
/// Where a line's entry begins. The prefix before it holds only digits and hex, so the
/// first occurrence is the real one.
const ENTRY_KEY: &str = ",\"entry\":";

/// When a [`FileAuditLog`] asks the operating system to put its bytes on the disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditSync {
    /// After every entry. The desktop's choice: its entries are a person's acts, a few a
    /// minute, and each one is the only record of who did it.
    EveryEntry,
    /// On [`AuditLog::flush`]. The node's choice: it flushes once per tick, as its
    /// journal's `SyncEveryEnvelope` budget of 100 ms allows, so a burst of refusals
    /// costs one `sync_data` rather than one each.
    OnFlush,
}

/// Whether a [`FileAuditLog`] is putting what it records on the disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditStatus {
    /// Every entry recorded so far is in the file.
    Durable,
    /// The file refused the most recent write. `unwritten` entries are held in memory
    /// only and will be counted in the file once a write succeeds.
    Failing { reason: String, unwritten: u64 },
}

/// One place an audit chain does not verify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainBreak {
    pub segment: PathBuf,
    /// One-based line number within the segment.
    pub line: usize,
    pub reason: String,
}

impl std::fmt::Display for ChainBreak {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} line {}: {}",
            self.segment.display(),
            self.line,
            self.reason
        )
    }
}

/// What [`verify_audit_dir`] found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AuditVerification {
    pub segments: usize,
    pub entries: u64,
    pub breaks: Vec<ChainBreak>,
}

impl AuditVerification {
    /// No break anywhere. **Not** "nothing was removed from the end": see the module
    /// documentation.
    #[must_use]
    pub fn intact(&self) -> bool {
        self.breaks.is_empty()
    }
}

/// The durable, hash-chained audit log (GAP-111, D-87). See the module documentation.
pub struct FileAuditLog {
    dir: PathBuf,
    sync: AuditSync,
    /// The segment this run writes, created on the first entry.
    segment: Option<(PathBuf, File)>,
    /// The hash of the last line in the chain, which the next line continues.
    head: String,
    next_seq: u64,
    recent: Vec<AuditEntry>,
    status: AuditStatus,
    /// Written and not yet synced.
    dirty: bool,
}

impl std::fmt::Debug for FileAuditLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileAuditLog")
            .field("dir", &self.dir)
            .field("segment", &self.segment.as_ref().map(|(p, _)| p))
            .field("next_seq", &self.next_seq)
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

impl FileAuditLog {
    /// Open the log in `dir`, continuing the chain from the newest segment there.
    ///
    /// The newest segment is verified on the way, because it is the one this run's
    /// chain continues; if it does not verify, the first entry this log writes is an
    /// [`events::CHAIN_BROKEN`] entry at `now` naming where, so the record says it was
    /// continued from a damaged one. The older segments are left to
    /// [`verify_audit_dir`], so opening costs one run's worth of reading however long the
    /// deployment has been up.
    ///
    /// # Errors
    ///
    /// `SecurityError::AuditUnavailable` when the directory cannot be created or read.
    /// A binary that cannot open its audit log refuses to start, exactly as it does when
    /// it cannot open its journal.
    pub fn open(dir: &Path, sync: AuditSync, now: f64) -> Result<Self, SecurityError> {
        std::fs::create_dir_all(dir).map_err(|e| {
            SecurityError::AuditUnavailable(format!("creating {}: {e}", dir.display()))
        })?;
        let mut log = Self {
            dir: dir.to_path_buf(),
            sync,
            segment: None,
            head: GENESIS.to_owned(),
            next_seq: 0,
            recent: Vec::new(),
            status: AuditStatus::Durable,
            dirty: false,
        };
        let segments = segments(dir)?;
        let mut continued = None;
        for (_, path) in segments.iter().rev() {
            let scan = scan_segment(path, None)?;
            if let Some((seq, hash)) = scan.last {
                log.head = hash;
                log.next_seq = seq.saturating_add(1);
                continued = Some(scan.breaks);
                break;
            }
        }
        if let Some(breaks) = continued {
            if !breaks.is_empty() {
                let said: Vec<String> = breaks.iter().map(ToString::to_string).collect();
                tracing::error!(breaks = %said.join("; "), "the audit log continued from a segment that does not verify");
                log.record(AuditEntry::new(
                    None,
                    events::CHAIN_BROKEN,
                    now,
                    format!(
                        "this log continues a segment that does not verify: {}",
                        said.join("; ")
                    ),
                ));
            }
        }
        Ok(log)
    }

    /// Where this log's segments are.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The segment this run writes, once it has written anything.
    #[must_use]
    pub fn segment(&self) -> Option<&Path> {
        self.segment.as_ref().map(|(p, _)| p.as_path())
    }

    /// Whether what has been recorded is in the file.
    #[must_use]
    pub fn status(&self) -> &AuditStatus {
        &self.status
    }

    /// Create this run's segment: the next number after every one there, with
    /// `create_new`, so a second process opening the directory at the same moment takes
    /// the number after rather than sharing a file.
    fn create_segment(&mut self) -> std::io::Result<()> {
        let mut number = segments(&self.dir)
            .map_err(|e| std::io::Error::other(e.to_string()))?
            .last()
            .map_or(1, |(n, _)| n.saturating_add(1));
        loop {
            let path = self
                .dir
                .join(format!("{SEGMENT_PREFIX}{number:06}{SEGMENT_SUFFIX}"));
            match OpenOptions::new().create_new(true).append(true).open(&path) {
                Ok(file) => {
                    self.segment = Some((path, file));
                    return Ok(());
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    number = number.saturating_add(1);
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Write one line continuing the chain, or leave the chain where it was.
    ///
    /// A write that fails part-way is cut back to where it began, so the next line
    /// starts on a line of its own rather than completing half of this one.
    fn write_line(&mut self, entry_json: &str) -> std::io::Result<()> {
        if self.segment.is_none() {
            self.create_segment()?;
        }
        let (line, hash) = chain_line(self.next_seq, &self.head, entry_json);
        let Some((_, file)) = self.segment.as_mut() else {
            return Err(std::io::Error::other("the audit segment vanished"));
        };
        let before = file.metadata()?.len();
        if let Err(e) = file.write_all(line.as_bytes()) {
            let _ = file.set_len(before);
            return Err(e);
        }
        self.head = hash;
        self.next_seq = self.next_seq.saturating_add(1);
        self.dirty = true;
        if self.sync == AuditSync::EveryEntry {
            file.sync_data()?;
            self.dirty = false;
        }
        Ok(())
    }

    fn keep(&mut self, entry: AuditEntry) {
        self.recent.push(entry);
        if self.recent.len() > 2 * RECENT_CAPACITY {
            let excess = self.recent.len() - RECENT_CAPACITY;
            self.recent.drain(..excess);
        }
    }
}

impl AuditLog for FileAuditLog {
    fn record(&mut self, entry: AuditEntry) {
        // Owed first: the entries a failed write left out, counted in the chain before
        // anything written after them.
        if let AuditStatus::Failing { reason, unwritten } = self.status.clone() {
            let owed = AuditEntry::new(
                None,
                events::WRITE_FAILED,
                entry.mission_time,
                format!(
                    "{unwritten} audit entries before this one could not be written to the \
                     file and are held only in the memory of the process that recorded \
                     them: {reason}"
                ),
            );
            match serde_json::to_string(&owed) {
                Ok(json) if self.write_line(&json).is_ok() => {
                    self.status = AuditStatus::Durable;
                    self.keep(owed);
                }
                _ => {}
            }
        }
        let written = match serde_json::to_string(&entry) {
            Ok(json) => self.write_line(&json).map_err(|e| e.to_string()),
            Err(e) => Err(format!("the entry could not be encoded: {e}")),
        };
        if let Err(reason) = written {
            let unwritten = match &self.status {
                AuditStatus::Failing { unwritten, .. } => unwritten.saturating_add(1),
                AuditStatus::Durable => {
                    tracing::error!(%reason, dir = %self.dir.display(), "the audit log could not write an entry");
                    1
                }
            };
            self.status = AuditStatus::Failing { reason, unwritten };
        }
        self.keep(entry);
    }

    fn entries(&self) -> &[AuditEntry] {
        &self.recent
    }

    fn flush(&mut self) -> Result<(), SecurityError> {
        if !self.dirty {
            return Ok(());
        }
        let Some((path, file)) = self.segment.as_ref() else {
            return Ok(());
        };
        file.sync_data().map_err(|e| {
            SecurityError::AuditUnavailable(format!("syncing {}: {e}", path.display()))
        })?;
        self.dirty = false;
        Ok(())
    }
}

impl Drop for FileAuditLog {
    /// Last-resort sync, logged rather than swallowed, as the journal's is.
    fn drop(&mut self) {
        if let Err(err) = self.flush() {
            tracing::error!(%err, "the audit log could not be synced on drop");
        }
    }
}

/// One line and the hash it ends the chain on.
fn chain_line(seq: u64, prev: &str, entry_json: &str) -> (String, String) {
    let hash = line_hash(seq, prev, entry_json);
    (
        format!(
            "{{\"seq\":{seq},\"prev\":\"{prev}\",\"hash\":\"{hash}\"{ENTRY_KEY}{entry_json}}}\n"
        ),
        hash,
    )
}

fn line_hash(seq: u64, prev: &str, entry_json: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(DOMAIN);
    hasher.update(prev.as_bytes());
    hasher.update(b"\n");
    hasher.update(seq.to_string().as_bytes());
    hasher.update(b"\n");
    hasher.update(entry_json.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Every segment in `dir`, in order.
fn segments(dir: &Path) -> Result<Vec<(u64, PathBuf)>, SecurityError> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| SecurityError::AuditUnavailable(format!("reading {}: {e}", dir.display())))?;
    let mut found: Vec<(u64, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name();
            let number = name
                .to_str()?
                .strip_prefix(SEGMENT_PREFIX)?
                .strip_suffix(SEGMENT_SUFFIX)?
                .parse()
                .ok()?;
            Some((number, entry.path()))
        })
        .collect();
    found.sort();
    Ok(found)
}

#[derive(serde::Deserialize)]
struct LinePrefix {
    seq: u64,
    prev: String,
    hash: String,
}

/// What reading one segment found.
struct SegmentScan {
    entries: u64,
    /// The last line's sequence number and stored hash.
    last: Option<(u64, String)>,
    breaks: Vec<ChainBreak>,
}

/// Read one segment, checking every line against the one before it.
///
/// `continues` is the `(seq, hash)` the first line must follow, when the caller knows it;
/// `None` takes the first line as the anchor.
fn scan_segment(
    path: &Path,
    continues: Option<&(u64, String)>,
) -> Result<SegmentScan, SecurityError> {
    let mut text = String::new();
    File::open(path)
        .and_then(|mut f| f.read_to_string(&mut text))
        .map_err(|e| SecurityError::AuditUnavailable(format!("reading {}: {e}", path.display())))?;
    let mut scan = SegmentScan {
        entries: 0,
        last: continues.cloned(),
        breaks: Vec::new(),
    };
    let broke = |line: usize, reason: String| ChainBreak {
        segment: path.to_path_buf(),
        line,
        reason,
    };
    let torn = !text.is_empty() && !text.ends_with('\n');
    let lines: Vec<&str> = text.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let number = index + 1;
        if torn && number == lines.len() {
            scan.breaks.push(broke(
                number,
                "the final line is incomplete: a write was interrupted, or the line was cut".into(),
            ));
            break;
        }
        let Some((prefix, entry_json)) = split_line(line) else {
            scan.breaks
                .push(broke(number, "not a line of the audit format".into()));
            continue;
        };
        if let Some((seq, hash)) = &scan.last {
            if prefix.prev != *hash {
                scan.breaks.push(broke(
                    number,
                    "does not continue the line before it: a line was removed, inserted or replaced"
                        .into(),
                ));
            } else if prefix.seq != seq.saturating_add(1) {
                scan.breaks.push(broke(
                    number,
                    format!("sequence jumps from {seq} to {}", prefix.seq),
                ));
            }
        }
        if line_hash(prefix.seq, &prefix.prev, entry_json) != prefix.hash {
            scan.breaks.push(broke(
                number,
                "the line's contents do not match its hash: it was altered".into(),
            ));
        }
        scan.entries += 1;
        scan.last = Some((prefix.seq, prefix.hash));
    }
    Ok(scan)
}

/// A line's chain fields and its entry's exact text, or `None` when it is not a line of
/// this format.
fn split_line(line: &str) -> Option<(LinePrefix, &str)> {
    let body = line.strip_suffix('}')?;
    let at = body.find(ENTRY_KEY)?;
    let prefix: LinePrefix = serde_json::from_str(&format!("{}}}", &body[..at])).ok()?;
    let entry_json = &body[at + ENTRY_KEY.len()..];
    // The entry is JSON, whatever its fields: a verifier checks the chain, not the
    // schema, so a later field added to `AuditEntry` still verifies here.
    serde_json::from_str::<serde_json::Value>(entry_json)
        .ok()
        .filter(serde_json::Value::is_object)?;
    Some((prefix, entry_json))
}

/// Verify every segment in `dir`, oldest first, each continuing the one before.
///
/// The oldest segment's first line is taken as the anchor: retention may have removed
/// what it continued, which is a deletion the policy made rather than a break.
///
/// # Errors
///
/// `SecurityError::AuditUnavailable` when the directory or a segment cannot be read.
pub fn verify_audit_dir(dir: &Path) -> Result<AuditVerification, SecurityError> {
    let mut verification = AuditVerification::default();
    let mut last: Option<(u64, String)> = None;
    for (_, path) in segments(dir)? {
        let scan = scan_segment(&path, last.as_ref())?;
        verification.segments += 1;
        verification.entries += scan.entries;
        verification.breaks.extend(scan.breaks);
        if scan.entries > 0 {
            last = scan.last;
        }
    }
    Ok(verification)
}

// ---------------------------------------------------------------------------------
// The outbox a request handler writes to
// ---------------------------------------------------------------------------------

/// How many entries of one kind an [`AuditOutbox`] takes: a burst, refilled at a rate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LaneLimit {
    pub burst: u32,
    pub per_second: f64,
}

/// Entries with a verified operator or machine behind them (D-87): generous, because
/// each is somebody's act and somebody is accountable for its volume.
pub const ATTRIBUTED_LIMIT: LaneLimit = LaneLimit {
    burst: 1_024,
    per_second: 200.0,
};

/// Entries with nobody verified behind them -- a refused sign-in, a missing token
/// (D-87): enough for every honest mistake a watch makes, and small enough that a
/// flood from outside can neither fill the disk nor crowd out the entries above. What it
/// turns away is counted, and the count is itself an entry.
pub const UNATTRIBUTED_LIMIT: LaneLimit = LaneLimit {
    burst: 64,
    per_second: 4.0,
};

#[derive(Debug)]
struct Lane {
    limit: LaneLimit,
    tokens: f64,
    refilled: Instant,
    dropped: u64,
}

impl Lane {
    fn new(limit: LaneLimit, now: Instant) -> Self {
        Self {
            limit,
            tokens: f64::from(limit.burst),
            refilled: now,
            dropped: 0,
        }
    }

    fn take(&mut self, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.refilled).as_secs_f64();
        self.tokens =
            (self.tokens + elapsed * self.limit.per_second).min(f64::from(self.limit.burst));
        self.refilled = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            self.dropped = self.dropped.saturating_add(1);
            false
        }
    }
}

#[derive(Debug)]
struct Outbox {
    entries: Vec<AuditEntry>,
    attributed: Lane,
    unattributed: Lane,
}

/// Where a request handler puts an audit entry for the loop that owns the log to write
/// (GAP-111, D-87).
///
/// **The handler never touches the disk.** It takes a lock for the length of a `push`,
/// and the node's loop drains the outbox once a tick into its [`FileAuditLog`] and syncs
/// once. Two lanes, each rate-limited, so an unauthenticated flood is bounded in memory
/// and on disk and cannot push out the entries verified callers are owed.
#[derive(Debug)]
pub struct AuditOutbox {
    inner: Mutex<Outbox>,
}

impl Default for AuditOutbox {
    fn default() -> Self {
        Self::with_limits(ATTRIBUTED_LIMIT, UNATTRIBUTED_LIMIT)
    }
}

impl AuditOutbox {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_limits(attributed: LaneLimit, unattributed: LaneLimit) -> Self {
        let now = Instant::now();
        Self {
            inner: Mutex::new(Outbox {
                entries: Vec::new(),
                attributed: Lane::new(attributed, now),
                unattributed: Lane::new(unattributed, now),
            }),
        }
    }

    /// Offer an entry. Kept in arrival order, or counted when its lane is spent.
    pub fn push(&self, entry: AuditEntry) {
        self.push_at(entry, Instant::now());
    }

    fn push_at(&self, entry: AuditEntry, now: Instant) {
        let mut outbox = self.lock();
        let admitted = if entry.is_attributed() {
            outbox.attributed.take(now)
        } else {
            outbox.unattributed.take(now)
        };
        if admitted {
            outbox.entries.push(entry);
        }
    }

    /// Everything offered since the last drain, and what the limits turned away.
    pub fn drain(&self) -> AuditDrain {
        let mut outbox = self.lock();
        AuditDrain {
            entries: std::mem::take(&mut outbox.entries),
            dropped_attributed: std::mem::take(&mut outbox.attributed.dropped),
            dropped_unattributed: std::mem::take(&mut outbox.unattributed.dropped),
        }
    }

    /// A poisoned lock is recovered: the only state behind it is a list of entries, and
    /// an outbox that refused every entry after one panic would be an audit that went
    /// quiet, which is the worse failure.
    fn lock(&self) -> std::sync::MutexGuard<'_, Outbox> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// What one drain of an [`AuditOutbox`] carries.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct AuditDrain {
    pub entries: Vec<AuditEntry>,
    pub dropped_attributed: u64,
    pub dropped_unattributed: u64,
}

impl AuditDrain {
    /// Record the entries in order, then one [`events::OVERFLOW`] entry per lane that
    /// turned any away. Returns how many entries were recorded.
    pub fn record_into(self, log: &mut dyn AuditLog, mission_time: f64) -> usize {
        let mut recorded = self.entries.len();
        for entry in self.entries {
            log.record(entry);
        }
        for (dropped, whose, limit) in [
            (
                self.dropped_attributed,
                "verified operators and machines",
                ATTRIBUTED_LIMIT,
            ),
            (
                self.dropped_unattributed,
                "callers nobody verified",
                UNATTRIBUTED_LIMIT,
            ),
        ] {
            if dropped > 0 {
                log.record(AuditEntry::new(
                    None,
                    events::OVERFLOW,
                    mission_time,
                    format!(
                        "{dropped} entries for requests from {whose} were counted and not \
                         recorded one by one: more arrived than the limit of {} at once and \
                         {} a second (D-87)",
                        limit.burst, limit.per_second
                    ),
                ));
                recorded += 1;
            }
        }
        recorded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(i: u32) -> AuditEntry {
        AuditEntry::new(
            Some(OperatorId(1)),
            "plan.decide",
            f64::from(i),
            format!("entry {i}"),
        )
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gungnir-audit-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn entries_are_kept_in_order() {
        let mut log = InMemoryAuditLog::new();
        for i in 0..3 {
            log.record(entry(i));
        }
        assert_eq!(log.entries().len(), 3);
        assert!((log.entries()[2].mission_time - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_party_is_written_only_when_there_is_one_and_an_old_entry_still_reads() {
        let plain = serde_json::to_string(&entry(0)).expect("encoded");
        assert!(!plain.contains("party"), "{plain}");
        let old: AuditEntry = serde_json::from_str(
            r#"{"operator":7,"action":"plan.decide","mission_time":1.0,"detail":"x"}"#,
        )
        .expect("an entry written before GAP-111 reads");
        assert_eq!(old.party, None);
        let by = entry(0).by_party(Some("desktop-0123456789abcdef".into()));
        assert!(serde_json::to_string(&by)
            .expect("encoded")
            .contains("desktop-0123456789abcdef"));
    }

    #[test]
    fn a_file_log_chains_across_runs_and_verifies() {
        let dir = temp("chain");
        {
            let mut log = FileAuditLog::open(&dir, AuditSync::EveryEntry, 0.0).expect("opened");
            assert!(
                log.segment().is_none(),
                "nothing is created until something is recorded"
            );
            for i in 0..3 {
                log.record(entry(i));
            }
            assert_eq!(log.status(), &AuditStatus::Durable);
        }
        {
            // A run that records nothing leaves no segment behind.
            let _idle = FileAuditLog::open(&dir, AuditSync::OnFlush, 0.0).expect("opened");
        }
        {
            let mut log = FileAuditLog::open(&dir, AuditSync::OnFlush, 5.0).expect("opened");
            log.record(entry(3));
            log.flush().expect("synced");
            assert_eq!(log.entries().len(), 1, "entries() is this run's");
        }
        let verified = verify_audit_dir(&dir).expect("read");
        assert!(verified.intact(), "{:?}", verified.breaks);
        assert_eq!(verified.segments, 2);
        assert_eq!(verified.entries, 4);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn one_segment(dir: &Path) -> PathBuf {
        segments(dir).expect("listed").pop().expect("a segment").1
    }

    #[test]
    fn an_altered_a_removed_and_an_inserted_line_each_break_the_chain_where_they_are() {
        for (name, tamper) in [
            (
                "altered",
                Box::new(|lines: &mut Vec<String>| {
                    lines[1] = lines[1].replace("entry 1", "entry 9");
                }) as Box<dyn Fn(&mut Vec<String>)>,
            ),
            (
                "removed",
                Box::new(|lines: &mut Vec<String>| {
                    lines.remove(1);
                }),
            ),
            (
                "inserted",
                Box::new(|lines: &mut Vec<String>| {
                    let copy = lines[0].clone();
                    lines.insert(1, copy);
                }),
            ),
        ] {
            let dir = temp(&format!("tamper-{name}"));
            {
                let mut log = FileAuditLog::open(&dir, AuditSync::EveryEntry, 0.0).expect("opened");
                for i in 0..4 {
                    log.record(entry(i));
                }
            }
            let path = one_segment(&dir);
            let text = std::fs::read_to_string(&path).expect("read");
            let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
            tamper(&mut lines);
            std::fs::write(&path, lines.join("\n") + "\n").expect("written");
            let verified = verify_audit_dir(&dir).expect("read");
            assert!(!verified.intact(), "{name} went unnoticed");
            assert_eq!(verified.breaks[0].line, 2, "{name}: {:?}", verified.breaks);
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn a_log_continued_from_a_damaged_segment_says_so_first() {
        let dir = temp("damaged");
        {
            let mut log = FileAuditLog::open(&dir, AuditSync::EveryEntry, 0.0).expect("opened");
            for i in 0..2 {
                log.record(entry(i));
            }
        }
        let path = one_segment(&dir);
        let text = std::fs::read_to_string(&path).expect("read");
        std::fs::write(&path, text.replace("entry 0", "entry 7")).expect("written");
        let log = FileAuditLog::open(&dir, AuditSync::EveryEntry, 42.0).expect("opened");
        let first = &log.entries()[0];
        assert_eq!(first.action, events::CHAIN_BROKEN);
        assert!((first.mission_time - 42.0).abs() < f64::EPSILON);
        assert!(first.detail.contains("line 1"), "{}", first.detail);
        drop(log);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_torn_final_line_is_reported_and_the_next_run_starts_a_clean_segment() {
        let dir = temp("torn");
        {
            let mut log = FileAuditLog::open(&dir, AuditSync::EveryEntry, 0.0).expect("opened");
            for i in 0..2 {
                log.record(entry(i));
            }
        }
        let path = one_segment(&dir);
        let text = std::fs::read_to_string(&path).expect("read");
        std::fs::write(&path, &text[..text.len() - 10]).expect("cut");
        let verified = verify_audit_dir(&dir).expect("read");
        assert_eq!(verified.breaks.len(), 1, "{:?}", verified.breaks);
        assert!(verified.breaks[0].reason.contains("incomplete"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn two_logs_on_one_directory_never_share_a_segment() {
        let dir = temp("two");
        let mut a = FileAuditLog::open(&dir, AuditSync::EveryEntry, 0.0).expect("opened");
        let mut b = FileAuditLog::open(&dir, AuditSync::EveryEntry, 0.0).expect("opened");
        a.record(entry(0));
        b.record(entry(1));
        assert_ne!(a.segment(), b.segment());
        drop((a, b));
        // Both continued the same (empty) predecessor, which is a fork and is said.
        let verified = verify_audit_dir(&dir).expect("read");
        assert_eq!(verified.entries, 2);
        assert_eq!(verified.breaks.len(), 1, "{:?}", verified.breaks);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_outbox_keeps_arrival_order_and_counts_what_each_lane_turns_away() {
        let outbox = AuditOutbox::with_limits(
            LaneLimit {
                burst: 3,
                per_second: 0.0,
            },
            LaneLimit {
                burst: 2,
                per_second: 0.0,
            },
        );
        let now = Instant::now();
        for i in 0..5 {
            outbox.push_at(entry(i), now);
            outbox.push_at(
                AuditEntry::new(None, events::SIGN_IN_REJECTED, f64::from(i), "x"),
                now,
            );
        }
        let drain = outbox.drain();
        assert_eq!(drain.entries.len(), 5);
        assert_eq!(drain.dropped_attributed, 2);
        assert_eq!(drain.dropped_unattributed, 3);
        assert_eq!(drain.entries[0].action, "plan.decide");
        assert_eq!(drain.entries[1].action, events::SIGN_IN_REJECTED);
        let mut log = InMemoryAuditLog::new();
        assert_eq!(drain.record_into(&mut log, 9.0), 7);
        let overflows: Vec<&AuditEntry> = log
            .entries()
            .iter()
            .filter(|e| e.action == events::OVERFLOW)
            .collect();
        assert_eq!(overflows.len(), 2);
        assert!(overflows[1].detail.starts_with("3 entries"));
        assert_eq!(outbox.drain(), AuditDrain::default(), "a drain empties it");
    }

    #[test]
    fn a_spent_lane_refills_at_its_rate() {
        let outbox = AuditOutbox::with_limits(
            ATTRIBUTED_LIMIT,
            LaneLimit {
                burst: 1,
                per_second: 2.0,
            },
        );
        let start = Instant::now();
        let refused = || AuditEntry::new(None, events::ACCESS_REFUSED, 0.0, "x");
        outbox.push_at(refused(), start);
        outbox.push_at(refused(), start);
        outbox.push_at(refused(), start + std::time::Duration::from_millis(600));
        let drain = outbox.drain();
        assert_eq!(drain.entries.len(), 2);
        assert_eq!(drain.dropped_unattributed, 1);
    }
}

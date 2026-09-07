//! Persistence & data lifecycle, per docs/gungnir-capabilities.md §5.1.
//! Nothing about a live session persists once it ends unless it goes through this
//! crate -- it is what makes "review a past session" (`gungnir-replay`) and
//! "reconcile after a disconnected period" (`gungnir-resilience`) possible. The
//! journal is the system of record: on the desktop in the disconnected profile, on
//! the service node in the connected profiles (ARCHITECTURE.md §8).

pub mod durability;
pub mod journal;
pub mod retention;
pub mod sealing;

pub use durability::{DurabilityPolicy, DESKTOP_FSYNC_INTERVAL};
pub use gungnir_eventing::Envelope;

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("storage backend unavailable: {0}")]
    Unavailable(String),
    #[error("journal I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("journal encoding failed: {0}")]
    Encoding(#[from] serde_json::Error),
    #[error("no journal for session {0:?}")]
    UnknownSession(SessionId),
    /// The journal could not be sealed or opened (GAP-060, DN-22).
    ///
    /// **An append that cannot be sealed fails.** Writing in the clear instead would tell
    /// an operator encryption was on in a deployment where it had stopped, which is the
    /// silent downgrade AP-02 exists to prevent.
    #[error("journal sealing failed: {0}")]
    Sealing(String),
}

/// Identifier of one recorded mission session.
///
/// Owned by `gungnir-model` and re-exported here, because six crates share it and
/// the standards put a shared type in the lowest crate that needs it
/// (agentic-coding-standards.md §1.2). It was defined here until 2026-09-05, when
/// `gungnir-workflow`'s review case (docs/design/DN-20-after-action-review.md)
/// became the sixth consumer and would otherwise have needed an edge to this crate.
pub use gungnir_model::SessionId;

/// Durable append-only record of every envelope the event bus carried, keyed by
/// mission session -- the basis for `gungnir-replay`'s deterministic playback.
pub trait EventJournal: Send + Sync {
    fn append(&mut self, session: SessionId, envelope: &Envelope) -> Result<(), StoreError>;
    fn read_session(&self, session: SessionId) -> Result<Vec<Envelope>, StoreError>;
    /// Every session this journal holds, ascending.
    fn sessions(&self) -> Result<Vec<SessionId>, StoreError>;

    /// Force everything appended so far onto the disk.
    ///
    /// This is the "fsync on session save" half of D-04
    /// (`ARCHITECTURE.md` §10 item 19): a host calls it when a session is saved or
    /// closed, and after that call every accepted envelope has reached the disk
    /// whatever the [`DurabilityPolicy`] is. Implementations that never buffer may
    /// leave the default, which does nothing because there is nothing owed.
    ///
    /// A failure here means envelopes that were accepted are **not** durable. It is
    /// an error, not a warning, and a caller that ignores it is claiming a durability
    /// it does not have.
    fn sync(&mut self) -> Result<(), StoreError> {
        Ok(())
    }

    /// Fsync if the policy's interval has elapsed, otherwise do nothing.
    ///
    /// A host calls this once per tick. Without it, D-04's "every 5 s" would only
    /// hold while envelopes keep arriving: the interval is checked on append, so a
    /// desktop that journals a burst and then goes quiet would leave that burst
    /// unsynced for as long as it stayed quiet. Calling this every frame is what
    /// makes the 5 s a wall-clock bound rather than a write-triggered one.
    fn sync_if_due(&mut self) -> Result<(), StoreError> {
        Ok(())
    }
}

/// The session file currently held open, with its buffer and its fsync clock.
///
/// Holding the file open across appends is the whole of the performance fix: the
/// previous implementation opened, wrote, and closed once per envelope, which is two
/// syscalls per envelope that buy nothing.
#[derive(Debug)]
struct OpenSession {
    session: SessionId,
    writer: BufWriter<File>,
    /// When this file was last fsynced, for the [`DurabilityPolicy::Buffered`] clock.
    last_sync: Instant,
    /// Whether anything has been written since the last fsync. Avoids an fsync on an
    /// idle desktop, where the 5 s timer would otherwise fire forever against a file
    /// nothing has touched.
    dirty: bool,
}

/// One JSON-lines file per session under `root` (see [`journal`] for the framing).
///
/// Deliberately **not** `Clone`: two clones would hold two buffered writers onto one
/// file and interleave partial lines. Nothing in the workspace cloned it.
///
/// The open file lives behind a `Mutex` rather than in the struct directly because
/// [`EventJournal::read_session`] takes `&self` and must still see envelopes that are
/// sitting in the buffer. A reader that silently missed the last few seconds of a
/// session would be a worse bug than the one this fixes.
pub struct FileEventJournal {
    root: PathBuf,
    policy: DurabilityPolicy,
    open: Mutex<Option<OpenSession>>,
    /// Seals each line when a deployment configures a key (GAP-060, DN-22).
    ///
    /// `None` is unencrypted, which is every deployment that has not set one up and is
    /// what the health summary reports rather than implying otherwise.
    sealer: Option<Box<dyn sealing::JournalSealer>>,
}

impl std::fmt::Debug for FileEventJournal {
    /// Reports **whether** the journal is sealing, never anything about the key.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileEventJournal")
            .field("root", &self.root)
            .field("policy", &self.policy)
            .field("sealing", &self.sealer.is_some())
            .finish_non_exhaustive()
    }
}

impl FileEventJournal {
    /// Open (creating if needed) the journal directory under the default policy,
    /// which is [`DurabilityPolicy::SyncEveryEnvelope`] -- the service-node profile.
    ///
    /// The default is the strong one on purpose: a caller that wants the buffered
    /// desktop profile asks for it by name through [`FileEventJournal::open_with_policy`],
    /// so no journal becomes less durable because a call site did not think about it.
    /// Seal every line this journal writes from now on.
    ///
    /// Set by the binary, which owns key custody (DN-22 §4). **Not set from a
    /// configuration baseline**: nothing there may name key material or a path to it.
    ///
    /// Lines already written stay readable: `sealing::decode` returns a plaintext line
    /// as-is, so switching encryption on does not orphan an existing session. AP-08
    /// forbids rewriting an append-only record to migrate it.
    pub fn seal_with(&mut self, sealer: Box<dyn sealing::JournalSealer>) {
        self.sealer = Some(sealer);
    }

    /// Whether this journal is sealing what it writes, for the health summary.
    #[must_use]
    pub fn is_sealing(&self) -> bool {
        self.sealer.is_some()
    }

    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        Self::open_with_policy(root, DurabilityPolicy::default())
    }

    /// Open the journal directory under an explicit D-04 profile.
    pub fn open_with_policy(
        root: impl Into<PathBuf>,
        policy: DurabilityPolicy,
    ) -> Result<Self, StoreError> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            policy,
            open: Mutex::new(None),
            sealer: None,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The durability profile this journal is running under.
    #[must_use]
    pub fn policy(&self) -> DurabilityPolicy {
        self.policy
    }

    fn session_path(&self, session: SessionId) -> PathBuf {
        self.root.join(journal::session_file_name(session))
    }

    /// Flush the buffer and fsync, if anything is owed.
    fn sync_open(open: &mut OpenSession) -> Result<(), StoreError> {
        if !open.dirty {
            return Ok(());
        }
        open.writer.flush()?;
        // `sync_data`, not `sync_all`: see `durability.rs`. `Write::flush` on a
        // `File` is a no-op, so without this call nothing is actually synced.
        open.writer.get_ref().sync_data()?;
        open.last_sync = Instant::now();
        open.dirty = false;
        Ok(())
    }

    /// Push the buffer into the operating system without necessarily fsyncing, so a
    /// reader (or another process) sees the bytes.
    fn flush_open(open: &mut OpenSession) -> Result<(), StoreError> {
        open.writer.flush()?;
        Ok(())
    }

    /// Ensure the open file is the one for `session`, switching if not.
    ///
    /// Switching sessions always fsyncs the file being left, whatever the policy: a
    /// session that is finished is exactly the "session save" D-04 names.
    fn ensure_open<'a>(
        &self,
        slot: &'a mut Option<OpenSession>,
        session: SessionId,
    ) -> Result<&'a mut OpenSession, StoreError> {
        let matches = slot.as_ref().is_some_and(|o| o.session == session);
        if !matches {
            if let Some(mut previous) = slot.take() {
                Self::sync_open(&mut previous)?;
            }
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.session_path(session))?;
            *slot = Some(OpenSession {
                session,
                writer: BufWriter::new(file),
                last_sync: Instant::now(),
                dirty: false,
            });
        }
        slot.as_mut()
            .ok_or_else(|| StoreError::Unavailable("journal session slot vanished".to_string()))
    }

    /// Lock the open-file slot, treating a poisoned mutex as recoverable.
    ///
    /// A panic while the lock was held cannot corrupt the file: the only state behind
    /// it is a buffered writer, and the worst a poisoned lock means is that a line was
    /// half-written. Refusing to journal for the rest of the process because of that
    /// would turn one panic into a session with no record at all, which is the more
    /// expensive failure.
    fn slot(&self) -> std::sync::MutexGuard<'_, Option<OpenSession>> {
        self.open.lock().unwrap_or_else(|poisoned| {
            tracing::warn!("journal lock was poisoned by an earlier panic; continuing");
            poisoned.into_inner()
        })
    }
}

impl Drop for FileEventJournal {
    /// Last-resort flush. A host is expected to call [`EventJournal::sync`] on session
    /// save; this only catches the path where a journal is dropped without that, and
    /// it logs rather than swallowing, because losing envelopes silently is the thing
    /// this whole change is about.
    fn drop(&mut self) {
        let mut slot = self.slot();
        if let Some(open) = slot.as_mut() {
            if let Err(err) = Self::sync_open(open) {
                tracing::error!(session = open.session.0, %err,
                    "journal could not be synced on drop; envelopes may be lost");
            }
        }
    }
}

impl EventJournal for FileEventJournal {
    fn append(&mut self, session: SessionId, envelope: &Envelope) -> Result<(), StoreError> {
        // Encode before touching the file: an envelope that cannot be encoded must
        // not leave a partial line behind.
        let line = sealing::encode(&journal::encode_line(envelope)?, self.sealer.as_deref())?;
        let policy = self.policy;
        let mut slot = self.slot();
        let open = self.ensure_open(&mut slot, session)?;
        open.writer.write_all(line.as_bytes())?;
        open.writer.write_all(b"\n")?;
        open.dirty = true;

        match policy {
            DurabilityPolicy::SyncEveryEnvelope => Self::sync_open(open)?,
            DurabilityPolicy::Buffered { fsync_interval } => {
                if open.last_sync.elapsed() >= fsync_interval {
                    Self::sync_open(open)?;
                }
            }
        }
        Ok(())
    }

    fn sync(&mut self) -> Result<(), StoreError> {
        let mut slot = self.slot();
        if let Some(open) = slot.as_mut() {
            Self::sync_open(open)?;
        }
        Ok(())
    }

    fn sync_if_due(&mut self) -> Result<(), StoreError> {
        let Some(interval) = self.policy.max_unsynced() else {
            // Every envelope is already synced; there is never anything owed.
            return Ok(());
        };
        let mut slot = self.slot();
        if let Some(open) = slot.as_mut() {
            if open.dirty && open.last_sync.elapsed() >= interval {
                Self::sync_open(open)?;
            }
        }
        Ok(())
    }

    fn read_session(&self, session: SessionId) -> Result<Vec<Envelope>, StoreError> {
        // Anything still in the buffer belongs to this session's record; push it to
        // the operating system before reading so a reader never sees a short file.
        // This is a flush, not an fsync: the reader goes through the same page cache.
        {
            let mut slot = self.slot();
            if let Some(open) = slot.as_mut() {
                if open.session == session {
                    Self::flush_open(open)?;
                }
            }
        }
        let path = self.session_path(session);
        if !path.exists() {
            return Err(StoreError::UnknownSession(session));
        }
        let reader = BufReader::new(File::open(&path)?);
        let lines: Vec<String> = reader.lines().collect::<Result<_, _>>()?;
        let mut envelopes = Vec::with_capacity(lines.len());
        let last = lines.len().saturating_sub(1);
        for (i, line) in lines.iter().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            match sealing::decode(line, self.sealer.as_deref())
                .and_then(|plain| journal::decode_line(&plain).map_err(StoreError::from))
            {
                Ok(env) => envelopes.push(env),
                // A key that cannot open the journal is **never** a torn line, however
                // it falls in the file. Without this the tolerance below swallows it and
                // `read_session` returns an empty session, so an operator missing a key
                // sees a mission that recorded nothing rather than a journal they cannot
                // read. Caught by `gungnir-node/tests/encryption_at_rest.rs`.
                Err(err @ StoreError::Sealing(_)) => return Err(err),
                // A torn final line means the process died mid-append; the
                // envelope was never acknowledged, so dropping it is correct.
                Err(err) if i == last => {
                    tracing::warn!(session = session.0, %err, "dropping torn final journal line");
                }
                Err(err) => return Err(err),
            }
        }
        Ok(envelopes)
    }

    fn sessions(&self) -> Result<Vec<SessionId>, StoreError> {
        // A session whose file exists only in the buffer would otherwise not be
        // listed; flushing first makes `sessions()` agree with `read_session()`.
        {
            let mut slot = self.slot();
            if let Some(open) = slot.as_mut() {
                Self::flush_open(open)?;
            }
        }
        let mut ids: Vec<SessionId> = fs::read_dir(&self.root)?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                journal::parse_session_file_name(&entry.file_name().to_string_lossy())
            })
            .collect();
        ids.sort();
        Ok(ids)
    }
}

#[cfg(test)]
#[allow(clippy::cast_precision_loss)]
mod tests {
    use super::*;
    use gungnir_eventing::{Event, TrackingEvent};
    use gungnir_model::{MissionTime, TrackId};

    fn temp_root(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gungnir-store-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn envelope(seq: u64) -> Envelope {
        Envelope {
            seq,
            mission_time: MissionTime(seq as f64 * 0.5),
            event: Event::Tracking(TrackingEvent::TrackDeleted(TrackId(seq))),
        }
    }

    #[test]
    fn append_then_read_round_trips_exactly() {
        let root = temp_root("roundtrip");
        let mut journal = FileEventJournal::open(&root).expect("open");
        let session = SessionId(42);
        let written: Vec<Envelope> = (0..3).map(envelope).collect();
        for env in &written {
            journal.append(session, env).expect("append");
        }
        assert_eq!(journal.read_session(session).expect("read"), written);
        assert_eq!(journal.sessions().expect("sessions"), vec![session]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn unknown_session_is_an_error() {
        let root = temp_root("unknown");
        let journal = FileEventJournal::open(&root).expect("open");
        assert!(matches!(
            journal.read_session(SessionId(1)),
            Err(StoreError::UnknownSession(_))
        ));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn torn_final_line_is_dropped_not_fatal() {
        let root = temp_root("torn");
        let mut journal = FileEventJournal::open(&root).expect("open");
        let session = SessionId(7);
        journal.append(session, &envelope(0)).expect("append");
        let path = root.join(journal::session_file_name(session));
        let mut file = OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open for torn write");
        file.write_all(b"{\"seq\":1,\"mission_ti")
            .expect("torn write");
        let read = journal
            .read_session(session)
            .expect("read tolerates torn tail");
        assert_eq!(read, vec![envelope(0)]);
        let _ = fs::remove_dir_all(&root);
    }
}

// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Persistence & data lifecycle, per docs/gungnir-capabilities.md §5.1.
//! Nothing about a live session persists once it ends unless it goes through this
//! crate -- it is what makes "review a past session" (`gungnir-replay`) and
//! "reconcile after a disconnected period" (`gungnir-resilience`) possible. The
//! journal is the system of record: on the desktop in the disconnected profile, on
//! the service node in the connected profiles (ARCHITECTURE.md §8).

pub mod durability;
pub mod journal;
pub mod nonfinite;
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
    /// An envelope whose line would not read back as it was written (GAP-126, D-77).
    ///
    /// Checked on the marked lines that carry a non-finite float, the one form this crate
    /// writes that plain JSON cannot. Refused before the file is touched, because a journal
    /// must never hold a line it cannot read back: mid-session such a line makes the
    /// whole session unreadable, and as the last line it is dropped as torn.
    #[error("journal line would not read back faithfully: {0}")]
    Unfaithful(String),
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

    /// Refuse a marked line that would not come back as it was written (GAP-126).
    ///
    /// Compared by re-encoding rather than by `==`, because NaN is unequal to itself and
    /// the re-encoded line carries every float's bits.
    fn check_reads_back(line: &str) -> Result<(), StoreError> {
        let back = journal::decode_line(line)
            .map_err(|e| StoreError::Unfaithful(format!("the line does not decode: {e}")))?;
        let again = journal::encode_line(&back)?;
        if again == line {
            Ok(())
        } else {
            Err(StoreError::Unfaithful(
                "the line decodes to a different envelope".into(),
            ))
        }
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
        let plain = journal::encode_line(envelope)?;
        if plain.starts_with(nonfinite::MARKER) {
            Self::check_reads_back(&plain)?;
        }
        let line = sealing::encode(&plain, self.sealer.as_deref())?;
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
    use gungnir_model::{
        Classification, MissionTime, Provenance, Quality, Releasability, TrackId, TrackStatus,
        TrackView,
    };
    use std::ops::RangeInclusive;

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

    /// Familiar non-dyadic doubles, the one the defect below was found with, and the edges
    /// of the finite range: a signed zero, the smallest subnormal, the smallest normal and
    /// the largest finite value.
    const NAMED: [f64; 7] = [
        0.1 + 0.2,
        1.0 / 3.0,
        57_744.670_227_102_644,
        -0.0,
        f64::from_bits(1),
        f64::MIN_POSITIVE,
        f64::MAX,
    ];

    /// Carrier tracks of random floats: 44 doubles and 2 singles each, 4,224 and 192 in all.
    const CARRIERS: usize = 96;

    /// Every biased exponent of a finite double, subnormals included.
    const WHOLE_RANGE: RangeInclusive<u64> = 0..=2046;

    /// 2^-64 up to 2^65, which holds every unit the journal carries -- metres, metres per
    /// second, their variances, seconds -- with room either side.
    const JOURNAL_RANGE: RangeInclusive<u64> = 959..=1087;

    /// Marsaglia's xorshift64 (J. Stat. Softw. 8(14), 2003; shifts 13, 7, 17), seeded fixed
    /// so a failure names the same floats on every run. Hand-rolled because this crate has
    /// no `rand` dependency.
    struct XorShift64(u64);

    impl XorShift64 {
        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }

        /// A double of random sign and random 52-bit fraction, its biased exponent drawn
        /// from `exponents`.
        fn next_f64(&mut self, exponents: RangeInclusive<u64>) -> f64 {
            let draw = self.next_u64();
            let span = exponents.end() - exponents.start() + 1;
            let exponent = exponents.start() + self.next_u64() % span;
            f64::from_bits((draw & (1 << 63)) | (exponent << 52) | (draw & ((1 << 52) - 1)))
        }

        /// A finite single of random sign, fraction and exponent.
        fn next_f32(&mut self) -> f32 {
            let draw = self.next_u64();
            let exponent = (draw >> 32) % 255;
            let bits = (draw & (1 << 31)) | (exponent << 23) | (draw & ((1 << 23) - 1));
            f32::from_bits(u32::try_from(bits).expect("sign, exponent and fraction fit 32 bits"))
        }
    }

    /// A track of the shape and scale the tracking pipeline publishes: 21 km out, moving at
    /// about 43 m/s, with the covariance a constant-velocity filter carries 0.1 s after an
    /// update left 5 m and 2 m/s one-sigma on each horizontal axis and 8 m and 1 m/s
    /// vertically. Every state entry and every nonzero covariance entry is non-dyadic.
    fn pipeline_track(mission_time: f64) -> TrackView {
        // Step, s; white-acceleration spectral density, m^2/s^3.
        let (dt, q) = (0.1_f64, 0.5_f64);
        let mut covariance = [[0.0_f64; 6]; 6];
        // Per axis, P' = F P F^T + Q, with F = [[1, dt], [0, 1]], P = diag(position,
        // velocity) and Q = q [[dt^3/3, dt^2/2], [dt^2/2, dt]].
        for (axis, (position, velocity)) in [(25.0, 4.0), (25.0, 4.0), (64.0, 1.0)]
            .into_iter()
            .enumerate()
        {
            covariance[axis][axis] = position + dt * dt * velocity + q * dt.powi(3) / 3.0;
            covariance[axis][axis + 3] = dt * velocity + q * dt * dt / 2.0;
            covariance[axis + 3][axis] = covariance[axis][axis + 3];
            covariance[axis + 3][axis + 3] = velocity + q * dt;
        }
        TrackView {
            id: TrackId(4127),
            status: TrackStatus::Confirmed,
            state: [
                21_406.337_190_62,
                -3_120.441_278_9,
                152.3,
                -41.17,
                12.903_4,
                0.1 + 0.2,
            ]
            .into(),
            covariance: covariance.into(),
            classification: Classification::Hostile,
            provenance: Provenance {
                source_sensor_ids: vec![3, 9],
                algorithm_version: "cv-ekf 1.4".into(),
                ..Provenance::default()
            },
            quality: Quality {
                association_confidence: 0.87,
                latency_s: 0.042,
                is_stale: false,
            },
            mission_time: MissionTime(mission_time),
            releasability: Releasability::default(),
        }
    }

    /// The journal under test: each named double as a deletion's mission time, the pipeline
    /// track, then the carriers, whose doubles alternate between the whole finite range and
    /// the journal's own so that neither is thinly sampled.
    fn float_journal() -> Vec<Envelope> {
        let mut envelopes: Vec<Envelope> = NAMED
            .iter()
            .zip(0_u64..)
            .map(|(&time, seq)| Envelope {
                seq,
                mission_time: MissionTime(time),
                event: Event::Tracking(TrackingEvent::TrackDeleted(TrackId(seq))),
            })
            .collect();
        let pipeline_time = NAMED[2] + 0.1;
        envelopes.push(Envelope {
            seq: envelopes.len() as u64,
            mission_time: MissionTime(pipeline_time),
            event: Event::Tracking(TrackingEvent::TrackInitiated(pipeline_track(pipeline_time))),
        });
        let mut rng = XorShift64(0x9E37_79B9_7F4A_7C15);
        for id in (0_u64..).take(CARRIERS) {
            let mut drawn = [0.0_f64; 44];
            for (i, slot) in drawn.iter_mut().enumerate() {
                *slot = rng.next_f64(if i % 2 == 0 {
                    WHOLE_RANGE
                } else {
                    JOURNAL_RANGE
                });
            }
            let mut state = [0.0_f64; 6];
            state.copy_from_slice(&drawn[1..7]);
            let mut covariance = [[0.0_f64; 6]; 6];
            covariance.copy_from_slice(drawn[7..43].as_chunks::<6>().0);
            let view = TrackView {
                id: TrackId(id),
                status: TrackStatus::Tentative,
                state: state.into(),
                covariance: covariance.into(),
                classification: Classification::Unknown,
                provenance: Provenance::default(),
                quality: Quality {
                    association_confidence: rng.next_f32(),
                    latency_s: rng.next_f32(),
                    is_stale: false,
                },
                mission_time: MissionTime(drawn[43]),
                releasability: Releasability::default(),
            };
            envelopes.push(Envelope {
                seq: envelopes.len() as u64,
                mission_time: MissionTime(drawn[0]),
                event: Event::Tracking(TrackingEvent::TrackUpdated(view)),
            });
        }
        envelopes
    }

    /// Every double and every single an envelope carries, in a fixed order.
    fn floats(envelope: &Envelope) -> (Vec<f64>, Vec<f32>) {
        let mut doubles = vec![envelope.mission_time.0];
        let mut singles = Vec::new();
        match &envelope.event {
            Event::Tracking(
                TrackingEvent::TrackInitiated(view) | TrackingEvent::TrackUpdated(view),
            ) => {
                doubles.extend(view.state.iter());
                doubles.extend(view.covariance.iter());
                doubles.push(view.mission_time.0);
                singles.extend([view.quality.association_confidence, view.quality.latency_s]);
            }
            Event::Tracking(TrackingEvent::TrackDeleted(_) | TrackingEvent::TrackCoasting(_)) => {}
            other => panic!("the journal under test carries tracking events only, not {other:?}"),
        }
        (doubles, singles)
    }

    /// Every float `read` does not hold bit for bit where `written` put it, described.
    fn changed_floats(written: &[Envelope], read: &[Envelope]) -> Vec<String> {
        let mut changed = Vec::new();
        for (wrote, got) in written.iter().zip(read) {
            let ((wrote_f64, wrote_f32), (got_f64, got_f32)) = (floats(wrote), floats(got));
            if (wrote_f64.len(), wrote_f32.len()) != (got_f64.len(), got_f32.len()) {
                changed.push(format!("seq {}: a different payload came back", wrote.seq));
                continue;
            }
            for (a, b) in wrote_f64.iter().zip(&got_f64) {
                if a.to_bits() != b.to_bits() {
                    changed.push(format!("seq {}: {a:?} came back as {b:?}", wrote.seq));
                }
            }
            for (a, b) in wrote_f32.iter().zip(&got_f32) {
                if a.to_bits() != b.to_bits() {
                    changed.push(format!("seq {}: {a:?}f32 came back as {b:?}f32", wrote.seq));
                }
            }
        }
        changed
    }

    /// The `gungnir-store` Journal round-trip; retention row of
    /// `docs/verification-capability-table.md` §2, its round-trip clause ("Exact
    /// round-trip"): every float an envelope carries comes back from the journal **bit for
    /// bit**, and every envelope comes back equal, under both D-04 profiles.
    ///
    /// **Why non-dyadic values.** `append_then_read_round_trips_exactly` journals
    /// half-integer times, short decimals any parser reads exactly, so it cannot fail on the
    /// float path. `serde_json` without its `float_roundtrip` feature is not correctly
    /// rounded: it converts a number's digits to a double and then multiplies or divides by
    /// a power of ten, two rounding steps where a correct parse takes one, and
    /// 57744.670227102644 came back as 57744.67022710264. Every journaled track state was
    /// exposed to that until the workspace manifest turned the feature on (2026-09-16, the
    /// GAP-067 walk); this test is what fails if it goes off again. Measured with this
    /// generator against `serde_json` 1.0.151 without the feature: 1,050 of the 4,224 random
    /// doubles came back changed, and 57744.670227102644 with them. `0.1 + 0.2` and `1/3`
    /// came back exact, so they are here as the familiar cases, not as the ones that catch it.
    ///
    /// **Why bits.** `==` calls `0.0` and `-0.0` equal and would pass a journal that lost a
    /// sign. Envelope equality is asserted as well, for everything that is not a float.
    #[test]
    fn non_dyadic_floats_round_trip_bit_for_bit_in_both_profiles() {
        let written = float_journal();
        let (doubles, singles) = written
            .iter()
            .map(floats)
            .fold((0, 0), |(d, s), (f64s, f32s)| {
                (d + f64s.len(), s + f32s.len())
            });
        // The journal carries what it says: one double for each named envelope, and 44
        // doubles and 2 singles for the pipeline track and for each carrier.
        let tracks = 1 + CARRIERS;
        assert_eq!((doubles, singles), (NAMED.len() + 44 * tracks, 2 * tracks));

        for (profile, policy) in [
            ("node", DurabilityPolicy::node()),
            ("desktop", DurabilityPolicy::desktop()),
        ] {
            let root = temp_root(&format!("floats-{profile}"));
            let mut journal = FileEventJournal::open_with_policy(&root, policy).expect("open");
            let session = SessionId(57);
            for env in &written {
                journal.append(session, env).expect("append");
            }
            let read = journal.read_session(session).expect("read");
            assert_eq!(
                read.len(),
                written.len(),
                "{profile}: envelopes lost or gained"
            );
            let changed = changed_floats(&written, &read);
            assert!(
                changed.is_empty(),
                "{profile}: {} of {} floats changed, the first: {:?}",
                changed.len(),
                doubles + singles,
                &changed[..changed.len().min(8)]
            );
            assert_eq!(read, written, "{profile}");
            let _ = fs::remove_dir_all(&root);
        }
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

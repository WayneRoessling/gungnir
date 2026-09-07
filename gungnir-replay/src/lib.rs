//! Reporting, after-action review & export (playback half), per
//! docs/gungnir-capabilities.md §5.6. `gungnir-scenario`'s round-trip fidelity
//! covers replaying a *synthetic, generated* scenario for testing; this crate
//! replays a *recorded* session for review/training/incident analysis -- reusing
//! `ReplayClockAuthority` from gungnir-time so replay timing is deterministic and
//! reproducible: two replays of the same journal yield the same envelope sequence.

use gungnir_eventing::Envelope;
use gungnir_model::MissionTime;
use gungnir_store::{EventJournal, SessionId, StoreError};
use gungnir_time::{ReplayClockAuthority, TimeAuthority};

/// A loaded session with a cursor. The clock always reads the mission time of the
/// most recently stepped envelope, so consumers that ask a `TimeAuthority` for
/// "now" see recorded time, not wall time.
pub struct ReplaySession {
    envelopes: Vec<Envelope>,
    cursor: usize,
    clock: ReplayClockAuthority,
}

impl ReplaySession {
    /// Load every envelope of `session` from `journal`. The clock starts at the
    /// first envelope's mission time (or zero for an empty session).
    pub fn open(journal: &dyn EventJournal, session: SessionId) -> Result<Self, StoreError> {
        let envelopes = journal.read_session(session)?;
        let start = envelopes
            .first()
            .map_or(MissionTime::default(), |e| e.mission_time);
        Ok(Self {
            envelopes,
            cursor: 0,
            clock: ReplayClockAuthority { current: start },
        })
    }

    /// Advance to the next envelope and return it; `None` at the end. The UI's
    /// timeline scrubber calls this (or [`seek_to`](Self::seek_to)) to drive playback.
    pub fn step(&mut self) -> Option<&Envelope> {
        let env = self.envelopes.get(self.cursor)?;
        self.clock.current = env.mission_time;
        self.cursor += 1;
        Some(env)
    }

    /// Move the cursor so that the next [`step`](Self::step) returns the first
    /// envelope at or after `t`; returns the new cursor position.
    pub fn seek_to(&mut self, t: MissionTime) -> usize {
        self.cursor = self.envelopes.partition_point(|e| e.mission_time < t);
        self.clock.current = self
            .envelopes
            .get(self.cursor.saturating_sub(1))
            .map_or(t, |e| e.mission_time);
        self.cursor
    }

    /// Mission time of the envelope at `index`, for a scrubber.
    ///
    /// A position slider works in fractions of the session and [`seek_to`](Self::seek_to)
    /// works in mission time, so something has to convert between them. Doing it here
    /// rather than in the caller keeps the envelope list private and keeps the
    /// conversion next to the ordering it depends on: two envelopes at the same instant
    /// are not separable by a scrubber, which is correct, because they were not
    /// separable in time either.
    pub fn mission_time_at(&self, index: usize) -> Option<MissionTime> {
        self.envelopes.get(index).map(|e| e.mission_time)
    }

    pub fn clock(&self) -> &dyn TimeAuthority {
        &self.clock
    }

    pub fn len(&self) -> usize {
        self.envelopes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.envelopes.is_empty()
    }

    pub fn remaining(&self) -> usize {
        self.envelopes.len() - self.cursor
    }
}

#[cfg(test)]
#[allow(clippy::cast_precision_loss)]
mod tests {
    use super::*;
    use gungnir_eventing::{Event, TrackingEvent};
    use gungnir_model::TrackId;
    use gungnir_store::FileEventJournal;

    fn recorded_session(tag: &str) -> (FileEventJournal, SessionId, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("gungnir-replay-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut journal = FileEventJournal::open(&root).expect("open");
        let session = SessionId(1);
        for seq in 0..4_u64 {
            journal
                .append(
                    session,
                    &Envelope {
                        seq,
                        mission_time: MissionTime(10.0 + seq as f64),
                        event: Event::Tracking(TrackingEvent::TrackDeleted(TrackId(seq))),
                    },
                )
                .expect("append");
        }
        (journal, session, root)
    }

    #[test]
    fn two_replays_are_identical_and_clock_follows_envelopes() {
        let (journal, session, root) = recorded_session("identical");
        let mut a = ReplaySession::open(&journal, session).expect("open a");
        let mut b = ReplaySession::open(&journal, session).expect("open b");
        assert_eq!(a.clock().now(), MissionTime(10.0));
        let seq_a: Vec<u64> = std::iter::from_fn(|| a.step().map(|e| e.seq)).collect();
        let seq_b: Vec<u64> = std::iter::from_fn(|| b.step().map(|e| e.seq)).collect();
        assert_eq!(seq_a, seq_b);
        assert_eq!(seq_a, vec![0, 1, 2, 3]);
        assert_eq!(a.clock().now(), MissionTime(13.0));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn seek_positions_cursor_at_first_envelope_not_before_t() {
        let (journal, session, root) = recorded_session("seek");
        let mut r = ReplaySession::open(&journal, session).expect("open");
        assert_eq!(r.seek_to(MissionTime(11.5)), 2);
        assert_eq!(r.step().map(|e| e.seq), Some(2));
        assert_eq!(r.remaining(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }
}

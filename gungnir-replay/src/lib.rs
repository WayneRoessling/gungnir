// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

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
    use gungnir_model::{
        Classification, Provenance, Quality, Releasability, TrackId, TrackStatus, TrackView,
    };
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

    /// One track initiated, updated four times a third of a second apart, and deleted, as
    /// the tracking pipeline journals one. Every nonzero float in it is non-dyadic, and the
    /// first mission time is the double `serde_json` without `float_roundtrip` read back one
    /// ULP off.
    fn track_journal() -> Vec<Envelope> {
        let start = 57_744.670_227_102_644;
        let mut journal: Vec<Envelope> = (0..5_u64)
            .map(|seq| {
                let elapsed = seq as f64 / 3.0;
                // Position variance shrinking as updates arrive, with its velocity terms.
                let shrink = 3.0 / (seq as f64 + 3.0);
                let mut covariance = [[0.0_f64; 6]; 6];
                for axis in 0..3 {
                    covariance[axis][axis] = 25.1 * shrink;
                    covariance[axis][axis + 3] = 0.402_5 * shrink;
                    covariance[axis + 3][axis] = 0.402_5 * shrink;
                    covariance[axis + 3][axis + 3] = 4.05 * shrink;
                }
                let view = TrackView {
                    id: TrackId(7),
                    status: if seq == 0 {
                        TrackStatus::Tentative
                    } else {
                        TrackStatus::Confirmed
                    },
                    state: [
                        21_406.337_190_62 - 41.17 * elapsed,
                        -3_120.441_278_9 + 12.903_4 * elapsed,
                        152.3,
                        -41.17,
                        12.903_4,
                        0.1 + 0.2,
                    ]
                    .into(),
                    covariance: covariance.into(),
                    classification: Classification::Hostile,
                    provenance: Provenance {
                        source_sensor_ids: vec![3],
                        algorithm_version: "cv-ekf 1.4".into(),
                        ..Provenance::default()
                    },
                    quality: Quality {
                        association_confidence: 0.87,
                        latency_s: 0.042,
                        is_stale: false,
                    },
                    mission_time: MissionTime(start + elapsed),
                    releasability: Releasability::default(),
                };
                Envelope {
                    seq,
                    mission_time: MissionTime(start + elapsed),
                    event: Event::Tracking(if seq == 0 {
                        TrackingEvent::TrackInitiated(view)
                    } else {
                        TrackingEvent::TrackUpdated(view)
                    }),
                }
            })
            .collect();
        journal.push(Envelope {
            seq: 5,
            mission_time: MissionTime(start + 5.0 / 3.0),
            event: Event::Tracking(TrackingEvent::TrackDeleted(TrackId(7))),
        });
        journal
    }

    /// The `gungnir-replay` Deterministic playback row of
    /// `docs/verification-capability-table.md` §2 ("Identical event sequences"), over a
    /// synthetic journal of `TrackView` payloads.
    ///
    /// `two_replays_are_identical_and_clock_follows_envelopes` compares sequence numbers,
    /// which a replay that returned a changed payload or a shifted mission time would still
    /// pass. This collects every envelope each replay steps through and compares them
    /// **whole**, payload and mission time included, and against what was journaled as well
    /// as against each other: two replays of one journal read the same bytes through the same
    /// parser, so they agree even when both are wrong. That is not hypothetical. Until the
    /// workspace turned on `serde_json`'s `float_roundtrip` feature (2026-09-16), its parser was
    /// not correctly rounded, read 57744.670227102644 back as 57744.67022710264, and both
    /// replays of this journal would have matched each other and not the record.
    /// `gungnir-store`'s `non_dyadic_floats_round_trip_bit_for_bit_in_both_profiles` gates
    /// the parse itself and records how often it was wrong.
    #[test]
    fn two_replays_equal_each_other_and_the_journal_whole() {
        let root =
            std::env::temp_dir().join(format!("gungnir-replay-whole-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let session = SessionId(2);
        let journaled = track_journal();
        {
            let mut journal = FileEventJournal::open(&root).expect("open");
            for env in &journaled {
                journal.append(session, env).expect("append");
            }
        }

        // Each replay over its own store on the directory, as two later reviews would open it.
        let replay = || {
            let journal = FileEventJournal::open(&root).expect("reopen");
            let mut playback = ReplaySession::open(&journal, session).expect("open replay");
            let stepped: Vec<Envelope> = std::iter::from_fn(|| playback.step().cloned()).collect();
            (stepped, playback.clock().now())
        };
        let (first, first_clock) = replay();
        let (second, second_clock) = replay();

        assert_eq!(first, second, "two replays of one journal differ");
        assert_eq!(first, journaled, "the replay is not what was journaled");
        // The clock ends on the last envelope's recorded time, to the bit.
        let last = journaled.last().expect("a journal").mission_time;
        assert_eq!(first_clock.0.to_bits(), last.0.to_bits());
        assert_eq!(second_clock.0.to_bits(), last.0.to_bits());
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

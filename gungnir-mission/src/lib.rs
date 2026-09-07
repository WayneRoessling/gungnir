// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Mission/session lifecycle, per gungnir-capabilities.md §5.1.
//! `gungnir-scenario` remains the *generator* of synthetic scenarios (test/bench
//! only, never a runtime dependency of a production crate, per
//! agentic-coding-standards.md §1.1); this crate is what turns a scenario -- or a
//! live sensor session -- into something an operator can create/save/reload/replay
//! through the application itself.
//!
//! # The state a mission is in is a claim about the world
//!
//! `Live` means sensors are feeding this session **now**. That is why a mission
//! reopened from disk is never `Live`, whatever the record says: the feed that made it
//! live is gone, and a session presented as live because a file said so would be the
//! worst thing this crate could do. See [`MissionState::Interrupted`].
//!
//! # Why this does not use `gungnir-replay`
//!
//! The closing action for GAP-051 names it, and it is the wrong tool. `gungnir-replay`
//! is a **cursor** over a journal -- open, step, seek, rate -- which is what PN-12 scrubs
//! with. The lifecycle needs the recorded sequence, in order, once, which is exactly
//! `gungnir_store::EventJournal::read_session`. Taking the edge would add a dependency to
//! use a subset of what this crate already has.

use gungnir_config::ConfigBaseline;
use gungnir_eventing::Envelope;
use gungnir_store::{EventJournal, SessionId};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MissionState {
    Created,
    Live,
    Paused,
    /// A session that was live and was never closed: the process stopped, or the
    /// machine did.
    ///
    /// **Not `Paused`**, which is somebody's decision to stop recording, and not `Live`,
    /// which is a claim about sensors feeding it right now. An interrupted session may be
    /// reviewed and never resumed -- whatever was happening while it was interrupted is
    /// not in the record, and a session that carried on afterwards would have a hole in
    /// it that nothing marked.
    Interrupted,
    Replaying,
    Closed,
}

impl MissionState {
    /// Whether a mission may move from this state to `next`.
    ///
    /// The rule worth stating: **nothing returns to `Live` except a deliberate pause.**
    /// Live means a feed is arriving, and a session reopened from disk has no feed.
    #[must_use]
    pub fn can_transition_to(self, next: MissionState) -> bool {
        use MissionState::{Closed, Created, Interrupted, Live, Paused, Replaying};
        matches!(
            (self, next),
            // A new mission starts recording or is abandoned unstarted; a paused one
            // resumes or ends. **Resuming is the only road back to `Live`, and it runs
            // through somebody's decision to pause.**
            (Created | Paused, Live | Closed)
                // Recording stops deliberately, or ends.
                | (Live, Paused | Closed)
                // An interrupted or closed session is reviewable and never resumable.
                | (Interrupted | Closed, Replaying | Closed)
                | (Replaying, Closed)
        )
    }

    /// True while envelopes may still be recorded into this session.
    #[must_use]
    pub fn is_recording(self) -> bool {
        matches!(self, MissionState::Live)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mission {
    pub session: SessionId,
    pub state: MissionState,
    pub config: ConfigBaseline,
    /// Why the baseline this session runs under did not validate, when it did not.
    ///
    /// **A session may legitimately run under a baseline that does not validate**, and a
    /// host may legitimately decide to start anyway -- the desktop does exactly that for
    /// a key provider that is designed and not built, because a console that refuses to
    /// open protects nobody. What must not happen is that the decision disappears: an
    /// after-action review reading this session has to be able to see that its verdicts
    /// were produced under settings something objected to, and what the objection was.
    pub baseline_objection: Option<String>,
}

/// What is written beside a journal so a session can be reopened.
///
/// **The baseline is part of the record, not a reference to the current one.** A session
/// replayed under a different baseline would be judged by policy settings, asset
/// priorities and authority rules it never ran under, and the verdicts an after-action
/// review read would be ones nobody made. It is stored, not looked up.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MissionRecord {
    pub session: SessionId,
    pub state: MissionState,
    pub config: ConfigBaseline,
    /// See [`Mission::baseline_objection`]. Defaulted when absent so records written
    /// before this field existed load as "nothing objected", which is what they meant.
    #[serde(default)]
    pub baseline_objection: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum MissionError {
    #[error("mission store unavailable: {0}")]
    Store(#[from] gungnir_store::StoreError),
    #[error("config invalid: {0}")]
    Config(#[from] gungnir_config::ConfigError),
    #[error("no mission record for session {0:?}")]
    UnknownMission(SessionId),
    /// The record could not be read or written.
    #[error("mission record I/O failed: {0}")]
    Io(String),
    /// A lifecycle step that would misrepresent what the session is.
    #[error("a mission in {from:?} cannot become {to:?}")]
    IllegalTransition {
        from: MissionState,
        to: MissionState,
    },
}

pub trait MissionManager {
    fn create(&mut self, config: ConfigBaseline) -> Result<Mission, MissionError>;
    fn load(&mut self, session: SessionId) -> Result<Mission, MissionError>;
    fn save(&mut self, mission: &Mission) -> Result<(), MissionError>;
    fn close(&mut self, mission: Mission) -> Result<(), MissionError>;

    /// Move a mission to another state, refusing one that would misrepresent it.
    ///
    /// # Errors
    ///
    /// [`MissionError::IllegalTransition`] for a step the state machine forbids -- above
    /// all, anything that would make a reopened session `Live`.
    fn transition(&mut self, mission: &mut Mission, to: MissionState) -> Result<(), MissionError>;

    /// Every envelope recorded in a session, in the order it was recorded.
    ///
    /// The verification row's criterion is that this is identical to what was recorded,
    /// which is why it returns the sequence rather than a cursor over it.
    fn replay(&self, session: SessionId) -> Result<Vec<Envelope>, MissionError>;

    /// Sessions this store holds a mission record for.
    fn missions(&self) -> Result<Vec<SessionId>, MissionError>;
}

/// The lifecycle over a journal directory.
///
/// A mission record sits beside its journal as `<id>.mission.json`, so a data directory
/// carries both what happened and what it was. The journal remains the record of events;
/// this adds only what the journal cannot say -- which baseline it ran under, and how it
/// ended.
pub struct JournalMissionManager<'a> {
    root: PathBuf,
    journal: &'a dyn EventJournal,
    next: u64,
}

impl std::fmt::Debug for JournalMissionManager<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JournalMissionManager")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

impl<'a> JournalMissionManager<'a> {
    /// # Errors
    ///
    /// When the directory cannot be created.
    pub fn open(
        root: impl Into<PathBuf>,
        journal: &'a dyn EventJournal,
    ) -> Result<Self, MissionError> {
        let root = root.into();
        std::fs::create_dir_all(&root).map_err(|e| MissionError::Io(e.to_string()))?;
        // Continue past the highest identifier **either** store already holds.
        //
        // Records alone are not enough. A data directory whose mission records were
        // deleted while its journals survived would hand a new mission an identifier some
        // journal already has envelopes under, and `replay` would return another
        // session's events under this one's name. The journal is asked too, and an error
        // listing it is propagated rather than defaulted: an identifier allocated without
        // knowing what exists is exactly the collision this is here to prevent.
        let next = Self::recorded(&root)
            .into_iter()
            .chain(journal.sessions()?)
            .map(|s| s.0)
            .max()
            .unwrap_or(0);
        Ok(Self {
            root,
            journal,
            next,
        })
    }

    fn record_path(root: &Path, session: SessionId) -> PathBuf {
        root.join(format!("{}.mission.json", session.0))
    }

    /// Sessions with a record on disk.
    ///
    /// A directory that cannot be read holds no records as far as this can tell, which is
    /// also what a fresh data directory looks like -- so this reports a list, not a
    /// result. `missions()` wraps it because the trait allows a store that can fail.
    fn recorded(root: &Path) -> Vec<SessionId> {
        let Ok(entries) = std::fs::read_dir(root) else {
            return Vec::new();
        };
        let mut sessions = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(id) = name.strip_suffix(".mission.json") {
                if let Ok(id) = id.parse() {
                    sessions.push(SessionId(id));
                }
            }
        }
        sessions.sort_unstable_by_key(|s| s.0);
        sessions
    }

    fn write(&self, record: &MissionRecord) -> Result<(), MissionError> {
        let encoded =
            serde_json::to_vec_pretty(record).map_err(|e| MissionError::Io(e.to_string()))?;
        std::fs::write(Self::record_path(&self.root, record.session), encoded)
            .map_err(|e| MissionError::Io(e.to_string()))
    }

    fn read(&self, session: SessionId) -> Result<MissionRecord, MissionError> {
        let path = Self::record_path(&self.root, session);
        let bytes = std::fs::read(&path).map_err(|_| MissionError::UnknownMission(session))?;
        serde_json::from_slice(&bytes).map_err(|e| MissionError::Io(e.to_string()))
    }
}

impl MissionManager for JournalMissionManager<'_> {
    fn create(&mut self, config: ConfigBaseline) -> Result<Mission, MissionError> {
        // Checked and recorded, not enforced. The host decides whether to start -- the
        // node refuses an invalid baseline before it ever gets here, and the desktop
        // deliberately starts under one and degrades honestly -- and this crate's job is
        // that the decision survives into the record the review reads.
        let baseline_objection = gungnir_config::validate(&config)
            .err()
            .map(|e| e.to_string());
        self.next += 1;
        let mission = Mission {
            session: SessionId(self.next),
            state: MissionState::Created,
            config,
            baseline_objection,
        };
        self.save(&mission)?;
        Ok(mission)
    }

    fn load(&mut self, session: SessionId) -> Result<Mission, MissionError> {
        let record = self.read(session)?;
        // **A reopened session is never live.** One recorded as `Live` was interrupted:
        // the process that was feeding it stopped without closing it, and whatever
        // happened next is not in the journal.
        let state = match record.state {
            MissionState::Live => MissionState::Interrupted,
            other => other,
        };
        Ok(Mission {
            session: record.session,
            state,
            config: record.config,
            baseline_objection: record.baseline_objection,
        })
    }

    fn save(&mut self, mission: &Mission) -> Result<(), MissionError> {
        self.write(&MissionRecord {
            session: mission.session,
            state: mission.state,
            config: mission.config.clone(),
            baseline_objection: mission.baseline_objection.clone(),
        })
    }

    fn close(&mut self, mission: Mission) -> Result<(), MissionError> {
        let mut mission = mission;
        self.transition(&mut mission, MissionState::Closed)?;
        self.save(&mission)
    }

    fn transition(&mut self, mission: &mut Mission, to: MissionState) -> Result<(), MissionError> {
        if !mission.state.can_transition_to(to) {
            return Err(MissionError::IllegalTransition {
                from: mission.state,
                to,
            });
        }
        mission.state = to;
        self.save(mission)
    }

    fn replay(&self, session: SessionId) -> Result<Vec<Envelope>, MissionError> {
        Ok(self.journal.read_session(session)?)
    }

    fn missions(&self) -> Result<Vec<SessionId>, MissionError> {
        Ok(Self::recorded(&self.root))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_eventing::Event;
    use gungnir_model::events::SensorEvent;
    use gungnir_model::{MissionTime, SensorId, SensorMode};
    use gungnir_store::{DurabilityPolicy, FileEventJournal};

    fn dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gungnir-mission-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn envelope(seq: u32) -> Envelope {
        let at = MissionTime(f64::from(seq));
        Envelope {
            seq: u64::from(seq),
            mission_time: at,
            event: Event::Sensor(SensorEvent::ModeChanged {
                sensor: SensorId(1),
                from: SensorMode::Standby,
                to: SensorMode::Search,
                at,
            }),
        }
    }

    /// The verification row's criterion: **the replayed sequence is identical to the
    /// recorded one.**
    #[test]
    fn a_replayed_sequence_is_identical_to_the_recorded_one() {
        let root = dir("replay");
        let mut journal =
            FileEventJournal::open_with_policy(&root, DurabilityPolicy::SyncEveryEnvelope)
                .expect("journal");

        let recorded: Vec<Envelope> = (1..=25).map(envelope).collect();
        let session = SessionId(1);
        for envelope in &recorded {
            journal.append(session, envelope).expect("appended");
        }
        journal.sync().expect("synced");

        let manager = JournalMissionManager::open(&root, &journal).expect("opened");
        assert_eq!(
            manager.replay(session).expect("replayed"),
            recorded,
            "the replayed sequence differs from what was recorded"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// **The rule the whole crate turns on.** A session recorded as live and reopened is
    /// `Interrupted`, never `Live`: the feed that made it live is gone.
    #[test]
    fn a_session_reopened_from_disk_is_never_live() {
        let root = dir("interrupted");
        let journal = FileEventJournal::open(&root).expect("journal");
        let mut manager = JournalMissionManager::open(&root, &journal).expect("opened");

        let mut mission = manager.create(ConfigBaseline::default()).expect("created");
        manager
            .transition(&mut mission, MissionState::Live)
            .expect("went live");
        // The process stops here without closing: the record still says Live.

        let reopened = manager.load(mission.session).expect("loaded");
        assert_eq!(reopened.state, MissionState::Interrupted);
        assert!(!reopened.state.is_recording());
        let _ = std::fs::remove_dir_all(root);
    }

    /// **A journal a mission record does not know about still holds an identifier.**
    /// Allocating over it would give a fresh session another session's events, and the
    /// replay would look like a full recording rather than an empty one.
    #[test]
    fn a_new_mission_does_not_reuse_a_journal_identifier() {
        let root = dir("journal-ids");
        let mut journal =
            FileEventJournal::open_with_policy(&root, DurabilityPolicy::SyncEveryEnvelope)
                .expect("journal");
        // Envelopes under session 7, and no mission record anywhere: the records were
        // cleared, or the journal predates this crate.
        for envelope in (1..=3).map(envelope) {
            journal.append(SessionId(7), &envelope).expect("appended");
        }
        journal.sync().expect("synced");

        let mut manager = JournalMissionManager::open(&root, &journal).expect("opened");
        let mission = manager.create(ConfigBaseline::default()).expect("created");
        assert!(
            mission.session.0 > 7,
            "a new mission took identifier {} which the journal already holds",
            mission.session.0
        );
        // Nothing has been recorded under it yet, so the journal has no such session --
        // which is what it says. The failure this guards against is the other one: a
        // fresh session replaying three envelopes it never recorded.
        match manager.replay(mission.session) {
            Ok(envelopes) => assert!(
                envelopes.is_empty(),
                "a new session replayed {} envelopes it never recorded",
                envelopes.len()
            ),
            Err(MissionError::Store(gungnir_store::StoreError::UnknownSession(id))) => {
                assert_eq!(id, mission.session);
            }
            Err(err) => panic!("replay of a new session failed unexpectedly: {err}"),
        }
        let _ = std::fs::remove_dir_all(root);
    }

    /// An interrupted session may be reviewed and never resumed. Resuming it would leave
    /// a hole in the record that nothing marked.
    #[test]
    fn an_interrupted_session_can_be_replayed_but_not_resumed() {
        assert!(MissionState::Interrupted.can_transition_to(MissionState::Replaying));
        assert!(MissionState::Interrupted.can_transition_to(MissionState::Closed));
        assert!(!MissionState::Interrupted.can_transition_to(MissionState::Live));
        assert!(!MissionState::Closed.can_transition_to(MissionState::Live));
        // A deliberate pause is the one thing that does return to live.
        assert!(MissionState::Paused.can_transition_to(MissionState::Live));
    }

    /// The baseline travels with the mission, so a replay is judged by the settings the
    /// session actually ran under.
    #[test]
    fn the_baseline_is_part_of_the_record() {
        let root = dir("baseline");
        let journal = FileEventJournal::open(&root).expect("journal");
        let mut manager = JournalMissionManager::open(&root, &journal).expect("opened");

        let config = ConfigBaseline {
            allocation_horizon: 42,
            ..ConfigBaseline::default()
        };
        let mission = manager.create(config).expect("created");
        manager.close(mission.clone()).expect("closed");

        let reopened = manager.load(mission.session).expect("loaded");
        assert_eq!(
            reopened.config.allocation_horizon, 42,
            "the mission was reopened under a different baseline than it ran under"
        );
        assert_eq!(reopened.state, MissionState::Closed);
        let _ = std::fs::remove_dir_all(root);
    }

    /// A baseline that does not validate is **recorded as an objection, not refused.**
    ///
    /// Refusing here would decide something the host has already decided: the desktop
    /// deliberately starts under a key provider that is designed and not built, journals
    /// in the clear and says so, because a console that will not open protects nobody.
    /// What this crate owes the review is that the objection is still there afterwards.
    #[test]
    fn a_baseline_that_does_not_validate_is_recorded_rather_than_refused() {
        let root = dir("invalid");
        let journal = FileEventJournal::open(&root).expect("journal");
        let mut manager = JournalMissionManager::open(&root, &journal).expect("opened");

        let invalid = ConfigBaseline {
            allocation_horizon: 10,
            security: gungnir_config::SecurityConfig {
                key_provider: gungnir_config::KeyProviderConfig::OperatingSystemKeystore {
                    account: "gungnir".into(),
                },
                authentication: gungnir_config::AuthenticationConfig::default(),
                tls: gungnir_config::TlsClientConfig::default(),
                escrow: None,
            },
            ..ConfigBaseline::default()
        };
        assert!(
            gungnir_config::validate(&invalid).is_err(),
            "this baseline was supposed to be one that does not validate"
        );

        let mission = manager.create(invalid).expect("created");
        let objection = mission
            .baseline_objection
            .as_deref()
            .expect("a baseline that does not validate was recorded as if it did");
        assert!(objection.contains("key provider"), "{objection}");

        // And it survives to disk, which is where the review reads it.
        let reopened = manager.load(mission.session).expect("loaded");
        assert_eq!(reopened.baseline_objection.as_deref(), Some(objection));
        let _ = std::fs::remove_dir_all(root);
    }

    /// The ordinary case: nothing objected, and the record says so rather than saying
    /// nothing.
    #[test]
    fn a_valid_baseline_records_no_objection() {
        let root = dir("valid");
        let journal = FileEventJournal::open(&root).expect("journal");
        let mut manager = JournalMissionManager::open(&root, &journal).expect("opened");
        let mission = manager.create(ConfigBaseline::default()).expect("created");
        assert!(mission.baseline_objection.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    /// A new mission never takes the identifier of one already on disk, which would
    /// overwrite its record.
    #[test]
    fn a_new_mission_does_not_reuse_a_recorded_identifier() {
        let root = dir("ids");
        let journal = FileEventJournal::open(&root).expect("journal");
        let first;
        {
            let mut manager = JournalMissionManager::open(&root, &journal).expect("opened");
            first = manager
                .create(ConfigBaseline::default())
                .expect("created")
                .session;
            manager.create(ConfigBaseline::default()).expect("created");
        }
        // A second manager over the same directory, as a restart would build.
        let mut manager = JournalMissionManager::open(&root, &journal).expect("reopened");
        let third = manager.create(ConfigBaseline::default()).expect("created");

        assert!(third.session.0 > first.0);
        assert_eq!(manager.missions().expect("listed").len(), 3);
        let _ = std::fs::remove_dir_all(root);
    }

    /// An illegal step is refused rather than silently applied.
    #[test]
    fn an_illegal_transition_is_refused() {
        let root = dir("illegal");
        let journal = FileEventJournal::open(&root).expect("journal");
        let mut manager = JournalMissionManager::open(&root, &journal).expect("opened");

        let mut mission = manager.create(ConfigBaseline::default()).expect("created");
        // Created straight to Replaying: there is nothing recorded to replay.
        let outcome = manager.transition(&mut mission, MissionState::Replaying);
        assert!(matches!(
            outcome,
            Err(MissionError::IllegalTransition { .. })
        ));
        assert_eq!(mission.state, MissionState::Created);
        let _ = std::fs::remove_dir_all(root);
    }

    /// Loading a session nothing recorded says so rather than inventing one.
    #[test]
    fn an_unknown_mission_is_an_error() {
        let root = dir("unknown");
        let journal = FileEventJournal::open(&root).expect("journal");
        let mut manager = JournalMissionManager::open(&root, &journal).expect("opened");
        assert!(matches!(
            manager.load(SessionId(999)),
            Err(MissionError::UnknownMission(_))
        ));
        let _ = std::fs::remove_dir_all(root);
    }
}

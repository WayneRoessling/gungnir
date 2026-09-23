// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Cross-session entity identity on the node (GAP-019, DN-19), over edge (s).
//!
//! # Why this is on the node at all, when the desktop already has one
//!
//! The desktop resolves identity for a panel to draw. The node resolves it for the
//! **record**: `gungnir-store`'s journal on a node is the authoritative account of a
//! mission (`ARCHITECTURE.md` §8.1), and a deployment whose desktops were not connected
//! for part of a watch has no other place the correlation could have been made. Until
//! 2026-09-06 it could not be made here at all: the resolver needs tracks, and the node
//! had none until GAP-011 closed.
//!
//! Edge (s) `gungnir-node` to `gungnir-identity` was drawn and accepted for exactly this.
//! Note that **edge (n)'s recorded reason for putting evidence fusion on the desktop --
//! "the node has no tracks until GAP-011" -- expired the same day**, so that edge now
//! stands on the rest of its argument and this one does not contradict it: identity
//! correlation belongs where the journal is, and evidence fusion belongs where the
//! operator working the picture is.
//!
//! # What is recorded, and what is deliberately not
//!
//! Every resolution is journalled, the joins **and** the mints. A reviewer asking why two
//! sightings were not joined needs the mint as much as the confidence of a join that was
//! made, and an event stream that recorded only the interesting outcome would answer half
//! the question.
//!
//! **The picture is not changed.** `TrackView` carries no `GlobalEntityId` and none was
//! added: putting an entity identity on the snapshot is a change to the canonical model
//! and to what every consumer of a track believes it is holding, and it is not needed to
//! make the record complete. The identity lives in the journal beside the track events it
//! is about.
//!
//! # What this does not do
//!
//! It builds no order-of-battle or pattern-of-life product. Those are `gungnir-reporting`,
//! the node has no edge to it, and one was not added: a second edge to carry products a
//! node has no panel to show would be an edge added for tidiness rather than for a
//! caller. The desktop assembles them, over the same lineages, from its own journal fold.

use std::collections::HashMap;

use gungnir_identity::{IdentityResolver, InMemoryIdentityResolver, Sighting};
use gungnir_model::events::IdentityEvent;
use gungnir_model::identity::GlobalEntityId;
use gungnir_model::{MissionTime, TrackId, TrackView};
use gungnir_store::{EventJournal, SessionId};

/// The node's resolver, folded from its retained sessions at start-up.
pub struct EntityIdentity {
    resolver: InMemoryIdentityResolver,
    /// What each live track has already been resolved to, so a track is journalled once
    /// rather than on every tick. Keyed by the session as well as the track, because a
    /// track number belongs to one session (GAP-123).
    settled: HashMap<Sighting, GlobalEntityId>,
    sessions: Vec<SessionId>,
    /// Why the fold is incomplete, if it is.
    ///
    /// **Said rather than skipped.** A resolver folded from three of five retained
    /// sessions will happily mint a new entity for a track the two unread ones would have
    /// matched, and a reader of the journal would see a new object appear where none did.
    unreadable: Option<String>,
}

impl EntityIdentity {
    /// Fold the retained sessions into a resolver.
    ///
    /// Only `TrackInitiated` and `TrackUpdated` carry state to correlate on; a track
    /// named by identifier alone cannot be attributed and is not pretended to be.
    ///
    /// **The identities this node already recorded are read back, not minted again**
    /// (GAP-123). Every `IdentityEvent` in the session says what a track was resolved to,
    /// so the fold binds each track to the identity it was given and only resolves a track
    /// no identity event names -- a journal written before those events existed, or one
    /// whose identity event was lost. Re-resolving them all, which is what this did until
    /// 2026-09-22, minted a fresh UUID for the same object at every start, so nothing
    /// named in an earlier session could be recognised by its identifier afterwards.
    ///
    /// The identity events of a session are read before its tracking events, in two
    /// passes, because a journal may hold the identity either side of the track update it
    /// is about.
    pub fn recover(journal: &dyn EventJournal, retention_sessions: usize) -> Self {
        let mut out = Self {
            resolver: InMemoryIdentityResolver::new(),
            settled: HashMap::new(),
            sessions: Vec::new(),
            unreadable: None,
        };
        let mut sessions = match journal.sessions() {
            Ok(s) => s,
            Err(err) => {
                out.unreadable = Some(format!("the session list could not be read: {err}"));
                return out;
            }
        };
        sessions.sort_unstable_by_key(|s| s.0);
        let skip = sessions.len().saturating_sub(retention_sessions);
        for session in sessions.into_iter().skip(skip) {
            match journal.read_session(session) {
                Ok(envelopes) => {
                    let recorded = journaled_identities(&envelopes);
                    for envelope in &envelopes {
                        let gungnir_eventing::Event::Tracking(tracking) = &envelope.event else {
                            continue;
                        };
                        if let gungnir_model::events::TrackingEvent::TrackInitiated(track)
                        | gungnir_model::events::TrackingEvent::TrackUpdated(track) = tracking
                        {
                            match recorded.get(&track.id) {
                                Some((entity, correlation)) => out.resolver.restore(
                                    session,
                                    track,
                                    *entity,
                                    correlation.clone(),
                                ),
                                None => {
                                    let _ = out.resolver.resolve(session, track);
                                }
                            }
                        }
                    }
                    out.sessions.push(session);
                }
                Err(err) => {
                    out.unreadable = Some(format!("session {}: {err}", session.0));
                    break;
                }
            }
        }
        out
    }

    /// How many retained sessions were folded in.
    pub fn sessions(&self) -> usize {
        self.sessions.len()
    }

    /// How many entities the resolver holds.
    pub fn entities(&self) -> usize {
        self.resolver.lineages().count()
    }

    /// Why the fold is incomplete, if it is.
    pub fn unreadable(&self) -> Option<&str> {
        self.unreadable.as_deref()
    }

    /// Resolve this tick's tracks, returning the events to journal.
    ///
    /// A track already settled is skipped, so the journal carries one identity event per
    /// track rather than one per tick: an identity is a claim about what a track *is*,
    /// and repeating it every 100 ms would bury the tracking events it sits beside.
    ///
    /// `session` is the live session, which names this tick's track numbers (GAP-123).
    pub fn observe(
        &mut self,
        session: SessionId,
        tracks: &[TrackView],
        now: MissionTime,
    ) -> Vec<IdentityEvent> {
        let mut out = Vec::new();
        for track in tracks {
            let sighting = Sighting::new(session, track.id);
            if self.settled.contains_key(&sighting) {
                continue;
            }
            let entity = self.resolver.resolve(session, track);
            self.settled.insert(sighting, entity);
            // The resolver records a correlation on the lineage when it matched one, and
            // records nothing when it minted. Reading the lineage back is how this tells
            // the two apart without the resolver having to report it twice.
            let matched = self.resolver.lineage(entity).and_then(|lineage| {
                lineage
                    .correlations
                    .iter()
                    .rev()
                    .find(|c| c.sighting == sighting)
                    .map(|c| (c.confidence, c.basis.clone()))
            });
            out.push(match matched {
                Some((confidence, basis)) => IdentityEvent::Correlated {
                    track: track.id,
                    entity,
                    confidence,
                    basis,
                    at: now,
                },
                None => IdentityEvent::Minted {
                    track: track.id,
                    entity,
                    at: now,
                },
            });
        }
        out
    }
}

/// What each track in this session was recorded as, from the session's own identity
/// events (GAP-123): the entity, and the confidence and basis when the record says the
/// track was correlated rather than minted.
///
/// The last event for a track wins, which is the one that stands: a track is journaled
/// once per session, and a second event for it could only be a later correction.
fn journaled_identities(
    envelopes: &[gungnir_eventing::Envelope],
) -> HashMap<TrackId, (GlobalEntityId, Option<(f64, String)>)> {
    let mut out = HashMap::new();
    for envelope in envelopes {
        let gungnir_eventing::Event::Identity(identity) = &envelope.event else {
            continue;
        };
        match identity {
            IdentityEvent::Minted { track, entity, .. } => {
                out.insert(*track, (*entity, None));
            }
            IdentityEvent::Correlated {
                track,
                entity,
                confidence,
                basis,
                ..
            } => {
                out.insert(*track, (*entity, Some((*confidence, basis.clone()))));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::{Classification, Provenance, Quality, Releasability, TrackStatus};

    /// The session every track in these tests belongs to (GAP-123).
    const SESSION: SessionId = SessionId(1);
    use nalgebra::{SMatrix, SVector};

    fn track(id: u64, east: f64, at: f64) -> TrackView {
        TrackView {
            id: TrackId(id),
            status: TrackStatus::Confirmed,
            state: SVector::<f64, 6>::from_column_slice(&[east, 0.0, 100.0, 10.0, 0.0, 0.0]),
            covariance: SMatrix::<f64, 6, 6>::identity() * 25.0,
            classification: Classification::default(),
            provenance: Provenance::default(),
            quality: Quality::default(),
            mission_time: MissionTime(at),
            releasability: Releasability::default(),
        }
    }

    /// GAP-123: the node folds its own journal at start, and the identities it recorded
    /// come back as they were. Re-resolving them, which is what this did until
    /// 2026-09-22, minted a fresh UUID for the same object on every start.
    ///
    /// The second session's track carries the number the tracker restarts at, so this
    /// also holds that a number is not what binds a sighting to an entity.
    #[test]
    fn the_identities_a_session_recorded_are_read_back_not_minted_again() {
        use gungnir_eventing::{Envelope, Event};
        use gungnir_store::FileEventJournal;

        let dir = std::env::temp_dir().join(format!(
            "gungnir-node-identity-{}-{}",
            std::process::id(),
            "recover"
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let mut journal = FileEventJournal::open(dir.clone()).expect("journal");
        let session = SessionId(1);
        let seen = track(3, 1_000.0, 10.0);
        // Whatever the journal recorded is what must come back, so this is written down
        // rather than minted: a literal identifier no resolver here could have produced.
        let entity = GlobalEntityId(0x0193_5c1d_7e00_7abc_9def_0123_4567_89ab);

        let mut seq = 0;
        let mut append = |journal: &mut FileEventJournal, event: Event, at: f64| {
            seq += 1;
            journal
                .append(
                    session,
                    &Envelope {
                        seq,
                        mission_time: MissionTime(at),
                        event,
                    },
                )
                .expect("appended");
        };
        append(
            &mut journal,
            Event::Tracking(gungnir_model::events::TrackingEvent::TrackInitiated(
                seen.clone(),
            )),
            10.0,
        );
        append(
            &mut journal,
            Event::Identity(IdentityEvent::Minted {
                track: seen.id,
                entity,
                at: MissionTime(10.0),
            }),
            10.0,
        );

        let mut recovered = EntityIdentity::recover(&journal, 5);
        assert!(
            recovered.unreadable().is_none(),
            "{:?}",
            recovered.unreadable()
        );
        assert_eq!(
            recovered.resolver.identity_of(session, seen.id),
            Some(entity),
            "the journaled identity was not read back"
        );
        assert_eq!(recovered.entities(), 1);

        // The live session that follows: the tracker counts from zero again, and this is
        // the same object where it was predicted to be. It joins the entity the journal
        // named, on similarity, and no second identity is minted for it.
        let live = SessionId(2);
        // 30 s later, where a 10 m/s track is predicted to be: 1 000 m + 10 m/s * 30 s.
        let again = track(0, 1_300.0, 40.0);
        let events = recovered.observe(live, &[again], MissionTime(40.0));
        assert_eq!(events.len(), 1);
        match &events[0] {
            IdentityEvent::Correlated { entity: e, .. } => assert_eq!(*e, entity),
            other @ IdentityEvent::Minted { .. } => {
                panic!("the restored entity was not recognised: {other:?}")
            }
        }
        assert_eq!(
            recovered.entities(),
            1,
            "a second entity was minted for one object"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_track_nothing_matches_is_recorded_as_minted() {
        let mut identity = EntityIdentity {
            resolver: InMemoryIdentityResolver::new(),
            settled: HashMap::new(),
            sessions: Vec::new(),
            unreadable: None,
        };
        let events = identity.observe(SESSION, &[track(1, 0.0, 10.0)], MissionTime(10.0));
        assert_eq!(events.len(), 1);
        assert!(
            matches!(events[0], IdentityEvent::Minted { .. }),
            "a first sighting must be recorded as minted, not silently dropped: {:?}",
            events[0]
        );
        assert_eq!(identity.entities(), 1);
    }

    /// The property that makes the journal worth writing: one identity event per track,
    /// not one per tick.
    #[test]
    fn a_settled_track_is_not_journalled_again() {
        let mut identity = EntityIdentity {
            resolver: InMemoryIdentityResolver::new(),
            settled: HashMap::new(),
            sessions: Vec::new(),
            unreadable: None,
        };
        let first = identity.observe(SESSION, &[track(1, 0.0, 10.0)], MissionTime(10.0));
        assert_eq!(first.len(), 1);
        for tick in 1..20 {
            let again = identity.observe(
                SESSION,
                &[track(1, 0.0, 10.0 + f64::from(tick))],
                MissionTime(0.0),
            );
            assert!(
                again.is_empty(),
                "tick {tick} journalled the same track's identity again"
            );
        }
    }

    #[test]
    fn every_live_track_is_accounted_for() {
        let mut identity = EntityIdentity {
            resolver: InMemoryIdentityResolver::new(),
            settled: HashMap::new(),
            sessions: Vec::new(),
            unreadable: None,
        };
        let tracks = [
            track(1, 0.0, 10.0),
            track(2, 5000.0, 10.0),
            track(3, 9000.0, 10.0),
        ];
        let events = identity.observe(SESSION, &tracks, MissionTime(10.0));
        assert_eq!(
            events.len(),
            3,
            "a track resolved but not recorded is an identity nobody can review"
        );
    }
}

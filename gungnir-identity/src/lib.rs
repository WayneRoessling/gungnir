// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Identity management & track continuity, per docs/gungnir-capabilities.md
//! §5.3. gungnir-track manages track IDs correctly *within one session*; a
//! multi-system or multi-session deployment needs identity that survives restarts,
//! replays, and external-system correlation -- "is this the same object we saw
//! yesterday" is a question the tracking core alone can't answer.
//!
//! The in-memory resolver here correlates on two things. A track keeps its global id
//! for as long as its session-local id lives, which costs a lookup. When the local id
//! is one it has not seen, it asks whether the track is nonetheless an entity it
//! already knows: [`similarity`] propagates each known entity's last state to the
//! candidate's time and scores the two under both covariances and their
//! classifications, and the best match above the merge threshold takes the candidate
//! (GAP-019). Only a track that matches nothing gets a newly minted id.
//!
//! **A track is named by its session as well as its number** (GAP-123). `TrackId` is a
//! counter `gungnir-track` restarts at zero in every process, so session two's track 0 is
//! not session one's track 0; keying on the number alone handed a restarted track whatever
//! entity happened to hold its number, with no correlation and no record of the join. Every
//! lookup here is by [`Sighting`], the session and the track together, so a match across
//! sessions can only be made by [`similarity`], which records why.
//!
//! **A correlation made that way is recorded, not just made.** Each one lands on the
//! lineage as a [`CorrelationEvent`] with its confidence and the distance, gap and
//! class agreement behind it, because the whole value of cross-session identity is that
//! a reviewer can ask why two sightings were called one thing and get an answer.
//! `docs/design/DN-19-order-of-battle.md` reads those records.

pub mod similarity;

pub use similarity::{CorrelationSettings, LastSeen, Similarity};

use gungnir_model::identity::GlobalEntityId;
use gungnir_model::time::MissionTime;
use gungnir_model::{SessionId, TrackId, TrackView};
use std::collections::HashMap;

/// One session's track: what a lineage is built from, and what every lookup is keyed by.
///
/// The session is half the key because a track number is only unique inside the session
/// that issued it (GAP-123).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Sighting {
    pub session: SessionId,
    pub track: TrackId,
}

impl Sighting {
    #[must_use]
    pub fn new(session: SessionId, track: TrackId) -> Self {
        Self { session, track }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EntityLineage {
    pub global_id: GlobalEntityId,
    /// Every sighting joined to this entity, in the order they were joined.
    pub sightings: Vec<Sighting>,
    pub aliases: Vec<String>,
    pub merge_history: Vec<MergeEvent>,
    /// Every correlation made on similarity rather than on a session id (GAP-019),
    /// with the confidence and the basis, so a reviewer can see why.
    pub correlations: Vec<CorrelationEvent>,
    /// The last state this entity was seen in, for the next correlation.
    pub last_seen: Option<LastSeen>,
}

/// A session-local track taken to be a known entity on similarity (GAP-019).
#[derive(Debug, Clone, PartialEq)]
pub struct CorrelationEvent {
    pub sighting: Sighting,
    pub confidence: f64,
    /// Human-readable: the normalised distance, the gap, and the class agreement.
    pub basis: String,
    pub mission_time: MissionTime,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MergeEvent {
    pub from: GlobalEntityId,
    pub into: GlobalEntityId,
    pub mission_time: MissionTime,
}

pub trait IdentityResolver: Send + Sync {
    /// Correlates a session-local track against known global entities, returning
    /// an existing or newly minted global id.
    ///
    /// `session` is required because a track number belongs to one session (GAP-123).
    fn resolve(&mut self, session: SessionId, track: &TrackView) -> GlobalEntityId;
    fn lineage(&self, id: GlobalEntityId) -> Option<&EntityLineage>;
}

#[derive(Debug, Default)]
pub struct InMemoryIdentityResolver {
    by_sighting: HashMap<Sighting, GlobalEntityId>,
    lineages: HashMap<GlobalEntityId, EntityLineage>,
    /// How similar a new track must be to a known entity to be taken as it (GAP-019).
    settings: CorrelationSettings,
    /// Kept for nothing but the record of how many this resolver has minted.
    ///
    /// **Identities are UUID v7 as of GAP-069**, so this no longer produces them: a
    /// counter is unique within one process and collides the moment two nodes exchange
    /// identities, which is the whole point of a *global* entity id.
    minted: u128,
}

impl InMemoryIdentityResolver {
    /// How many identities this resolver has minted.
    ///
    /// Not an identity source and never was a good one: a counter is unique within one
    /// process and collides the moment two nodes exchange identities (GAP-069).
    #[must_use]
    pub fn minted(&self) -> u128 {
        self.minted
    }

    pub fn new() -> Self {
        Self::default()
    }

    /// The same resolver with other correlation thresholds.
    #[must_use]
    pub fn with_settings(mut self, settings: CorrelationSettings) -> Self {
        self.settings = settings;
        self
    }

    /// Every lineage this resolver holds, in no particular order (GAP-025: the order
    /// of battle is assembled from them).
    pub fn lineages(&self) -> impl Iterator<Item = &EntityLineage> {
        self.lineages.values()
    }

    /// The most similar known entity to `track`, above the merge threshold.
    fn best_match(&self, track: &TrackView) -> Option<(GlobalEntityId, Similarity)> {
        self.lineages
            .values()
            .filter_map(|l| {
                let last = l.last_seen.as_ref()?;
                let s = similarity::similarity(track, last, &self.settings)?;
                (s.confidence >= self.settings.merge_threshold).then_some((l.global_id, s))
            })
            .max_by(|a, b| a.1.confidence.total_cmp(&b.1.confidence))
    }

    /// Record that `from` was determined to be the same object as `into`; `from`'s
    /// session tracks now resolve to `into`.
    pub fn merge(&mut self, from: GlobalEntityId, into: GlobalEntityId, now: MissionTime) -> bool {
        if from == into || !self.lineages.contains_key(&into) {
            return false;
        }
        let Some(old) = self.lineages.remove(&from) else {
            return false;
        };
        for sighting in &old.sightings {
            self.by_sighting.insert(*sighting, into);
        }
        if let Some(target) = self.lineages.get_mut(&into) {
            target.sightings.extend(old.sightings);
            target.aliases.extend(old.aliases);
            target.correlations.extend(old.correlations);
            target.merge_history.push(MergeEvent {
                from,
                into,
                mission_time: now,
            });
        }
        true
    }

    /// Bind a sighting to the identity the journal already recorded for it (GAP-123).
    ///
    /// **This is how an identity survives a restart.** Recovery used to re-resolve every
    /// journaled track, which minted a fresh UUID for the same object on every start, so
    /// nothing an earlier session had named could be recognised afterwards by its
    /// identifier. The node has journaled `IdentityEvent::Minted` and `Correlated` since
    /// GAP-025 and the desktop does now; replaying them puts the identities back exactly
    /// as they were recorded, which is what keeps D-11's UUID v7 and its mint ordering.
    ///
    /// `correlation` carries the confidence and basis of a journaled `Correlated` event,
    /// so a restored lineage can still say why two sightings were joined. `None` restores
    /// a mint, which has no basis to state.
    pub fn restore(
        &mut self,
        session: SessionId,
        track: &TrackView,
        entity: GlobalEntityId,
        correlation: Option<(f64, String)>,
    ) {
        let sighting = Sighting::new(session, track.id);
        self.by_sighting.insert(sighting, entity);
        let lineage = self
            .lineages
            .entry(entity)
            .or_insert_with(|| EntityLineage {
                global_id: entity,
                sightings: Vec::new(),
                aliases: Vec::new(),
                merge_history: Vec::new(),
                correlations: Vec::new(),
                last_seen: None,
            });
        if !lineage.sightings.contains(&sighting) {
            lineage.sightings.push(sighting);
        }
        if let Some((confidence, basis)) = correlation {
            lineage.correlations.push(CorrelationEvent {
                sighting,
                confidence,
                basis,
                mission_time: track.mission_time,
            });
        }
        lineage.last_seen = Some(LastSeen::of(track));
    }

    /// The identity this sighting already has, if it has one.
    #[must_use]
    pub fn identity_of(&self, session: SessionId, track: TrackId) -> Option<GlobalEntityId> {
        self.by_sighting
            .get(&Sighting::new(session, track))
            .copied()
    }

    pub fn add_alias(&mut self, id: GlobalEntityId, alias: impl Into<String>) -> bool {
        match self.lineages.get_mut(&id) {
            Some(l) => {
                l.aliases.push(alias.into());
                true
            }
            None => false,
        }
    }
}

impl IdentityResolver for InMemoryIdentityResolver {
    fn resolve(&mut self, session: SessionId, track: &TrackView) -> GlobalEntityId {
        let sighting = Sighting::new(session, track.id);
        if let Some(id) = self.by_sighting.get(&sighting).copied() {
            if let Some(l) = self.lineages.get_mut(&id) {
                l.last_seen = Some(LastSeen::of(track));
            }
            return id;
        }
        // GAP-019: an unknown session id may still be a known entity.
        if let Some((id, s)) = self.best_match(track) {
            self.by_sighting.insert(sighting, id);
            if let Some(l) = self.lineages.get_mut(&id) {
                l.sightings.push(sighting);
                l.correlations.push(CorrelationEvent {
                    sighting,
                    confidence: s.confidence,
                    basis: format!(
                        "normalised distance² {:.2} over {:.0} s, class {}",
                        s.distance_sq,
                        s.gap_s,
                        if s.class_agrees {
                            "agrees"
                        } else {
                            "disagrees"
                        }
                    ),
                    mission_time: track.mission_time,
                });
                l.last_seen = Some(LastSeen::of(track));
            }
            return id;
        }
        self.minted += 1;
        // v7: a millisecond timestamp in the first 48 bits, so identities sort in the
        // order they were minted -- which is the order an after-action review reads them
        // in, and what keeps an index over them from degenerating.
        let id = GlobalEntityId(uuid::Uuid::now_v7().as_u128());
        self.by_sighting.insert(sighting, id);
        self.lineages.insert(
            id,
            EntityLineage {
                global_id: id,
                sightings: vec![sighting],
                aliases: Vec::new(),
                merge_history: Vec::new(),
                correlations: Vec::new(),
                last_seen: Some(LastSeen::of(track)),
            },
        );
        id
    }

    fn lineage(&self, id: GlobalEntityId) -> Option<&EntityLineage> {
        self.lineages.get(&id)
    }
}

#[cfg(test)]
mod uuid_tests {
    use super::*;
    use gungnir_model::{Provenance, Quality, TrackId, TrackStatus};

    const S1: SessionId = SessionId(1);

    fn track(id: u64) -> TrackView {
        TrackView {
            id: TrackId(id),
            status: TrackStatus::Tentative,
            state: nalgebra::Vector6::zeros(),
            covariance: nalgebra::Matrix6::identity(),
            classification: gungnir_model::Classification::Unknown,
            provenance: Provenance::default(),
            quality: Quality::default(),
            mission_time: MissionTime(0.0),
            releasability: gungnir_model::Releasability::default(),
        }
    }

    /// **A counter is unique in one process and collides the moment two nodes exchange
    /// identities**, which is the whole point of a *global* entity id. Minted ids are
    /// UUID v7 (GAP-069, D-11).
    #[test]
    fn minted_identities_are_uuid_v7() {
        let mut r = InMemoryIdentityResolver::new();
        let id = r.resolve(S1, &track(1));
        assert_eq!(
            id.version(),
            Some(uuid::Version::SortRand),
            "{id} is not a v7 identity"
        );
        assert_ne!(id.0, 1, "the counter is still minting identities");
    }

    /// Two resolvers minting independently -- which is what two nodes are -- do not
    /// collide. Under the counter both would have produced 1.
    #[test]
    fn two_resolvers_do_not_mint_the_same_identity() {
        let mut a = InMemoryIdentityResolver::new();
        let mut b = InMemoryIdentityResolver::new();
        assert_ne!(a.resolve(S1, &track(1)), b.resolve(S1, &track(1)));
    }

    /// v7 sorts by mint time, which is the order an after-action review reads them in.
    #[test]
    fn identities_sort_in_the_order_they_were_minted() {
        let mut r = InMemoryIdentityResolver::new();
        let first = r.resolve(S1, &track(1));
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second = r.resolve(S1, &track(2));
        assert!(first < second, "{first} did not sort before {second}");
    }

    /// The same track resolves to the same identity: minting is per entity, not per call.
    #[test]
    fn resolving_one_track_twice_mints_once() {
        let mut r = InMemoryIdentityResolver::new();
        let first = r.resolve(S1, &track(1));
        assert_eq!(r.resolve(S1, &track(1)), first);
        assert_eq!(r.minted(), 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::{Classification, Provenance, Quality, TrackStatus};

    const S1: SessionId = SessionId(1);
    const S2: SessionId = SessionId(2);

    fn track(id: u64) -> TrackView {
        TrackView {
            id: TrackId(id),
            status: TrackStatus::Confirmed,
            state: nalgebra_zero(),
            covariance: nalgebra_identity(),
            classification: Classification::Unknown,
            provenance: Provenance::default(),
            quality: Quality::default(),
            mission_time: MissionTime(0.0),
            releasability: gungnir_model::Releasability::default(),
        }
    }

    fn nalgebra_zero() -> nalgebra::SVector<f64, 6> {
        nalgebra::SVector::<f64, 6>::zeros()
    }

    fn nalgebra_identity() -> nalgebra::SMatrix<f64, 6, 6> {
        nalgebra::SMatrix::<f64, 6, 6>::identity()
    }

    #[test]
    fn same_track_resolves_to_same_global_id() {
        let mut r = InMemoryIdentityResolver::new();
        let a = r.resolve(S1, &track(1));
        let b = r.resolve(S1, &track(1));
        let c = r.resolve(S1, &track(2));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(
            r.lineage(a).unwrap().sightings,
            vec![Sighting::new(S1, TrackId(1))]
        );
    }

    /// GAP-123: `gungnir-track` restarts its counter at zero in every process, so a
    /// track number says nothing across sessions. Two different objects that happen to
    /// share one are two entities, and the second is not joined to the first by its
    /// number -- only [`similarity`] may join them, and it records why when it does.
    #[test]
    fn a_track_number_reused_in_a_later_session_is_not_the_same_entity() {
        let mut r = InMemoryIdentityResolver::new();
        let mut far_away = track(0);
        far_away.state[0] = 300_000.0;
        far_away.mission_time = MissionTime(5_000.0);

        let first = r.resolve(S1, &track(0));
        let second = r.resolve(S2, &far_away);

        assert_ne!(
            first, second,
            "session two's track 0 inherited session one's entity by its number"
        );
        assert_eq!(
            r.lineage(first).unwrap().sightings,
            vec![Sighting::new(S1, TrackId(0))]
        );
        assert_eq!(
            r.lineage(second).unwrap().sightings,
            vec![Sighting::new(S2, TrackId(0))]
        );
        assert!(
            r.lineage(second).unwrap().correlations.is_empty(),
            "a join was recorded where none was made"
        );
    }

    /// GAP-123: the identities a journal already holds are put back as they were, rather
    /// than minted afresh. A restored entity keeps its identifier, and the basis of a
    /// journaled correlation is restored with it so a reviewer can still ask why.
    #[test]
    fn a_journaled_identity_is_restored_rather_than_minted() {
        let mut first_run = InMemoryIdentityResolver::new();
        let entity = first_run.resolve(S1, &track(4));

        // A later process folds the journal: the same sighting, the same identity.
        let mut recovered = InMemoryIdentityResolver::new();
        recovered.restore(S1, &track(4), entity, None);
        assert_eq!(recovered.identity_of(S1, TrackId(4)), Some(entity));
        assert_eq!(
            recovered.resolve(S1, &track(4)),
            entity,
            "a restored sighting re-minted"
        );
        assert_eq!(
            recovered.minted(),
            0,
            "recovery minted an identity it was given"
        );

        // A journaled correlation restores its basis too.
        recovered.restore(
            S2,
            &track(9),
            entity,
            Some((0.87, "normalised distance 0.4".into())),
        );
        let lineage = recovered.lineage(entity).expect("the restored lineage");
        assert_eq!(
            lineage.sightings,
            vec![Sighting::new(S1, TrackId(4)), Sighting::new(S2, TrackId(9))]
        );
        let correlation = lineage
            .correlations
            .last()
            .expect("the restored correlation");
        assert_eq!(correlation.sighting, Sighting::new(S2, TrackId(9)));
        assert!((correlation.confidence - 0.87).abs() < 1e-9);
        assert_eq!(correlation.basis, "normalised distance 0.4");
    }

    #[test]
    fn merge_redirects_tracks_and_records_history() {
        let mut r = InMemoryIdentityResolver::new();
        let a = r.resolve(S1, &track(1));
        let b = r.resolve(S1, &track(2));
        assert!(r.merge(a, b, MissionTime(5.0)));
        assert_eq!(r.resolve(S1, &track(1)), b);
        let lineage = r.lineage(b).unwrap();
        assert_eq!(
            lineage.sightings,
            vec![Sighting::new(S1, TrackId(2)), Sighting::new(S1, TrackId(1))]
        );
        assert_eq!(lineage.merge_history.len(), 1);
        assert!(r.lineage(a).is_none());
    }
}

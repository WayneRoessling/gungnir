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
//! **A correlation made that way is recorded, not just made.** Each one lands on the
//! lineage as a [`CorrelationEvent`] with its confidence and the distance, gap and
//! class agreement behind it, because the whole value of cross-session identity is that
//! a reviewer can ask why two sightings were called one thing and get an answer.
//! `docs/design/DN-19-order-of-battle.md` reads those records.

pub mod similarity;

pub use similarity::{CorrelationSettings, LastSeen, Similarity};

use gungnir_model::identity::GlobalEntityId;
use gungnir_model::time::MissionTime;
use gungnir_model::{TrackId, TrackView};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct EntityLineage {
    pub global_id: GlobalEntityId,
    pub session_track_ids: Vec<TrackId>,
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
    pub track: TrackId,
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
    fn resolve(&mut self, track: &TrackView) -> GlobalEntityId;
    fn lineage(&self, id: GlobalEntityId) -> Option<&EntityLineage>;
}

#[derive(Debug, Default)]
pub struct InMemoryIdentityResolver {
    by_track: HashMap<TrackId, GlobalEntityId>,
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
        for id in &old.session_track_ids {
            self.by_track.insert(*id, into);
        }
        if let Some(target) = self.lineages.get_mut(&into) {
            target.session_track_ids.extend(old.session_track_ids);
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
    fn resolve(&mut self, track: &TrackView) -> GlobalEntityId {
        if let Some(id) = self.by_track.get(&track.id).copied() {
            if let Some(l) = self.lineages.get_mut(&id) {
                l.last_seen = Some(LastSeen::of(track));
            }
            return id;
        }
        // GAP-019: an unknown session id may still be a known entity.
        if let Some((id, s)) = self.best_match(track) {
            self.by_track.insert(track.id, id);
            if let Some(l) = self.lineages.get_mut(&id) {
                l.session_track_ids.push(track.id);
                l.correlations.push(CorrelationEvent {
                    track: track.id,
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
        self.by_track.insert(track.id, id);
        self.lineages.insert(
            id,
            EntityLineage {
                global_id: id,
                session_track_ids: vec![track.id],
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
        let id = r.resolve(&track(1));
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
        assert_ne!(a.resolve(&track(1)), b.resolve(&track(1)));
    }

    /// v7 sorts by mint time, which is the order an after-action review reads them in.
    #[test]
    fn identities_sort_in_the_order_they_were_minted() {
        let mut r = InMemoryIdentityResolver::new();
        let first = r.resolve(&track(1));
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second = r.resolve(&track(2));
        assert!(first < second, "{first} did not sort before {second}");
    }

    /// The same track resolves to the same identity: minting is per entity, not per call.
    #[test]
    fn resolving_one_track_twice_mints_once() {
        let mut r = InMemoryIdentityResolver::new();
        let first = r.resolve(&track(1));
        assert_eq!(r.resolve(&track(1)), first);
        assert_eq!(r.minted(), 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::{Classification, Provenance, Quality, TrackStatus};

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
        let a = r.resolve(&track(1));
        let b = r.resolve(&track(1));
        let c = r.resolve(&track(2));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(r.lineage(a).unwrap().session_track_ids, vec![TrackId(1)]);
    }

    #[test]
    fn merge_redirects_tracks_and_records_history() {
        let mut r = InMemoryIdentityResolver::new();
        let a = r.resolve(&track(1));
        let b = r.resolve(&track(2));
        assert!(r.merge(a, b, MissionTime(5.0)));
        assert_eq!(r.resolve(&track(1)), b);
        let lineage = r.lineage(b).unwrap();
        assert_eq!(lineage.session_track_ids, vec![TrackId(2), TrackId(1)]);
        assert_eq!(lineage.merge_history.len(), 1);
        assert!(r.lineage(a).is_none());
    }
}

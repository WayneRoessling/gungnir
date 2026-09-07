// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Cross-session identity on the desktop (GAP-019, GAP-025; DN-19).
//!
//! The resolver is built at start from the sessions the journal retains, oldest first,
//! so a track seen this morning is a known entity by the afternoon session; every live
//! track is resolved each frame, which mints an identity for a new one and correlates
//! one that looks like an entity seen before (`gungnir_identity::similarity`, with the
//! confidence and the basis on the lineage). PN-04 shows the lineage; PN-13 folds the
//! retained sessions into DN-19's order of battle and its pattern of life through the
//! same resolver.
//!
//! **The product assembles evidence; the analyst concludes.** Every entry rests on the
//! session and sequence it was seen in, a correlation carries the confidence it was
//! made at, and nothing here decides what a thing *is*.
//!
//! **Coverage is measured, not assumed.** Every track the journal names is counted, and
//! the ones no lineage in the product accounts for are published as
//! `OrderOfBattle::unattributed_tracks`, which is what keeps `attribution_coverage`
//! from reporting a perfect score for an assessment that missed half the picture.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use gungnir_identity::{EntityLineage, IdentityResolver, InMemoryIdentityResolver};
use gungnir_model::events::TrackingEvent;
use gungnir_model::identity::GlobalEntityId;
use gungnir_model::{MissionTime, TrackId};
use gungnir_reporting::order_of_battle::{
    assemble, pattern_of_life as fold_pattern_of_life, EntityEvidence, EntityMovement,
    ObservedLocation, OrderOfBattle, PatternOfLife, PatternSettings, ProductError, Traversal,
};
use gungnir_store::{EventJournal, SessionId};

use crate::state::AppState;

/// One lineage's sightings as the assembler wants them: the lineage, where each session
/// first wrote it, and where it was last seen.
type LineageEvidence = (EntityLineage, Vec<(SessionId, u64)>, ObservedLocation);

/// How far a track has to move before another waypoint is kept for it.
///
/// A tenth of [`ROUTE_CELL_M`], so the sampling does not itself skip a cell the track
/// crossed: a cell is missed only where a single journalled update jumped across it,
/// which is a gap in the picture rather than in the sampling.
const WAYPOINT_SPACING_M: f64 = 25.0;

/// The most waypoints kept for one track before the path is resampled.
///
/// Bounds what the retained sessions cost in memory -- a journal folded at start holds
/// every track of every retained session -- at about 32 KiB per track.
const MAX_WAYPOINTS: usize = 1_024;

/// The cell the pattern of life bins positions into before comparing two paths.
const ROUTE_CELL_M: f64 = 250.0;

/// Where and when a session-local track was recorded, so an entry can trace back, and
/// the path it took, so recurring behaviour can be folded out of it.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackRecord {
    pub session: SessionId,
    pub first_seq: u64,
    pub first_seen: MissionTime,
    pub last_seen: MissionTime,
    /// The sampled path, in the order it was recorded.
    pub waypoints: Vec<(MissionTime, [f64; 3])>,
    /// The spacing the path is currently sampled at, which doubles every time
    /// [`MAX_WAYPOINTS`] is reached.
    pub spacing_m: f64,
}

impl TrackRecord {
    /// Keep this position if the track has moved [`Self::spacing_m`] since the last one
    /// kept.
    ///
    /// At the cap the path is **resampled rather than clipped**: every second waypoint
    /// is dropped and the spacing doubles, so a long journey stays a whole journey at
    /// half the resolution. A clipped path would be a prefix of a real one, and a prefix
    /// compared against whole paths matches nothing -- which would report that a route
    /// nobody drove twice recurred less often than it did. The spacing passes
    /// [`ROUTE_CELL_M`] only after a single object has travelled some two hundred
    /// kilometres inside the retained sessions; beyond that its route is published from
    /// a coarser path and can miss a cell it crossed.
    fn push_waypoint(&mut self, at: MissionTime, position: [f64; 3]) {
        if !position.iter().all(|p| p.is_finite()) {
            return;
        }
        if let Some((_, last)) = self.waypoints.last() {
            let d = [
                position[0] - last[0],
                position[1] - last[1],
                position[2] - last[2],
            ];
            if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] < self.spacing_m * self.spacing_m {
                return;
            }
        }
        self.waypoints.push((at, position));
        if self.waypoints.len() >= MAX_WAYPOINTS {
            let mut n = 0_usize;
            self.waypoints.retain(|_| {
                n += 1;
                n % 2 == 1
            });
            self.spacing_m *= 2.0;
        }
    }
}

/// The resolver and what it was built from.
#[derive(Debug, Default)]
pub struct IdentityState {
    pub resolver: InMemoryIdentityResolver,
    /// Sessions folded at start, oldest first, and the live one once it has tracks.
    pub sessions: Vec<SessionId>,
    /// Why the fold stopped short, if it did: the product then says it is partial.
    pub unreadable: Option<String>,
    pub records: HashMap<TrackId, TrackRecord>,
    /// Every track the journal named, whether or not the resolver could attribute it.
    ///
    /// The denominator of `OrderOfBattle::attribution_coverage`. A track the journal
    /// mentions only by identifier -- coasted or deleted, with no state on the event --
    /// carries nothing to correlate, so it lands here and never in a lineage: that is
    /// what an unattributed track *is*, and counting it is the difference between a
    /// coverage figure and a compliment.
    pub seen_tracks: BTreeSet<TrackId>,
    /// How many order-of-battle versions this session has produced.
    pub versions: u32,
}

impl IdentityState {
    /// Fold the retained sessions of `journal` into a fresh resolver.
    #[must_use]
    pub fn recover(journal: &dyn EventJournal, retention_sessions: usize) -> Self {
        let mut state = Self::default();
        let mut sessions = match journal.sessions() {
            Ok(s) => s,
            Err(err) => {
                state.unreadable = Some(format!("the session list could not be read: {err}"));
                return state;
            }
        };
        sessions.sort_unstable_by_key(|s| s.0);
        let keep = sessions.len().saturating_sub(retention_sessions);
        for session in sessions.into_iter().skip(keep) {
            match journal.read_session(session) {
                Ok(envelopes) => {
                    for e in &envelopes {
                        let gungnir_eventing::Event::Tracking(tracking) = &e.event else {
                            continue;
                        };
                        match tracking {
                            TrackingEvent::TrackInitiated(t) | TrackingEvent::TrackUpdated(t) => {
                                state.observe(session, e.seq, t);
                            }
                            // A track named only by its identifier carries no state to
                            // correlate, so the resolver cannot attribute it. Counted
                            // rather than ignored: an unattributed track nothing counts
                            // is the perfect coverage score this product must never
                            // report.
                            TrackingEvent::TrackCoasting(id) | TrackingEvent::TrackDeleted(id) => {
                                state.seen_tracks.insert(*id);
                            }
                        }
                    }
                    state.sessions.push(session);
                }
                Err(err) => {
                    state.unreadable = Some(format!("session {}: {err}", session.0));
                    break;
                }
            }
        }
        state
    }

    /// One sighting of a track in `session`.
    fn observe(&mut self, session: SessionId, seq: u64, track: &gungnir_model::TrackView) {
        let _ = self.resolver.resolve(track);
        self.seen_tracks.insert(track.id);
        let position = track.position_enu();
        let record = self.records.entry(track.id).or_insert_with(|| TrackRecord {
            session,
            first_seq: seq,
            first_seen: track.mission_time,
            last_seen: track.mission_time,
            waypoints: Vec::new(),
            spacing_m: WAYPOINT_SPACING_M,
        });
        if track.mission_time > record.last_seen {
            record.last_seen = track.mission_time;
        }
        record.push_waypoint(track.mission_time, position);
    }

    /// The lineage a track belongs to.
    #[must_use]
    pub fn lineage_of(&self, track: TrackId) -> Option<&EntityLineage> {
        // `resolve` is the only way to look up by track and it needs a `TrackView`; the
        // lineages are few, so a scan is what a lookup costs here.
        self.resolver
            .lineages()
            .find(|l| l.session_track_ids.contains(&track))
    }
}

/// The tick step: every live track resolved, under the live session.
pub fn tick(state: &mut AppState) {
    let Some(session) = state.session() else {
        return;
    };
    let tracks: Vec<gungnir_model::TrackView> = state.tracking.tracks().to_vec();
    if tracks.is_empty() {
        return;
    }
    if !state.identity.sessions.contains(&session) {
        state.identity.sessions.push(session);
    }
    for t in &tracks {
        // The live picture has no envelope sequence; the record carries the journal's
        // once the session is folded at the next start.
        state.identity.observe(session, 0, t);
    }
}

/// PN-04's lineage lines for a track, owned so the view can borrow them.
#[derive(Debug, Clone, PartialEq)]
pub struct OwnedLineage {
    pub session: u64,
    pub local_track: u64,
    pub basis: String,
}

#[must_use]
pub fn lineage_lines(state: &AppState, track: TrackId) -> Vec<OwnedLineage> {
    let Some(lineage) = state.identity.lineage_of(track) else {
        return Vec::new();
    };
    lineage
        .session_track_ids
        .iter()
        .map(|id| {
            let session = state.identity.records.get(id).map_or(0, |r| r.session.0);
            let basis = lineage
                .correlations
                .iter()
                .find(|c| c.track == *id)
                .map_or_else(
                    || "session track id".to_string(),
                    |c| format!("similarity {:.2}: {}", c.confidence, c.basis),
                );
            OwnedLineage {
                session,
                local_track: id.0,
                basis,
            }
        })
        .collect()
}

/// DN-19's product over the retained sessions, versioned per generation.
///
/// The unattributed count is the tracks the desktop has seen that **no entry in this
/// product accounts for**: tracks the journal named without state to correlate, and
/// tracks whose lineage was dropped because nothing was recorded of where or when it
/// was seen. It was hard-coded to zero until 2026-09-06, which made
/// `attribution_coverage` report perfect coverage for every product the desktop had
/// ever assembled. When `IdentityState::unreadable` is set the fold stopped short and
/// the count is a floor rather than the whole of what was missed, which is why the
/// caveat is drawn beside it.
///
/// # Errors
///
/// [`ProductError::NoSessions`] before any session has tracks.
pub fn order_of_battle(state: &mut AppState) -> Result<OrderOfBattle, ProductError> {
    let now = state.clock.now();
    let retention = state.config.reporting.retention_sessions;
    let identity = &mut state.identity;
    identity.versions += 1;
    let version = identity.versions;
    let lineages: Vec<EntityLineage> = identity.resolver.lineages().cloned().collect();
    let evidence: Vec<LineageEvidence> = lineages
        .into_iter()
        .filter_map(|l| {
            let records: Vec<&TrackRecord> = l
                .session_track_ids
                .iter()
                .filter_map(|id| identity.records.get(id))
                .collect();
            let first_seen =
                records
                    .iter()
                    .map(|r| r.first_seen)
                    .reduce(|a, b| if b.0 < a.0 { b } else { a })?;
            let last_seen =
                records
                    .iter()
                    .map(|r| r.last_seen)
                    .reduce(|a, b| if b.0 > a.0 { b } else { a })?;
            let position_enu = l.last_seen.as_ref().map_or([0.0; 3], |s| s.position_enu);
            let sources = records.iter().map(|r| (r.session, r.first_seq)).collect();
            Some((
                l,
                sources,
                ObservedLocation {
                    position_enu,
                    first_seen,
                    last_seen,
                    sightings: u32::try_from(records.len()).unwrap_or(u32::MAX),
                },
            ))
        })
        .collect();
    let borrowed: Vec<EntityEvidence<'_>> = evidence
        .iter()
        .map(|(l, s, loc)| (l, s.clone(), *loc))
        .collect();
    let attributed: BTreeSet<TrackId> = borrowed
        .iter()
        .flat_map(|(l, _, _)| l.session_track_ids.iter().copied())
        .collect();
    let unattributed = u32::try_from(
        identity
            .seen_tracks
            .iter()
            .filter(|id| !attributed.contains(id))
            .count(),
    )
    .unwrap_or(u32::MAX);
    assemble(
        version,
        now,
        &identity.sessions,
        &borrowed,
        unattributed,
        retention.max(1),
    )
}

/// DN-19's pattern of life over the same retained sessions (GAP-025).
///
/// One traversal per entity per session: every session-local track the resolver
/// attributed to the entity contributes the waypoints recorded for it, and the producer
/// walks them in mission-time order. The settings are the desktop's own
/// ([`ROUTE_CELL_M`], twice travelled, three cells long) rather than the analyst's,
/// because nothing in PN-13 offers them to be set yet; when it does they belong in the
/// reporting baseline beside `retention_sessions`.
///
/// This is deliberately **not** versioned the way the order of battle is. A pattern of
/// life is a reading of the same retained sessions rather than an assessment somebody
/// signs, so there is no history of it to argue with and no version to argue about.
///
/// # Errors
///
/// [`ProductError::NoSessions`] before any session has tracks.
pub fn pattern_of_life(state: &AppState) -> Result<PatternOfLife, ProductError> {
    let identity = &state.identity;
    let movements: Vec<EntityMovement<'_>> = identity
        .resolver
        .lineages()
        .map(|lineage| {
            let mut by_session: BTreeMap<SessionId, Vec<(MissionTime, [f64; 3])>> = BTreeMap::new();
            for id in &lineage.session_track_ids {
                let Some(record) = identity.records.get(id) else {
                    continue;
                };
                by_session
                    .entry(record.session)
                    .or_default()
                    .extend(record.waypoints.iter().copied());
            }
            let traversals = by_session
                .into_iter()
                .map(|(session, points)| Traversal { session, points })
                .collect();
            (lineage, traversals)
        })
        .collect();
    fold_pattern_of_life(
        &identity.sessions,
        &movements,
        PatternSettings {
            route_cell_m: ROUTE_CELL_M,
            ..PatternSettings::default()
        },
        state.config.reporting.retention_sessions.max(1),
    )
}

/// The identities of the live tracks, for a caller that wants the global id.
#[must_use]
pub fn global_id(state: &AppState, track: TrackId) -> Option<GlobalEntityId> {
    state.identity.lineage_of(track).map(|l| l.global_id)
}

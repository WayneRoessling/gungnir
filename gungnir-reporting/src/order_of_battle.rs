//! Cross-session products: the order of battle and pattern of life.
//!
//! Design: docs/design/DN-19-order-of-battle.md. Capability CAP-2.12; mission thread
//! MT-08 step 5, where the intelligence analyst assesses what was seen and updates
//! the order of battle. Today priors for the next laydown come from somebody's
//! memory.
//!
//! This crate gained an edge to `gungnir-identity` for it, accepted by the
//! engineering reviewer on 2026-09-05 and drawn in ARCHITECTURE.md §7.1. An order of
//! battle is a list of **entities**, not of tracks, and only `gungnir-identity` knows
//! that: the lineage is what makes an entry defensible when an analyst asks why two
//! sightings are one thing.
//!
//! **The design stance, in one sentence: the product assembles evidence; the analyst
//! concludes.** The resolver correlates an unknown session track id against the last
//! state of each known entity and records the confidence and the basis it did so on
//! (`gungnir_identity::similarity`, GAP-019). An entry is still built from entities the
//! resolver actually merged and no more, the unattributed count is published so the
//! analyst sees the coverage of the assessment, and nothing beyond a recorded
//! correlation is guessed: a merge the resolver did not make is the analyst's to make,
//! with an assessment attached. An automatic merge with no recorded basis would produce
//! a confident order of battle nobody can audit.

use std::collections::{BTreeMap, BTreeSet};

use gungnir_identity::EntityLineage;
use gungnir_model::identity::GlobalEntityId;
use gungnir_model::MissionTime;
use gungnir_store::SessionId;

/// Where an entity was seen, and how recently.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ObservedLocation {
    pub position_enu: [f64; 3],
    pub first_seen: MissionTime,
    pub last_seen: MissionTime,
    pub sightings: u32,
}

/// One entry in the order of battle.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OrderOfBattleEntry {
    pub entity: GlobalEntityId,
    pub label: String,
    pub class: Option<String>,
    pub locations: Vec<ObservedLocation>,
    pub first_seen: MissionTime,
    pub last_seen: MissionTime,
    /// Sightings behind this entry.
    ///
    /// A single-sighting entry and a hundred-sighting entry must never look the
    /// same, so the count is published rather than implied by the list length.
    pub sighting_count: u32,
    /// Sessions and envelope sequences this entry rests on, so any claim traces
    /// back. An entry that cannot be traced is an opinion with a version number.
    pub sources: Vec<(SessionId, u64)>,
    /// Set by the analyst, never by the query.
    pub assessment: Option<String>,
}

/// A versioned assessment of what is out there, built from many sessions.
///
/// Versioned rather than mutable: an order of battle that changes without a history
/// cannot be argued with.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OrderOfBattle {
    pub version: u32,
    pub produced: MissionTime,
    pub sources: Vec<SessionId>,
    pub entries: Vec<OrderOfBattleEntry>,
    /// Tracks the query could **not** attribute to an entity.
    ///
    /// Published so the analyst sees the coverage of the assessment rather than
    /// assuming it is complete. With GAP-019 open this is expected to be non-zero.
    pub unattributed_tracks: u32,
}

impl OrderOfBattle {
    /// Fraction of observed tracks this assessment accounts for, 0.0 to 1.0.
    ///
    /// `None` when nothing was observed at all, which is different from complete
    /// coverage of nothing.
    pub fn attribution_coverage(&self) -> Option<f64> {
        let attributed: u32 = self.entries.iter().map(|e| e.sighting_count).sum();
        let total = attributed + self.unattributed_tracks;
        (total > 0).then(|| f64::from(attributed) / f64::from(total))
    }

    /// True when every entry names at least one source.
    pub fn is_traceable(&self) -> bool {
        self.entries.iter().all(|e| !e.sources.is_empty())
    }
}

/// Recurring behaviour in one area: activity by hour, and the routes observed.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PatternOfLife {
    /// Histogram of activity by hour of day, over the sessions queried.
    pub by_hour: [u32; 24],
    pub routes: Vec<Vec<[f64; 3]>>,
    /// Sessions queried, and sessions in which anything was seen.
    ///
    /// **Both are published.** "Seen on six occasions" means different things out
    /// of six sessions and out of six hundred, and a histogram without its
    /// denominator is the most common way an activity chart misleads.
    pub sessions_covered: u32,
    pub sessions_with_activity: u32,
}

impl PatternOfLife {
    /// Fraction of sessions in which anything was seen; `None` when none were
    /// queried.
    pub fn activity_rate(&self) -> Option<f64> {
        (self.sessions_covered > 0)
            .then(|| f64::from(self.sessions_with_activity) / f64::from(self.sessions_covered))
    }

    /// The busiest hour and its count, or `None` when nothing was seen.
    pub fn busiest_hour(&self) -> Option<(usize, u32)> {
        self.by_hour
            .iter()
            .copied()
            .enumerate()
            .max_by_key(|(_, count)| *count)
            .filter(|(_, count)| *count > 0)
    }
}

/// One entity's evidence: its lineage, the sessions and sequences it rests on, and
/// where it was seen. Named so the assembly signature stays readable.
pub type EntityEvidence<'a> = (&'a EntityLineage, Vec<(SessionId, u64)>, ObservedLocation);

/// One entity's movement through one session: the positions it was recorded at.
///
/// Per session because a session is one continuous observation: two runs down the same
/// road inside one session are two traversals of it, and the same road on two days is
/// two more. The producer counts how often a path was travelled and publishes the ones
/// that recurred; it does not decide that two traversals were the same journey.
#[derive(Debug, Clone, PartialEq)]
pub struct Traversal {
    pub session: SessionId,
    /// The recorded positions with the mission time of each. Order does not matter:
    /// the producer walks them in mission-time order, because an out-of-sequence
    /// measurement arriving late does not mean the entity doubled back.
    pub points: Vec<(MissionTime, [f64; 3])>,
}

/// One entity's movement evidence: its lineage and every traversal attributed to it.
/// Named so the producer's signature stays readable, as [`EntityEvidence`] is for
/// [`assemble`].
pub type EntityMovement<'a> = (&'a EntityLineage, Vec<Traversal>);

/// How [`pattern_of_life`] folds traversals into recurring behaviour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PatternSettings {
    /// The side in metres of the cell a position is binned into before two paths are
    /// compared. Two traversals are the same route when they cross the same cells in
    /// the same order; comparing positions instead would find no pattern anywhere,
    /// because the same road driven twice never produces the same floating-point track.
    pub route_cell_m: f64,
    /// How many traversals of a path make it a route. **Below two nothing is a
    /// pattern:** a path travelled once is a track history, and drawing it as recurring
    /// behaviour is exactly the confident, unauditable claim DN-19 exists to prevent.
    pub min_traversals: u32,
    /// How many cells a path has to cross to be a route at all, so an entity that sat
    /// in one place for an hour does not become a route through one cell.
    pub min_route_cells: usize,
}

impl Default for PatternSettings {
    /// A quarter-kilometre cell, twice travelled, three cells long.
    ///
    /// The cell is coarse on purpose: a road is not a line, and a cell narrower than
    /// the position error of the sensors that saw it would split one route into several
    /// that each fall short of `min_traversals` -- which reports no pattern where there
    /// is one, the failure that is hardest to notice.
    fn default() -> Self {
        Self {
            route_cell_m: 250.0,
            min_traversals: 2,
            min_route_cells: 3,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProductError {
    #[error("no sessions were queried")]
    NoSessions,
    #[error("session {0:?} is outside the retention window")]
    OutsideRetention(SessionId),
    #[error("the route cell size must be a positive, finite number of metres")]
    InvalidRouteCell,
    #[error("a path travelled once is a track history, not a pattern: a route needs at least two traversals")]
    SingleTraversal,
}

/// Builds an order of battle from lineages the resolver actually produced.
///
/// **It does not guess.** Two similar tracks in different sessions stay two entries;
/// the analyst may merge them with a recorded assessment. `unattributed` is the
/// count of tracks no lineage claimed, and it is published rather than hidden.
pub fn assemble(
    version: u32,
    produced: MissionTime,
    sessions: &[SessionId],
    lineages: &[EntityEvidence<'_>],
    unattributed_tracks: u32,
    retention_sessions: usize,
) -> Result<OrderOfBattle, ProductError> {
    if sessions.is_empty() {
        return Err(ProductError::NoSessions);
    }
    if sessions.len() > retention_sessions {
        // The query may not reach further back than the deployment retains.
        return Err(ProductError::OutsideRetention(sessions[retention_sessions]));
    }
    let entries = lineages
        .iter()
        .map(|(lineage, sources, location)| OrderOfBattleEntry {
            entity: lineage.global_id,
            label: lineage
                .aliases
                .first()
                .cloned()
                .unwrap_or_else(|| format!("entity-{:?}", lineage.global_id)),
            class: None,
            locations: vec![*location],
            first_seen: location.first_seen,
            last_seen: location.last_seen,
            // One sighting per session track the resolver attributed.
            sighting_count: u32::try_from(lineage.session_track_ids.len()).unwrap_or(u32::MAX),
            sources: sources.clone(),
            assessment: None,
        })
        .collect();
    Ok(OrderOfBattle {
        version,
        produced,
        sources: sessions.to_vec(),
        entries,
        unattributed_tracks,
    })
}

/// The hour of day a mission time falls in, or `None` when it is not a finite number
/// of seconds.
///
/// Mission time is Unix seconds in the live profiles and whatever the journal recorded
/// under replay (`gungnir_model::time`), so this is a division and not a calendar: it
/// is the UTC hour for a session clocked from the wall, and an hour counted from the
/// scenario's own zero for one clocked from a replay. **The caller knows which clock it
/// fed in and this does not guess**, which is why the histogram is labelled where the
/// product is drawn rather than here.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn hour_of_day(at: MissionTime) -> Option<usize> {
    if !at.0.is_finite() {
        return None;
    }
    // `rem_euclid` rather than `%`: a time before the epoch is a negative number of
    // seconds, and `%` would produce a negative hour for it.
    let hour = (at.0 / 3_600.0).floor().rem_euclid(24.0);
    // Guarded rather than assumed, because an index of 24 would panic and the guard
    // costs a comparison. Inside it the cast can neither truncate nor lose a sign.
    (0.0..24.0).contains(&hour).then_some(hour as usize)
}

/// The grid cell a position falls in, or `None` for a position that is not finite.
#[allow(clippy::cast_possible_truncation)]
fn cell_of(position: [f64; 3], cell_m: f64) -> Option<[i64; 3]> {
    let mut cell = [0_i64; 3];
    for (out, p) in cell.iter_mut().zip(position) {
        if !p.is_finite() {
            return None;
        }
        // The cast saturates rather than wrapping, so a position far enough out for the
        // range of `i64` cells to matter lands in the extreme cell rather than beside
        // the origin. Nothing on a route is out that far.
        *out = (p / cell_m).floor() as i64;
    }
    Some(cell)
}

/// The cells one traversal crossed, in the order it crossed them.
///
/// Consecutive repeats are collapsed, so how long an entity spent in a cell does not
/// change the shape of its route; a position that is not finite is dropped, because the
/// cells either side of it are still crossed in the same order.
fn route_cells(points: &[(MissionTime, [f64; 3])], cell_m: f64) -> Vec<[i64; 3]> {
    let mut ordered: Vec<&(MissionTime, [f64; 3])> = points.iter().collect();
    ordered.sort_by(|a, b| a.0 .0.total_cmp(&b.0 .0));
    let mut cells: Vec<[i64; 3]> = Vec::new();
    for (_, position) in ordered {
        let Some(cell) = cell_of(*position, cell_m) else {
            continue;
        };
        if cells.last() != Some(&cell) {
            cells.push(cell);
        }
    }
    cells
}

/// The centre of each cell on a route.
///
/// The published polyline is the centre line of the cells the traversals shared rather
/// than any one traversal's own positions: the pattern is the path they have in common,
/// and electing one traversal to stand for the rest would publish its noise as the
/// route.
#[allow(clippy::cast_precision_loss)]
fn route_centres(cells: &[[i64; 3]], cell_m: f64) -> Vec<[f64; 3]> {
    // The cast is lossless for every cell index a finite position can produce at any
    // sane cell size; an index beyond 2^53 would need a position outside the observable
    // universe in metres.
    cells
        .iter()
        .map(|c| c.map(|i| (i as f64 + 0.5) * cell_m))
        .collect()
}

/// Folds the movement of resolved entities into recurring behaviour (GAP-025, DN-19).
///
/// **The product assembles evidence; the analyst concludes**, the same stance
/// [`assemble`] takes, and two things follow from it.
///
/// A path is published as a route only once it has been travelled
/// [`PatternSettings::min_traversals`] times. A path travelled once is a track history,
/// and a track history drawn as a pattern of life is a claim about what an adversary
/// habitually does made from a single observation.
///
/// The histogram counts **one per entity per session per hour**, not one per recorded
/// position. A position count measures how often the sensors revisited, which is a fact
/// about the collection plan rather than about the activity, and an entity parked under
/// a staring radar would otherwise dominate every chart it appeared in. Entities are
/// counted by the lineage's global id, so the same entity offered twice in `movements`
/// is one entity and an entity seen in two sessions is counted in both.
///
/// Evidence from a session outside `sessions` is not folded in at all -- not into the
/// hours, the routes, or the activity count. The denominator the product publishes has
/// to be the set of sessions it actually covers, and a duplicate in `sessions` is one
/// session for the same reason.
///
/// # Errors
///
/// [`ProductError::NoSessions`] when nothing was queried; [`ProductError::OutsideRetention`]
/// when the query reaches further back than the deployment retains;
/// [`ProductError::InvalidRouteCell`] and [`ProductError::SingleTraversal`] when the
/// settings ask for a pattern that cannot be one.
pub fn pattern_of_life(
    sessions: &[SessionId],
    movements: &[EntityMovement<'_>],
    settings: PatternSettings,
    retention_sessions: usize,
) -> Result<PatternOfLife, ProductError> {
    if sessions.is_empty() {
        return Err(ProductError::NoSessions);
    }
    if sessions.len() > retention_sessions {
        // The query may not reach further back than the deployment retains.
        return Err(ProductError::OutsideRetention(sessions[retention_sessions]));
    }
    if !(settings.route_cell_m.is_finite() && settings.route_cell_m > 0.0) {
        return Err(ProductError::InvalidRouteCell);
    }
    if settings.min_traversals < 2 {
        return Err(ProductError::SingleTraversal);
    }
    let queried: BTreeSet<SessionId> = sessions.iter().copied().collect();
    let mut hours: BTreeSet<(GlobalEntityId, SessionId, usize)> = BTreeSet::new();
    let mut active: BTreeSet<SessionId> = BTreeSet::new();
    let mut paths: BTreeMap<Vec<[i64; 3]>, u32> = BTreeMap::new();
    for (lineage, traversals) in movements {
        for traversal in traversals
            .iter()
            .filter(|t| queried.contains(&t.session) && !t.points.is_empty())
        {
            active.insert(traversal.session);
            for (at, _) in &traversal.points {
                if let Some(hour) = hour_of_day(*at) {
                    hours.insert((lineage.global_id, traversal.session, hour));
                }
            }
            let cells = route_cells(&traversal.points, settings.route_cell_m);
            if cells.len() >= settings.min_route_cells {
                *paths.entry(cells).or_default() += 1;
            }
        }
    }
    let mut by_hour = [0_u32; 24];
    for (_, _, hour) in hours {
        by_hour[hour] = by_hour[hour].saturating_add(1);
    }
    let mut recurring: Vec<(u32, Vec<[i64; 3]>)> = paths
        .into_iter()
        .filter(|(_, count)| *count >= settings.min_traversals)
        .map(|(cells, count)| (count, cells))
        .collect();
    // Most travelled first, the cell sequence breaking a tie: the same evidence has to
    // produce the same product every time, or two versions of it cannot be compared.
    recurring.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    Ok(PatternOfLife {
        by_hour,
        routes: recurring
            .iter()
            .map(|(_, cells)| route_centres(cells, settings.route_cell_m))
            .collect(),
        sessions_covered: u32::try_from(queried.len()).unwrap_or(u32::MAX),
        sessions_with_activity: u32::try_from(active.len()).unwrap_or(u32::MAX),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::TrackId;

    fn lineage(id: u128, tracks: usize, alias: Option<&str>) -> EntityLineage {
        EntityLineage {
            global_id: GlobalEntityId(id),
            session_track_ids: (0..tracks).map(|n| TrackId(n as u64)).collect(),
            aliases: alias.map(|a| vec![a.to_string()]).unwrap_or_default(),
            merge_history: Vec::new(),
            correlations: Vec::new(),
            last_seen: None,
        }
    }

    fn location() -> ObservedLocation {
        ObservedLocation {
            position_enu: [1.0, 2.0, 0.0],
            first_seen: MissionTime(10.0),
            last_seen: MissionTime(90.0),
            sightings: 3,
        }
    }

    #[test]
    fn a_query_over_no_sessions_is_an_error_not_an_empty_product() {
        let err = assemble(1, MissionTime(0.0), &[], &[], 0, 10).expect_err("no sessions");
        assert_eq!(err, ProductError::NoSessions);
    }

    #[test]
    fn a_query_beyond_retention_is_refused() {
        let sessions: Vec<_> = (0..12).map(SessionId).collect();
        let err =
            assemble(1, MissionTime(0.0), &sessions, &[], 0, 10).expect_err("beyond retention");
        assert!(matches!(err, ProductError::OutsideRetention(_)));
    }

    #[test]
    fn every_entry_traces_to_the_sessions_that_produced_it() {
        let l = lineage(1, 3, Some("battery-a"));
        let sources = vec![(SessionId(1), 42), (SessionId(2), 77)];
        let product = assemble(
            1,
            MissionTime(0.0),
            &[SessionId(1), SessionId(2)],
            &[(&l, sources.clone(), location())],
            0,
            10,
        )
        .expect("assembles");
        assert!(product.is_traceable());
        assert_eq!(product.entries[0].sources, sources);
        assert_eq!(product.entries[0].label, "battery-a");
    }

    #[test]
    fn the_unattributed_count_is_published_and_shapes_the_coverage() {
        let l = lineage(1, 3, None);
        let product = assemble(
            1,
            MissionTime(0.0),
            &[SessionId(1)],
            &[(&l, vec![(SessionId(1), 1)], location())],
            7,
            10,
        )
        .expect("assembles");
        assert_eq!(product.unattributed_tracks, 7);
        let coverage = product.attribution_coverage().expect("something was seen");
        assert!(
            (coverage - 0.3).abs() < 1e-9,
            "three of ten attributed: {coverage}"
        );
    }

    #[test]
    fn coverage_is_none_when_nothing_was_observed() {
        let product =
            assemble(1, MissionTime(0.0), &[SessionId(1)], &[], 0, 10).expect("assembles");
        assert!(
            product.attribution_coverage().is_none(),
            "nothing seen is not complete coverage of nothing"
        );
    }

    #[test]
    fn a_single_sighting_entry_does_not_look_like_a_hundred_sighting_one() {
        let one = lineage(1, 1, None);
        let many = lineage(2, 40, None);
        let product = assemble(
            1,
            MissionTime(0.0),
            &[SessionId(1)],
            &[
                (&one, vec![(SessionId(1), 1)], location()),
                (&many, vec![(SessionId(1), 2)], location()),
            ],
            0,
            10,
        )
        .expect("assembles");
        assert_eq!(product.entries[0].sighting_count, 1);
        assert_eq!(product.entries[1].sighting_count, 40);
    }

    #[test]
    fn nothing_is_merged_that_the_resolver_did_not_merge() {
        // Two lineages the resolver kept apart stay two entries. The analyst may
        // merge them with a recorded assessment; the query never does.
        let a = lineage(1, 2, None);
        let b = lineage(2, 2, None);
        let product = assemble(
            1,
            MissionTime(0.0),
            &[SessionId(1)],
            &[
                (&a, vec![(SessionId(1), 1)], location()),
                (&b, vec![(SessionId(1), 2)], location()),
            ],
            0,
            10,
        )
        .expect("assembles");
        assert_eq!(product.entries.len(), 2);
        assert!(product.entries.iter().all(|e| e.assessment.is_none()));
    }

    #[test]
    fn a_pattern_of_life_always_publishes_its_denominator() {
        let mut by_hour = [0_u32; 24];
        by_hour[3] = 5;
        by_hour[14] = 11;
        let p = PatternOfLife {
            by_hour,
            routes: Vec::new(),
            sessions_covered: 40,
            sessions_with_activity: 6,
        };
        assert_eq!(p.busiest_hour(), Some((14, 11)));
        let rate = p.activity_rate().expect("sessions were queried");
        assert!((rate - 0.15).abs() < 1e-9, "six of forty: {rate}");
    }

    #[test]
    fn an_empty_pattern_reports_no_busiest_hour_and_no_rate() {
        let p = PatternOfLife {
            by_hour: [0; 24],
            routes: Vec::new(),
            sessions_covered: 0,
            sessions_with_activity: 0,
        };
        assert!(p.busiest_hour().is_none());
        assert!(p.activity_rate().is_none());
    }

    /// A run east along one road, `points` positions 100 m apart, starting at `at`.
    fn road(session: u64, at: f64, east0: f64, points: usize) -> Traversal {
        Traversal {
            session: SessionId(session),
            points: (0..points)
                .map(|n| {
                    #[allow(clippy::cast_precision_loss)]
                    let n = n as f64;
                    (MissionTime(at + n * 10.0), [east0 + n * 100.0, 0.0, 0.0])
                })
                .collect(),
        }
    }

    /// The same road driven backwards.
    fn road_reversed(session: u64, at: f64, east0: f64, points: usize) -> Traversal {
        let mut t = road(session, at, east0, points);
        let times: Vec<MissionTime> = t.points.iter().map(|(at, _)| *at).collect();
        t.points.reverse();
        for (point, at) in t.points.iter_mut().zip(times) {
            point.0 = at;
        }
        t
    }

    fn movements(lineage: &EntityLineage, traversals: Vec<Traversal>) -> Vec<EntityMovement<'_>> {
        vec![(lineage, traversals)]
    }

    #[test]
    fn a_path_travelled_once_is_a_track_history_and_not_a_route() {
        let l = lineage(1, 1, None);
        let p = pattern_of_life(
            &[SessionId(1)],
            &movements(&l, vec![road(1, 0.0, 0.0, 10)]),
            PatternSettings::default(),
            10,
        )
        .expect("folds");
        assert!(
            p.routes.is_empty(),
            "one traversal became a pattern: {:?}",
            p.routes
        );
    }

    #[test]
    fn a_path_travelled_twice_is_one_route_on_the_cells_the_two_shared() {
        let l = lineage(1, 2, None);
        // The second run is offset by 40 m, well inside the 250 m cell, so the two are
        // the same road rather than two roads.
        let p = pattern_of_life(
            &[SessionId(1), SessionId(2)],
            &movements(&l, vec![road(1, 0.0, 0.0, 10), road(2, 86_400.0, 40.0, 10)]),
            PatternSettings::default(),
            10,
        )
        .expect("folds");
        assert_eq!(p.routes.len(), 1, "{:?}", p.routes);
        // Ten positions 100 m apart cross four 250 m cells, published at their centres.
        assert_eq!(p.routes[0].len(), 4, "{:?}", p.routes[0]);
        assert!(
            (p.routes[0][0][0] - 125.0).abs() < 1e-9,
            "the route is the cell centre line, not one traversal: {:?}",
            p.routes[0]
        );
    }

    #[test]
    fn the_same_road_driven_the_other_way_is_not_the_same_behaviour() {
        let l = lineage(1, 2, None);
        let p = pattern_of_life(
            &[SessionId(1), SessionId(2)],
            &movements(
                &l,
                vec![road(1, 0.0, 0.0, 10), road_reversed(2, 86_400.0, 0.0, 10)],
            ),
            PatternSettings::default(),
            10,
        )
        .expect("folds");
        assert!(
            p.routes.is_empty(),
            "an approach and a withdrawal were counted as one route: {:?}",
            p.routes
        );
    }

    #[test]
    fn an_entity_that_did_not_move_is_not_a_route() {
        let l = lineage(1, 2, None);
        let parked = |session: u64, at: f64| Traversal {
            session: SessionId(session),
            points: (0..50)
                .map(|n| (MissionTime(at + f64::from(n)), [10.0, 20.0, 0.0]))
                .collect(),
        };
        let p = pattern_of_life(
            &[SessionId(1), SessionId(2)],
            &movements(&l, vec![parked(1, 0.0), parked(2, 86_400.0)]),
            PatternSettings::default(),
            10,
        )
        .expect("folds");
        assert!(p.routes.is_empty(), "{:?}", p.routes);
        assert_eq!(p.sessions_with_activity, 2, "it was still seen twice");
    }

    #[test]
    fn the_histogram_counts_an_entity_once_an_hour_however_often_it_was_sampled() {
        let l = lineage(1, 1, None);
        // Fifty sightings inside the fourth hour: one entity, one hour, one count.
        let dense = Traversal {
            session: SessionId(1),
            points: (0..50)
                .map(|n| {
                    (
                        MissionTime(10_800.0 + f64::from(n)),
                        [f64::from(n) * 5.0, 0.0, 0.0],
                    )
                })
                .collect(),
        };
        let p = pattern_of_life(
            &[SessionId(1)],
            &movements(&l, vec![dense]),
            PatternSettings::default(),
            10,
        )
        .expect("folds");
        assert_eq!(
            p.busiest_hour(),
            Some((3, 1)),
            "the sample rate was counted as activity: {:?}",
            p.by_hour
        );
    }

    #[test]
    fn two_entities_in_one_hour_are_two_and_one_entity_in_two_sessions_is_two() {
        let a = lineage(1, 1, None);
        let b = lineage(2, 1, None);
        let at_hour_three = |session: u64| Traversal {
            session: SessionId(session),
            points: vec![(MissionTime(10_800.0), [0.0, 0.0, 0.0])],
        };
        let p = pattern_of_life(
            &[SessionId(1), SessionId(2)],
            &[
                (&a, vec![at_hour_three(1), at_hour_three(2)]),
                (&b, vec![at_hour_three(1)]),
            ],
            PatternSettings::default(),
            10,
        )
        .expect("folds");
        assert_eq!(p.busiest_hour(), Some((3, 3)), "{:?}", p.by_hour);
    }

    #[test]
    fn the_same_entity_offered_twice_is_one_entity() {
        let l = lineage(1, 1, None);
        let seen = Traversal {
            session: SessionId(1),
            points: vec![(MissionTime(10_800.0), [0.0, 0.0, 0.0])],
        };
        let p = pattern_of_life(
            &[SessionId(1)],
            &[(&l, vec![seen.clone()]), (&l, vec![seen])],
            PatternSettings::default(),
            10,
        )
        .expect("folds");
        assert_eq!(p.busiest_hour(), Some((3, 1)), "{:?}", p.by_hour);
    }

    #[test]
    fn a_session_outside_the_query_is_not_folded_into_any_figure() {
        let l = lineage(1, 2, None);
        let p = pattern_of_life(
            &[SessionId(1)],
            &movements(&l, vec![road(1, 0.0, 0.0, 10), road(9, 86_400.0, 40.0, 10)]),
            PatternSettings::default(),
            10,
        )
        .expect("folds");
        assert_eq!(p.sessions_covered, 1);
        assert_eq!(p.sessions_with_activity, 1);
        assert!(
            p.routes.is_empty(),
            "a traversal nobody queried made a pattern: {:?}",
            p.routes
        );
    }

    #[test]
    fn a_session_named_twice_in_the_query_is_one_session() {
        let l = lineage(1, 1, None);
        let p = pattern_of_life(
            &[SessionId(1), SessionId(1)],
            &movements(
                &l,
                vec![Traversal {
                    session: SessionId(1),
                    points: vec![(MissionTime(0.0), [0.0, 0.0, 0.0])],
                }],
            ),
            PatternSettings::default(),
            10,
        )
        .expect("folds");
        assert_eq!(p.sessions_covered, 1);
        let rate = p.activity_rate().expect("sessions were queried");
        assert!(
            (rate - 1.0).abs() < 1e-9,
            "a repeated session inflated the denominator: {rate}"
        );
    }

    #[test]
    fn the_denominator_is_every_session_queried_not_only_the_busy_ones() {
        let l = lineage(1, 1, None);
        let sessions: Vec<_> = (1..=8).map(SessionId).collect();
        let p = pattern_of_life(
            &sessions,
            &movements(
                &l,
                vec![Traversal {
                    session: SessionId(3),
                    points: vec![(MissionTime(0.0), [0.0, 0.0, 0.0])],
                }],
            ),
            PatternSettings::default(),
            10,
        )
        .expect("folds");
        assert_eq!(p.sessions_covered, 8);
        assert_eq!(p.sessions_with_activity, 1);
        let rate = p.activity_rate().expect("sessions were queried");
        assert!((rate - 0.125).abs() < 1e-9, "one of eight: {rate}");
    }

    #[test]
    fn a_pattern_over_no_sessions_or_beyond_retention_is_an_error() {
        assert_eq!(
            pattern_of_life(&[], &[], PatternSettings::default(), 10).expect_err("no sessions"),
            ProductError::NoSessions
        );
        let sessions: Vec<_> = (0..12).map(SessionId).collect();
        assert!(matches!(
            pattern_of_life(&sessions, &[], PatternSettings::default(), 10)
                .expect_err("beyond retention"),
            ProductError::OutsideRetention(_)
        ));
    }

    #[test]
    fn settings_that_cannot_describe_a_pattern_are_refused() {
        let settings = |cell: f64, min: u32| PatternSettings {
            route_cell_m: cell,
            min_traversals: min,
            ..PatternSettings::default()
        };
        for cell in [0.0, -250.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                pattern_of_life(&[SessionId(1)], &[], settings(cell, 2), 10).expect_err("refused"),
                ProductError::InvalidRouteCell,
                "cell {cell}"
            );
        }
        assert_eq!(
            pattern_of_life(&[SessionId(1)], &[], settings(250.0, 1), 10).expect_err("refused"),
            ProductError::SingleTraversal
        );
    }

    #[test]
    fn the_busiest_route_is_published_first_and_the_order_does_not_depend_on_the_input() {
        let l = lineage(1, 6, None);
        // One road driven three times, another twice.
        let mut traversals = vec![
            road(1, 0.0, 0.0, 10),
            road(2, 86_400.0, 20.0, 10),
            road(3, 172_800.0, 40.0, 10),
            road(4, 0.0, 100_000.0, 10),
            road(5, 86_400.0, 100_020.0, 10),
        ];
        let sessions: Vec<_> = (1..=5).map(SessionId).collect();
        let first = pattern_of_life(
            &sessions,
            &movements(&l, traversals.clone()),
            PatternSettings::default(),
            10,
        )
        .expect("folds");
        assert_eq!(first.routes.len(), 2);
        assert!(
            first.routes[0][0][0] < 1_000.0,
            "the road driven three times is not first: {:?}",
            first.routes
        );
        traversals.reverse();
        let second = pattern_of_life(
            &sessions,
            &movements(&l, traversals),
            PatternSettings::default(),
            10,
        )
        .expect("folds");
        assert_eq!(first, second, "the same evidence produced two products");
    }

    #[test]
    fn an_hour_before_the_epoch_or_off_the_number_line_does_not_index_out_of_range() {
        assert_eq!(hour_of_day(MissionTime(0.0)), Some(0));
        assert_eq!(hour_of_day(MissionTime(10_800.0)), Some(3));
        // 23:00 the day before the epoch, not hour -1.
        assert_eq!(hour_of_day(MissionTime(-3_600.0)), Some(23));
        assert_eq!(hour_of_day(MissionTime(f64::NAN)), None);
        assert_eq!(hour_of_day(MissionTime(f64::INFINITY)), None);
        // Absurd but finite: the remainder is exact, so it is still an hour of a day
        // and still an index this array can hold.
        assert!(hour_of_day(MissionTime(f64::MAX)).is_some_and(|h| h < 24));
    }

    #[test]
    fn a_position_that_is_not_a_position_is_dropped_rather_than_binned() {
        assert!(cell_of([f64::NAN, 0.0, 0.0], 250.0).is_none());
        assert_eq!(cell_of([-1.0, 0.0, 251.0], 250.0), Some([-1, 0, 1]));
        let l = lineage(1, 2, None);
        let with_hole = |session: u64, at: f64| Traversal {
            session: SessionId(session),
            points: vec![
                (MissionTime(at), [0.0, 0.0, 0.0]),
                (MissionTime(at + 1.0), [f64::NAN, 0.0, 0.0]),
                (MissionTime(at + 2.0), [300.0, 0.0, 0.0]),
                (MissionTime(at + 3.0), [600.0, 0.0, 0.0]),
            ],
        };
        let p = pattern_of_life(
            &[SessionId(1), SessionId(2)],
            &movements(&l, vec![with_hole(1, 0.0), with_hole(2, 86_400.0)]),
            PatternSettings::default(),
            10,
        )
        .expect("folds");
        assert_eq!(p.routes.len(), 1, "{:?}", p.routes);
        assert_eq!(p.routes[0].len(), 3, "{:?}", p.routes[0]);
    }

    #[test]
    fn points_recorded_out_of_sequence_do_not_double_the_route_back() {
        let l = lineage(1, 2, None);
        let shuffled = |session: u64, at: f64| {
            let mut t = road(session, at, 0.0, 10);
            t.points.swap(2, 7);
            t
        };
        let ordered = pattern_of_life(
            &[SessionId(1), SessionId(2)],
            &movements(&l, vec![road(1, 0.0, 0.0, 10), road(2, 86_400.0, 0.0, 10)]),
            PatternSettings::default(),
            10,
        )
        .expect("folds");
        let out_of_sequence = pattern_of_life(
            &[SessionId(1), SessionId(2)],
            &movements(&l, vec![shuffled(1, 0.0), shuffled(2, 86_400.0)]),
            PatternSettings::default(),
            10,
        )
        .expect("folds");
        assert_eq!(ordered.routes, out_of_sequence.routes);
    }
}

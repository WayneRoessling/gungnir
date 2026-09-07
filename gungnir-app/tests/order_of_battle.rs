//! Cross-session identity and the order of battle (GAP-019, GAP-025; DN-19): a track
//! seen in one session is recognised in the next by similarity, the lineage says on
//! what basis, and the product over both sessions rests on the journal's own
//! sequences.

use gungnir_app::state::AppState;
use gungnir_app::{identity, update};
use gungnir_config::ConfigBaseline;
use gungnir_eventing::Event;
use gungnir_model::events::TrackingEvent;
use gungnir_model::{
    Classification, DetectionView, MissionTime, Provenance, Quality, Releasability, TrackId,
    TrackStatus, TrackView,
};
use gungnir_time::ReplayClockAuthority;
use gungnir_tracking_service::{SubmitError, TrackingService};

struct Picture(Vec<TrackView>);
impl TrackingService for Picture {
    fn submit_detection(&mut self, _: DetectionView) -> Result<(), SubmitError> {
        Ok(())
    }
    fn poll(&mut self, _: MissionTime) {}
    fn tracks(&self) -> &[TrackView] {
        &self.0
    }
    fn is_healthy(&self) -> bool {
        true
    }
}

fn track(id: u64, e: f64, ve: f64, at: f64) -> TrackView {
    TrackView {
        id: TrackId(id),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::<f64, 6>::new(e, 0.0, 100.0, ve, 0.0, 0.0),
        covariance: nalgebra::SMatrix::<f64, 6, 6>::identity() * 100.0,
        classification: Classification::Unknown,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(at),
        releasability: Releasability::default(),
    }
}

fn desktop(dir: &std::path::Path, at: f64) -> AppState {
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("valid");
    let mut state = AppState::with_config(config).expect("starts");
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(at),
    });
    state
}

#[test]
fn a_track_seen_in_an_earlier_session_is_recognised_and_the_product_spans_both() {
    let dir = std::env::temp_dir().join(format!("gungnir-oob-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    // Session one: two tracks on the record, one of them moving east at 10 m/s.
    let first_session;
    {
        let mut state = desktop(&dir, 100.0);
        first_session = state.session().expect("session");
        let moving = track(1, 1_000.0, 10.0, 100.0);
        let parked = track(2, -5_000.0, 0.0, 100.0);
        state.tracking = Box::new(Picture(vec![moving.clone(), parked.clone()]));
        update::publish(
            &mut state,
            MissionTime(100.0),
            Event::Tracking(TrackingEvent::TrackInitiated(moving)),
        );
        update::publish(
            &mut state,
            MissionTime(100.0),
            Event::Tracking(TrackingEvent::TrackInitiated(parked)),
        );
        update::tick(&mut state);
        assert_eq!(identity::lineage_lines(&state, TrackId(1)).len(), 1);
        let product = identity::order_of_battle(&mut state).expect("a product");
        assert_eq!(product.version, 1);
        assert_eq!(product.entries.len(), 2);
        assert!(product.entries.iter().all(|e| e.sighting_count == 1));
        state.save_session().expect("saved");
    }

    // Session two, thirty seconds later: a new session-local id where the moving track
    // was predicted to be. The resolver takes it to be the same entity and says why.
    let mut state = desktop(&dir, 130.0);
    assert!(
        state.identity.sessions.contains(&first_session),
        "{:?}",
        state.identity.sessions
    );
    assert!(
        state.identity.unreadable.is_none(),
        "{:?}",
        state.identity.unreadable
    );
    let same = track(7, 1_300.0, 10.0, 130.0);
    state.tracking = Box::new(Picture(vec![same]));
    update::tick(&mut state);
    let lines = identity::lineage_lines(&state, TrackId(7));
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines
        .iter()
        .any(|l| l.session == first_session.0 && l.local_track == 1));
    assert!(
        lines.iter().any(|l| l.basis.starts_with("similarity")),
        "{lines:?}"
    );
    let product = identity::order_of_battle(&mut state).expect("a product");
    assert_eq!(
        product.entries.len(),
        2,
        "the parked track stays its own entry"
    );
    let entity = product
        .entries
        .iter()
        .find(|e| e.sighting_count == 2)
        .expect("the entity seen in both sessions");
    let sessions: std::collections::BTreeSet<u64> =
        entity.sources.iter().map(|(s, _)| s.0).collect();
    assert_eq!(sessions.len(), 2, "{:?}", entity.sources);
    // The earlier sighting traces to the journal's own sequence for that session.
    assert!(
        entity.sources.iter().any(|(s, _)| *s == first_session),
        "{:?}",
        entity.sources
    );
    assert_eq!(product.unattributed_tracks, 0);
    let _ = std::fs::remove_dir_all(dir);
}

/// One sighting of a track at a position, so a road can be laid down.
fn at_point(id: u64, position: [f64; 3], at: f64) -> TrackView {
    TrackView {
        id: TrackId(id),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::<f64, 6>::new(
            position[0],
            position[1],
            position[2],
            0.0,
            0.0,
            0.0,
        ),
        covariance: nalgebra::SMatrix::<f64, 6, 6>::identity() * 100.0,
        classification: Classification::Unknown,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(at),
        releasability: Releasability::default(),
    }
}

/// Journal a track driving `points` metres east from `east0`, 200 m at a time.
fn drive_east(state: &mut AppState, id: u64, north: f64, from: f64, points: u32) {
    for n in 0..points {
        let view = at_point(
            id,
            [f64::from(n) * 200.0, north, 100.0],
            from + f64::from(n) * 10.0,
        );
        let event = if n == 0 {
            TrackingEvent::TrackInitiated(view.clone())
        } else {
            TrackingEvent::TrackUpdated(view.clone())
        };
        update::publish(state, view.mission_time, Event::Tracking(event));
    }
}

/// **The coverage figure is measured, not assumed** (GAP-019, DN-19). The desktop used
/// to hand the assembler a hard-coded zero, so every order of battle it had ever
/// produced reported perfect attribution.
#[test]
fn a_track_no_lineage_attributes_is_counted_against_the_coverage() {
    let dir = std::env::temp_dir().join(format!("gungnir-oob-cover-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    {
        let mut state = desktop(&dir, 100.0);
        let seen = track(1, 0.0, 0.0, 100.0);
        update::publish(
            &mut state,
            MissionTime(100.0),
            Event::Tracking(TrackingEvent::TrackInitiated(seen)),
        );
        // Named by identifier alone: nothing on the event to correlate, so no lineage
        // can claim it.
        update::publish(
            &mut state,
            MissionTime(110.0),
            Event::Tracking(TrackingEvent::TrackDeleted(TrackId(99))),
        );
        state.save_session().expect("saved");
    }

    let mut state = desktop(&dir, 200.0);
    let product = identity::order_of_battle(&mut state).expect("a product");
    assert_eq!(product.entries.len(), 1);
    assert_eq!(
        product.unattributed_tracks, 1,
        "the deleted track was not counted: {:?}",
        product.entries
    );
    let coverage = product.attribution_coverage().expect("something was seen");
    assert!(
        (coverage - 0.5).abs() < 1e-9,
        "one of two attributed, not a perfect score: {coverage}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// GAP-025: a road driven in two sessions is recurring behaviour; one driven once is a
/// track history and is not published as a pattern.
#[test]
fn a_road_travelled_in_two_sessions_is_a_route_and_one_travelled_once_is_not() {
    let dir = std::env::temp_dir().join(format!("gungnir-pol-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    // Session one, in the first hour: one track down the road, one down a different
    // road 50 km north that nothing ever repeats.
    {
        let mut state = desktop(&dir, 100.0);
        drive_east(&mut state, 1, 0.0, 100.0, 6);
        drive_east(&mut state, 2, 50_000.0, 100.0, 6);
        state.save_session().expect("saved");
    }
    // Session two, in the fourth hour of the next day: a different session-local track
    // down the same road.
    {
        let mut state = desktop(&dir, 97_200.0);
        drive_east(&mut state, 11, 0.0, 97_200.0, 6);
        state.save_session().expect("saved");
    }

    let state = desktop(&dir, 200_000.0);
    assert!(
        state.identity.unreadable.is_none(),
        "{:?}",
        state.identity.unreadable
    );
    let pattern = identity::pattern_of_life(&state).expect("a pattern");
    assert_eq!(pattern.sessions_covered, 2);
    assert_eq!(pattern.sessions_with_activity, 2);
    assert_eq!(
        pattern.routes.len(),
        1,
        "the road driven once was published as a pattern: {:?}",
        pattern.routes
    );
    assert!(
        pattern.routes[0].len() >= 3,
        "a route has to cross more than a cell or two: {:?}",
        pattern.routes[0]
    );
    // Two entities in the first hour, one in the fourth: the histogram counts entities
    // and not the twenty-four sightings behind them.
    assert_eq!(pattern.by_hour[0], 2, "{:?}", pattern.by_hour);
    assert_eq!(pattern.by_hour[3], 1, "{:?}", pattern.by_hour);
    assert_eq!(pattern.busiest_hour(), Some((0, 2)));
    let rate = pattern.activity_rate().expect("sessions were queried");
    assert!((rate - 1.0).abs() < 1e-9, "{rate}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_desktop_with_no_tracks_has_no_pattern_rather_than_an_empty_one() {
    let dir = std::env::temp_dir().join(format!("gungnir-pol-empty-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let state = desktop(&dir, 1.0);
    assert!(
        identity::pattern_of_life(&state).is_err(),
        "a desktop that has seen nothing published a pattern of no activity"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_desktop_with_no_tracks_has_no_product_rather_than_an_empty_one() {
    let dir = std::env::temp_dir().join(format!("gungnir-oob-empty-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut state = desktop(&dir, 1.0);
    assert!(identity::order_of_battle(&mut state).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

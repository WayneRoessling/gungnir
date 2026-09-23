// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! GAP-123, CAP-2.7: an object keeps its `GlobalEntityId` across a restart and a replay.
//!
//! Both binaries used to rebuild the resolver from the journal at start and mint a fresh
//! UUID v7 as they folded it, so the same object carried a different identity after every
//! restart and in every replay -- the continuity CAP-2.7 exists for, absent. The
//! identities are journaled now and read back, which is what these tests hold.
//!
//! They also hold the defect found alongside it: `gungnir-track` restarts its track
//! counter at zero in every process, so a track number means nothing across sessions, and
//! keying identity on the number alone handed a restarted track whatever entity happened
//! to hold its number -- silently, with no correlation recorded.

use gungnir_app::state::AppState;
use gungnir_app::{identity, update};
use gungnir_config::ConfigBaseline;
use gungnir_eventing::Event;
use gungnir_model::events::TrackingEvent;
use gungnir_model::identity::GlobalEntityId;
use gungnir_model::{
    Classification, MissionTime, Provenance, Quality, Releasability, TrackId, TrackStatus,
    TrackView,
};
use gungnir_time::ReplayClockAuthority;
use gungnir_tracking_service::{SubmitError, TrackingService};

/// A fixed picture: the tracking service the desktop reads this frame.
struct Picture(Vec<TrackView>);

impl TrackingService for Picture {
    fn submit_detection(&mut self, _: gungnir_model::DetectionView) -> Result<(), SubmitError> {
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

fn track(id: u64, east: f64, ve: f64, at: f64) -> TrackView {
    let mut state = nalgebra::Vector6::zeros();
    state[0] = east;
    state[3] = ve;
    TrackView {
        id: TrackId(id),
        status: TrackStatus::Confirmed,
        state,
        covariance: nalgebra::Matrix6::identity() * 100.0,
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
    let mut state = AppState::with_config(config).expect("the desktop starts");
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(at),
    });
    state
}

/// Put one track on the record and resolve it, the way a session does.
fn record(state: &mut AppState, view: &TrackView) -> GlobalEntityId {
    state.tracking = Box::new(Picture(vec![view.clone()]));
    update::publish(
        state,
        view.mission_time,
        Event::Tracking(TrackingEvent::TrackInitiated(view.clone())),
    );
    update::tick(state);
    identity::global_id(state, view.id).expect("the track was resolved to an entity")
}

/// The gap's own criterion: one object, two sessions with a restart between them, and a
/// replay of both -- the same identity in all three.
#[test]
fn one_object_keeps_its_identity_across_a_restart_and_a_replay() {
    let dir = std::env::temp_dir().join(format!("gungnir-identity-restart-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    // Session one: the object is seen, minted, and the identity goes on the record.
    let first = {
        let mut state = desktop(&dir, 100.0);
        let id = record(&mut state, &track(1, 1_000.0, 10.0, 100.0));
        state.save_session().expect("saved");
        id
    };

    // Session two, after a restart: the tracker counts from zero again, so this is track
    // 0 where session one's track 1 was predicted to be. The same object, so the same
    // identity -- joined by similarity, which says why, and not by a track number.
    let second = {
        let mut state = desktop(&dir, 130.0);
        let id = record(&mut state, &track(0, 1_300.0, 10.0, 130.0));
        assert_eq!(
            id, first,
            "the object was minted a second identity after a restart"
        );
        let lines = identity::lineage_lines(&state, TrackId(0));
        assert_eq!(lines.len(), 2, "the lineage lost a session: {lines:?}");
        assert!(
            lines.iter().any(|l| l.basis.starts_with("similarity")),
            "the join across the restart was not made on similarity: {lines:?}"
        );
        state.save_session().expect("saved");
        id
    };

    // A replay: a third start folds both sessions and reads the identities back. Each
    // sighting keeps the identity its session recorded.
    let replay = desktop(&dir, 200.0);
    assert!(
        replay.identity.unreadable.is_none(),
        "{:?}",
        replay.identity.unreadable
    );
    let entities: std::collections::BTreeSet<GlobalEntityId> = replay
        .identity
        .resolver
        .lineages()
        .map(|l| l.global_id)
        .collect();
    assert_eq!(
        entities,
        [first].into_iter().collect(),
        "the replay holds identities the sessions never recorded"
    );
    assert_eq!(second, first);
    let _ = std::fs::remove_dir_all(dir);
}

/// The defect found alongside GAP-123: a track number reused after a restart is not the
/// same object, and must not inherit the earlier entity. Here session two's track 1 is a
/// different object 300 km away.
#[test]
fn a_reused_track_number_does_not_inherit_the_earlier_entity() {
    let dir = std::env::temp_dir().join(format!("gungnir-identity-reuse-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let first = {
        let mut state = desktop(&dir, 100.0);
        let id = record(&mut state, &track(1, 1_000.0, 10.0, 100.0));
        state.save_session().expect("saved");
        id
    };

    let mut state = desktop(&dir, 5_000.0);
    let second = record(&mut state, &track(1, 300_000.0, 0.0, 5_000.0));
    assert_ne!(
        second, first,
        "a different object inherited the earlier entity by its track number"
    );
    let lines = identity::lineage_lines(&state, TrackId(1));
    assert_eq!(
        lines.len(),
        1,
        "the unrelated object was joined to the earlier lineage: {lines:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

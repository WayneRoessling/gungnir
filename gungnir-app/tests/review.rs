//! The after-action review reaches PN-13 (GAP-049, DN-20 §8).
//!
//! The workflow's rules have their own tests. These are the wiring's: a finding recorded
//! under replay carries the cursor's mission time and seeks the replay back to it; the
//! review concludes, closes and promotes through the desktop; every state change is on the
//! bus with the operator who made it, `None` said as `None`.

use gungnir_app::review;
use gungnir_app::state::AppState;
use gungnir_app::sustainment::SustainmentState;
use gungnir_app::update;
use gungnir_config::ConfigBaseline;
use gungnir_eventing::Event;
use gungnir_model::events::{ReviewEvent, TrackingEvent};
use gungnir_model::{
    Classification, MissionTime, Provenance, Quality, Releasability, TrackId, TrackStatus,
    TrackView,
};
use gungnir_time::ReplayClockAuthority;
use gungnir_ui::panels::reports::{FindingKindView, ReviewAction};
use gungnir_workflow::ReviewState;

fn desktop(name: &str) -> (AppState, SustainmentState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gungnir-review-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    };
    let mut state = AppState::with_config(config).expect("the desktop starts");
    // Something to replay: a track initiated at each of five mission seconds.
    for t in 1..=5_u32 {
        state.clock = Box::new(ReplayClockAuthority {
            current: MissionTime(f64::from(t) * 10.0),
        });
        let track = TrackView {
            id: TrackId(u64::from(t)),
            status: TrackStatus::Confirmed,
            state: nalgebra::SVector::zeros(),
            covariance: nalgebra::SMatrix::identity(),
            classification: Classification::Unknown,
            provenance: Provenance::default(),
            quality: Quality::default(),
            mission_time: MissionTime(f64::from(t) * 10.0),
            releasability: Releasability::default(),
        };
        state
            .events
            .publish(
                MissionTime(f64::from(t) * 10.0),
                Event::Tracking(TrackingEvent::TrackInitiated(track)),
            )
            .expect("publish");
        update::tick(&mut state);
    }
    let mut sustainment = SustainmentState::default();
    let session = state.session().expect("a session is open");
    sustainment
        .replay
        .open(&state, session)
        .expect("the live session replays");
    (state, sustainment, dir)
}

fn review_events(
    state: &AppState,
    since: &gungnir_eventing::Receiver<gungnir_eventing::Envelope>,
) -> Vec<ReviewEvent> {
    let _ = state;
    since
        .try_iter()
        .filter_map(|e| match e.event {
            Event::Review(r) => Some(r),
            _ => None,
        })
        .collect()
}

/// **A finding records the moment and seeks back to it** (DN-20 §5, §8): recorded at
/// the replay cursor, it carries that mission time; after the cursor moves away, seeking
/// to the finding puts it back.
#[test]
fn a_finding_records_the_replay_moment_and_seeks_back_to_it() {
    let (mut state, mut sustainment, dir) = desktop("seek");
    review::apply(&mut state, &mut sustainment, ReviewAction::Open);
    assert!(sustainment.review.is_some(), "{:?}", state.alerts);

    sustainment.replay.step();
    sustainment.replay.step();
    let at = review::replay_clock(&sustainment).expect("replay open");
    assert!(at.0 > 0.0);
    review::apply(
        &mut state,
        &mut sustainment,
        ReviewAction::RecordFinding {
            summary: "the second track was late".into(),
            kind: FindingKindView::SystemBehaviour,
        },
    );
    let case = sustainment.review.as_ref().expect("open");
    assert_eq!(case.findings.len(), 1);
    assert_eq!(case.findings[0].at, Some(at));

    sustainment.replay.seek_fraction(1.0);
    assert_ne!(review::replay_clock(&sustainment), Some(at));
    review::apply(&mut state, &mut sustainment, ReviewAction::SeekTo(1));
    assert_eq!(review::replay_clock(&sustainment), Some(at));
    let _ = std::fs::remove_dir_all(dir);
}

/// **Every state change is journalled with an operator** (DN-20 §8), which with nobody
/// signed in is `None` on the record rather than a name nobody typed; and a promoted
/// finding carries its gap.
#[test]
fn every_state_change_reaches_the_bus_with_its_operator() {
    let (mut state, mut sustainment, dir) = desktop("bus");
    let events = state.events.subscribe();
    review::apply(&mut state, &mut sustainment, ReviewAction::Open);
    review::apply(
        &mut state,
        &mut sustainment,
        ReviewAction::RecordFinding {
            summary: "the queue got behind".into(),
            kind: FindingKindView::SystemBehaviour,
        },
    );
    review::apply(
        &mut state,
        &mut sustainment,
        ReviewAction::Promote {
            finding: 1,
            gap: "GAP-034".into(),
        },
    );
    review::apply(&mut state, &mut sustainment, ReviewAction::Conclude);
    review::apply(&mut state, &mut sustainment, ReviewAction::Close);
    assert_eq!(
        sustainment.review.as_ref().map(|r| r.state),
        Some(ReviewState::Closed),
        "{:?}",
        state.alerts
    );

    let published = review_events(&state, &events);
    let kinds: Vec<&str> = published
        .iter()
        .map(|e| match e {
            ReviewEvent::Opened { .. } => "opened",
            ReviewEvent::FindingRecorded { .. } => "finding",
            ReviewEvent::FindingPromoted { .. } => "promoted",
            ReviewEvent::Concluded { .. } => "concluded",
            ReviewEvent::Closed { .. } => "closed",
        })
        .collect();
    assert_eq!(
        kinds,
        ["opened", "finding", "promoted", "concluded", "closed"]
    );
    for e in &published {
        let operator = match e {
            ReviewEvent::Opened { operator, .. }
            | ReviewEvent::FindingRecorded { operator, .. }
            | ReviewEvent::FindingPromoted { operator, .. }
            | ReviewEvent::Concluded { operator, .. }
            | ReviewEvent::Closed { operator, .. } => operator,
        };
        assert!(
            operator.is_none(),
            "nobody signed in, yet {e:?} names someone"
        );
    }
    assert!(matches!(
        &published[2],
        ReviewEvent::FindingPromoted { gap, finding: 1, .. } if gap == "GAP-034"
    ));
    assert!(matches!(
        &published[1],
        ReviewEvent::FindingRecorded { kind, .. } if kind == "system-behaviour"
    ));
    let _ = std::fs::remove_dir_all(dir);
}

/// A practice finding is never promoted, and the refusal is an alert rather than silence.
#[test]
fn a_practice_finding_is_not_promoted_and_says_so() {
    let (mut state, mut sustainment, dir) = desktop("practice");
    review::apply(&mut state, &mut sustainment, ReviewAction::Open);
    review::apply(
        &mut state,
        &mut sustainment,
        ReviewAction::RecordFinding {
            summary: "the brief was good".into(),
            kind: FindingKindView::Practice,
        },
    );
    let before = state.alerts.len();
    review::apply(
        &mut state,
        &mut sustainment,
        ReviewAction::Promote {
            finding: 1,
            gap: "GAP-001".into(),
        },
    );
    assert_eq!(state.alerts.len(), before + 1, "{:?}", state.alerts);
    assert!(sustainment.review.as_ref().expect("open").findings[0]
        .promoted_to_gap
        .is_none());
    let _ = std::fs::remove_dir_all(dir);
}

/// A review must be concluded before it closes; the desktop relays the workflow's refusal.
#[test]
fn closing_an_open_review_is_refused_with_the_reason() {
    let (mut state, mut sustainment, dir) = desktop("close");
    review::apply(&mut state, &mut sustainment, ReviewAction::Open);
    review::apply(&mut state, &mut sustainment, ReviewAction::Close);
    assert_eq!(
        sustainment.review.as_ref().map(|r| r.state),
        Some(ReviewState::Open)
    );
    assert!(
        state.alerts.iter().any(|a| a.contains("concluded")),
        "{:?}",
        state.alerts
    );
    let _ = std::fs::remove_dir_all(dir);
}

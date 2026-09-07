//! A peer's launch warning on the desktop (GAP-009,
//! `docs/design/DN-16-peer-sources.md` §5).
//!
//! DN-16 §5: a launch warning "raises an alert with the peer named, and it never creates
//! a track". DN-16 §8 states the same as CAP-1.6's pass criterion. This file holds the
//! desktop to both halves through the real frame tick: the alert reaches the list PN-08
//! draws, the fact reaches this session's journal, and the picture gains nothing.
//!
//! The peer link here is scripted rather than connected. What is under test is the
//! desktop's handling of a warning that has already arrived; the wire itself is held to
//! account in `gungnir-remote/tests/launch_warning.rs`, and standing a node up here would
//! be testing the transport a second time.

use gungnir_app::peers::BoundPeer;
use gungnir_app::state::AppState;
use gungnir_app::update;
use gungnir_config::ConfigBaseline;
use gungnir_eventing::Event;
use gungnir_ingest::adapters::peer::{LaunchWarningOutcome, LaunchWarningSink};
use gungnir_model::events::LaunchWarningEvent;
use gungnir_model::{LaunchWarningReport, MissionTime, PeerLaunchWarning, Releasability};
use gungnir_remote::peer::PeerLink;
use gungnir_store::EventJournal;
use gungnir_time::ReplayClockAuthority;

fn desktop(name: &str) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-peer-warning-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    };
    gungnir_config::validate(&config).expect("valid");
    let mut state = AppState::with_config(config).expect("starts");
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(200.0),
    });
    (state, dir)
}

/// A peer bound without a partner on the other end, and the sink its adapter would fill.
fn bound_peer(state: &mut AppState) -> LaunchWarningSink {
    let launch_warnings = LaunchWarningSink::default();
    state.peer_links.push(BoundPeer {
        name: "kal-cell".into(),
        link: PeerLink::scripted("https://kal-cell.example:7410"),
        launch_warnings: launch_warnings.clone(),
    });
    launch_warnings
}

fn report() -> LaunchWarningReport {
    LaunchWarningReport {
        id: "LW-91".into(),
        what: "ballistic launch, northern sector".into(),
        at: MissionTime(190.0),
        releasability: Releasability::AllPeers,
    }
}

/// The criterion: an alert naming the peer, an envelope on the record, and no track.
#[test]
fn a_launch_warning_raises_an_alert_naming_the_peer_and_creates_no_track() {
    let (mut state, dir) = desktop("alert");
    let sink = bound_peer(&mut state);
    let before = state.alerts.len();
    let tracks_before = state.tracking.tracks().len();
    sink.lock()
        .expect("sink")
        .push_back(LaunchWarningOutcome::Admitted(PeerLaunchWarning {
            peer: "kal-cell".into(),
            report: report(),
            receipt_time: MissionTime(200.0),
        }));

    update::tick(&mut state);

    let raised = &state.alerts[before..];
    assert_eq!(raised.len(), 1, "{raised:?}");
    assert!(raised[0].contains("kal-cell"), "{}", raised[0]);
    assert!(
        raised[0].contains("ballistic launch"),
        "the peer's own words reach the operator: {}",
        raised[0]
    );
    assert!(
        raised[0].contains("10 s old"),
        "the age is on the alert: {}",
        raised[0]
    );
    assert_eq!(
        state.tracking.tracks().len(),
        tracks_before,
        "a launch warning must not create a track: DN-16 §5"
    );

    // Once, not on every frame: a warning redrawn as a new alert each tick would fill
    // the list and bury everything else.
    let after_one = state.alerts.len();
    update::tick(&mut state);
    assert_eq!(state.alerts.len(), after_one);

    let _ = std::fs::remove_dir_all(dir);
}

/// The warning reaches this session's journal as `LaunchWarningEvent::Received`, so an
/// after-action review can answer what a peer told this watch and when it was heard.
#[test]
fn a_launch_warning_reaches_the_journal_as_a_received_launch_warning() {
    let (mut state, dir) = desktop("journal");
    let sink = bound_peer(&mut state);
    sink.lock()
        .expect("sink")
        .push_back(LaunchWarningOutcome::Admitted(PeerLaunchWarning {
            peer: "kal-cell".into(),
            report: report(),
            receipt_time: MissionTime(200.0),
        }));

    update::tick(&mut state);
    let session = state.session().expect("a session is open");
    state.save_session().expect("the journal is flushed");

    let journal = gungnir_store::FileEventJournal::open(&dir).expect("journal");
    let envelopes = journal.read_session(session).expect("read back");
    let received: Vec<&PeerLaunchWarning> = envelopes
        .iter()
        .filter_map(|e| match &e.event {
            Event::LaunchWarning(LaunchWarningEvent::Received(w)) => Some(w),
            _ => None,
        })
        .collect();
    assert_eq!(received.len(), 1, "{envelopes:?}");
    assert_eq!(received[0].peer, "kal-cell");
    assert_eq!(received[0].report, report());

    let _ = std::fs::remove_dir_all(dir);
}

/// A refused warning is loud too. A peer sending warnings this deployment cannot read is
/// a fault an operator has to see; silence would look exactly like a peer that saw
/// nothing.
#[test]
fn a_refused_launch_warning_raises_an_alert_with_its_reason() {
    let (mut state, dir) = desktop("refused");
    let sink = bound_peer(&mut state);
    let before = state.alerts.len();
    sink.lock()
        .expect("sink")
        .push_back(LaunchWarningOutcome::Quarantined {
            peer: "kal-cell".into(),
            reason: "the launch warning says nothing".into(),
            at: MissionTime(200.0),
        });

    update::tick(&mut state);

    let raised = &state.alerts[before..];
    assert_eq!(raised.len(), 1, "{raised:?}");
    assert!(raised[0].contains("kal-cell"), "{}", raised[0]);
    assert!(
        raised[0].contains("says nothing"),
        "the reason travels with the refusal: {}",
        raised[0]
    );

    let _ = std::fs::remove_dir_all(dir);
}

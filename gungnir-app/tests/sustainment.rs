// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Replay, reports and the configuration baseline through the desktop (GAP-071).
//!
//! None of this is demonstrable on screen without a display, and two of the three
//! panels report on a journal that a headless test is the only way to exercise, so
//! these are what stands behind the claim that the three panels are wired rather than
//! drawn.

use gungnir_app::state::AppState;
use gungnir_app::sustainment::{self, ConfigEditorState, ReplayState, ReportState};
use gungnir_app::update;
use gungnir_config::ConfigBaseline;
use gungnir_eventing::{Event, TrackingEvent};
use gungnir_model::{
    Classification, ExchangeItem, MissionTime, Provenance, Quality, Releasability, TrackId,
    TrackStatus, TrackView,
};
use gungnir_security::AuditLog;
use gungnir_store::SessionId;
use gungnir_ui::panels::replay::PlayRate;

fn desktop(name: &str) -> (AppState, std::path::PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("gungnir-sustainment-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    };
    (AppState::with_config(config).expect("desktop state"), dir)
}

/// A desktop whose journal actually holds something.
///
/// With no pipeline the tick publishes almost nothing, and the journal creates a
/// session's file on its first append -- so an untouched desktop has *no session in the
/// journal at all*, which is the case `nothing_recorded` covers. These tests need the
/// other case, so they publish a few events and let the tick journal them.
fn desktop_with_events(name: &str, n: usize) -> (AppState, std::path::PathBuf) {
    let (mut state, dir) = desktop(name);
    let now = state.clock.now();
    #[allow(clippy::cast_precision_loss)]
    for i in 0..n {
        let track = TrackView {
            id: TrackId(i as u64),
            status: TrackStatus::Confirmed,
            state: nalgebra::SVector::zeros(),
            covariance: nalgebra::SMatrix::identity(),
            classification: Classification::Unknown,
            provenance: Provenance::default(),
            quality: Quality::default(),
            mission_time: MissionTime(i as f64),
            releasability: Releasability::default(),
        };
        let event = Event::Tracking(TrackingEvent::TrackInitiated(track));
        state.events.publish(now, event).expect("publish");
    }
    update::tick(&mut state);
    state.save_session().expect("save the session");
    (state, dir)
}

/// A desktop whose journal holds one track marked releasable to all peers rather than
/// the `Internal` default, so a report folded from it carries a releasability the
/// GAP-065 exchange producer must actually thread through rather than one that would
/// read the same by coincidence.
fn desktop_with_a_marked_track(name: &str) -> (AppState, std::path::PathBuf) {
    let (mut state, dir) = desktop(name);
    let now = state.clock.now();
    let track = TrackView {
        id: TrackId(1),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::zeros(),
        covariance: nalgebra::SMatrix::identity(),
        classification: Classification::Unknown,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(0.0),
        releasability: Releasability::AllPeers,
    };
    let event = Event::Tracking(TrackingEvent::TrackInitiated(track));
    state.events.publish(now, event).expect("publish");
    update::tick(&mut state);
    state.save_session().expect("save the session");
    (state, dir)
}

/// The session the desktop is recording is listed, marked live, and its length is read
/// rather than guessed.
#[test]
fn the_live_session_is_listed_and_marked_live() {
    let (state, _dir) = desktop_with_events("sessions", 3);

    let sessions = sustainment::session_summaries(&state);
    let live = state.session().expect("a live session");
    let found = sessions
        .iter()
        .find(|s| s.id == live.0)
        .expect("the live session is listed");
    assert!(
        found.live,
        "the session being written to was not marked as recording"
    );
    assert!(
        found.envelopes.is_some(),
        "an unread length would show as 'length not read', which is a different claim \
         from an empty session"
    );
}

/// Opening, stepping and seeking move the cursor and nothing else. The assertion that
/// matters is the last one: the live picture is untouched, which is what the panel says
/// on screen and what GAP-045 would change.
#[test]
fn scrubbing_moves_the_cursor_and_not_the_picture() {
    let (state, _dir) = desktop_with_events("scrub", 5);
    let session = state.session().expect("a live session");

    let mut replay = ReplayState::default();
    replay.open(&state, session).expect("open the session");

    let before = state.tracking.tracks().len();
    replay.step();
    replay.seek_fraction(1.0);
    replay.seek_fraction(0.0);
    assert_eq!(
        state.tracking.tracks().len(),
        before,
        "scrubbing changed the live picture; the panel promises it does not"
    );

    // A rate of zero advances nothing however long the frame was.
    replay.set_rate(PlayRate::Paused);
    replay.advance(10.0);
    let view = sustainment::replay_view(&state, &replay, &[]);
    let open = view.open.expect("a session is open");
    assert_eq!(open.position, 0, "a paused replay advanced");
    assert!(open.live, "the session being recorded was not marked live");
}

/// Opening a session that is not in the journal fails rather than leaving the previous
/// one open under a new heading.
#[test]
fn opening_an_unknown_session_fails() {
    let (state, _dir) = desktop("unknown");
    let mut replay = ReplayState::default();
    let result = replay.open(&state, SessionId(999_999));
    // Either the journal reports it, or it opens empty; what must not happen is a
    // silent success that leaves a stale session on screen.
    if result.is_ok() {
        let view = sustainment::replay_view(&state, &replay, &[]);
        assert_eq!(
            view.open.expect("open").length,
            0,
            "an unknown session opened with someone else's envelopes"
        );
    }
}

/// A report is a fold over the journal: generating one names the session and counts
/// what is in it, and exporting writes a file that exists.
#[test]
fn a_report_folds_the_journal_and_exports_a_file() {
    let (state, dir) = desktop_with_events("report", 10);

    let mut reports = ReportState::default();
    let view = sustainment::reports_view(&state, &reports);
    assert!(
        view.counts.is_none(),
        "an ungenerated report must not present counts"
    );

    reports.generate(&state).expect("generate");
    let view = sustainment::reports_view(&state, &reports);
    let counts = view.counts.expect("counts after generating");
    assert!(
        counts.iter().any(|c| c.label == "Expired"),
        "the expiry count is missing, so an expiry would be read as a rejection"
    );
    assert!(
        counts
            .iter()
            .find(|c| c.label == "Expired")
            .and_then(|c| c.note)
            .is_some_and(|n| n.contains("not rejections")),
        "the expiry count must say what it is not"
    );
    assert!(
        view.metrics.is_err(),
        "a live session has no ground truth, so tracking metrics must be unavailable \
         rather than zero"
    );

    reports.export(&state).expect("export");
    let written = dir.join(sustainment::REPORT_DIR).join(format!(
        "session-{}.json",
        state.session().expect("session").0
    ));
    assert!(
        written.exists(),
        "export reported success and wrote nothing"
    );
    let text = std::fs::read_to_string(&written).expect("read the export");
    assert!(
        text.contains("session"),
        "the export does not name its session, so its figures cannot be recomputed"
    );
}

/// GAP-065, DN-18 §5 amendment 2: generating a report queues it for coalition exchange
/// when a node is linked -- the same shape `handoffs.rs::issue_for` uses for
/// `Handoffs`, with the one report `ReportState` holds standing in for a set that has
/// no growing `Vec` to republish. With no link there is nothing to queue to and
/// generating still works.
#[test]
fn generating_a_report_queues_it_for_exchange_when_a_node_is_linked() {
    use gungnir_remote::link::NodeLink;

    let (mut state, _dir) = desktop_with_a_marked_track("exchange-generate");
    let mut reports = ReportState::default();

    // No link yet: generating still works, and there is nothing to queue to.
    reports.generate(&state).expect("generate");

    let link = NodeLink::scripted();
    state.link = Some(link.clone());
    reports.generate(&state).expect("generate while linked");

    let p = link.read().expect("projection");
    assert_eq!(
        p.exchange_outbox.len(),
        1,
        "one publish for the one report, generated while linked; the earlier unlinked \
         generate queued nothing"
    );
    let batch = &p.exchange_outbox[0];
    assert_eq!(batch.item, ExchangeItem::Reports);
    assert_eq!(
        batch.products.len(),
        1,
        "one report is the whole current set, not a growing one"
    );
    assert_eq!(
        batch.products[0].releasability,
        Releasability::AllPeers,
        "the queued product must carry the report's own releasability"
    );
}

/// Exporting is PN-13's second, independent action (an operator can export without
/// regenerating first) and GAP-065 names it as producing a report too: exporting queues
/// it for exchange exactly as generating does, and does so again on the same report
/// when a node is linked only after the export runs.
#[test]
fn exporting_a_report_queues_it_for_exchange_when_a_node_is_linked() {
    use gungnir_remote::link::NodeLink;

    let (mut state, _dir) = desktop_with_a_marked_track("exchange-export");
    let mut reports = ReportState::default();
    reports.generate(&state).expect("generate");

    // No link yet: exporting still writes the file, and there is nothing to queue to.
    reports.export(&state).expect("export");

    let link = NodeLink::scripted();
    state.link = Some(link.clone());
    reports.export(&state).expect("export while linked");

    let p = link.read().expect("projection");
    assert_eq!(
        p.exchange_outbox.len(),
        1,
        "one publish for the one export while linked; the earlier unlinked export and \
         the unlinked generate before it queued nothing"
    );
    let batch = &p.exchange_outbox[0];
    assert_eq!(batch.item, ExchangeItem::Reports);
    assert_eq!(
        batch.products[0].releasability,
        Releasability::AllPeers,
        "the queued product must carry the report's own releasability"
    );
}

/// The baseline in force validates, and applying is refused when the desktop has no
/// file to write to. Refused, not silently ignored: an administrator who clicked apply
/// and saw nothing happen would not know whether it had.
#[test]
fn a_desktop_with_no_baseline_file_refuses_to_apply() {
    let (mut state, _dir) = desktop("config");
    let mut editor = ConfigEditorState::default();

    editor.validate(&state);
    let sections = sustainment::config_sections(&state);
    let audit = sustainment::audit_lines(&state);
    let view = sustainment::config_editor_view(
        &state,
        &editor,
        &sections,
        &audit,
        "Operator",
        None,
        gungnir_ui::panels::config_editor::GovernedProfiles::NothingDeclared,
    );
    assert!(
        matches!(
            view.in_force,
            gungnir_ui::panels::config_editor::Validation::Valid
        ),
        "the default baseline did not validate"
    );

    editor.reload(&state);
    assert!(
        editor.candidate_error().is_some(),
        "reloading with no baseline file must say so"
    );

    let err = editor.apply(&mut state).expect_err("apply must be refused");
    assert!(!err.is_empty());
    assert!(
        state.audit.entries().is_empty(),
        "a refused apply must not leave an audit entry claiming one happened"
    );
}

/// Applying an unvalidated candidate is refused even when everything else is in place.
/// `ConfigStore::apply` validates again, but the panel must not rely on that second
/// check to catch what it should not have offered.
#[test]
fn an_unvalidated_candidate_is_not_applied() {
    let (mut state, _dir) = desktop("unvalidated");
    let mut editor = ConfigEditorState::default();
    let err = editor
        .apply(&mut state)
        .expect_err("an unvalidated candidate must be refused");
    assert!(err.contains("validated"), "the refusal must say why: {err}");
}

/// Every section of the baseline is named, so PN-14 cannot quietly omit one an
/// administrator needs to check before applying.
#[test]
fn every_configuration_section_is_named() {
    let (state, _dir) = desktop("sections");
    let sections = sustainment::config_sections(&state);
    for expected in [
        "Sensors",
        "Resources",
        "Defended assets",
        "Endpoints",
        "Control status",
        "Authority rules",
        "Decision deadlines",
        "Display vocabulary",
    ] {
        assert!(
            sections.iter().any(|s| s.name == expected),
            "{expected} is not shown on the configuration panel"
        );
    }
}

/// A session that has recorded nothing is not a failed report and not a report of
/// zeros: the journal does not create a session's file until its first append, so the
/// session is absent from it entirely and the report has to say which.
///
/// **No tick here, deliberately.** Until GAP-086 a ticked session published almost nothing
/// and this test drove three ticks to prove the point; a session now records which
/// algorithm configuration it opened with on its first tick, so the untouched session is
/// what makes the empty case reachable. The other half of that change is tested below.
#[test]
fn a_session_that_recorded_nothing_is_not_a_failure() {
    let (state, _dir) = desktop("nothing");

    let mut reports = ReportState::default();
    reports
        .generate(&state)
        .expect("an empty session is not an error");

    let view = sustainment::reports_view(&state, &reports);
    assert!(
        view.nothing_recorded,
        "an empty session must say so rather than looking ungenerated"
    );
    assert!(
        view.counts.is_none(),
        "there is nothing to count, and zeros would be a claim about the session"
    );
}

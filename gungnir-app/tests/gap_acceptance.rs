// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A commander accepting a coverage gap, and the acceptance re-opening by itself (GAP-106,
//! D-118, `docs/design/DN-33-accepting-a-coverage-gap.md` §12's Draft row).
//!
//! Every test runs round 1's committed baseline and laydowns
//! (`testdata/usability/round-1.json`): both radars searching leave the outer end of the
//! upper Vell approach uncovered, which is the gap US-16 asks a commander to accept.
//!
//! - accepted by a signed-in commander, journaled, audited, marked on PN-11, the viewport,
//!   the strip and PN-16's current row, and counted inside the measure, not taken out of it;
//! - refused for an operator, for nobody signed in, and without a reason, recording
//!   nothing;
//! - re-opened, journaled and audited, when the gap's shape changes, when the baseline's
//!   revision changes and when another laydown is marked current -- and standing across a
//!   restart when none of them has.

use gungnir_app::gap_acceptance::{self, AcceptError};
use gungnir_app::state::AppState;
use gungnir_app::{sustainment, update, workspace};
use gungnir_config::ConfigBaseline;
use gungnir_eventing::Event;
use gungnir_model::events::PlanningEvent;
use gungnir_model::{
    AcceptedGap, GapSeverity, LaydownId, MissionTime, ReopenedBecause, SensorId, SensorMode,
};
use gungnir_security::{
    hash_passphrase, Account, AuditLog, InMemoryAccountStore, LocalAccountAuthority, OperatorId,
    Role,
};
use gungnir_sensor_management::SensorRegistry;
use gungnir_time::ReplayClockAuthority;
use std::path::{Path, PathBuf};

const PASSPHRASE: &str = "correct horse battery staple";
const COMMANDER: OperatorId = OperatorId(41);
const OPERATOR: OperatorId = OperatorId(42);

fn round_1() -> ConfigBaseline {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata/usability/round-1.json");
    let text = std::fs::read_to_string(path).expect("round-1.json is committed");
    serde_json::from_str(&text).expect("round-1.json parses")
}

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("gungnir-gap-accept-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Round 1 at `revision`, with `current` the laydown in force, journaling into `dir`, on
/// a clock the test steps. A second call with the same `dir` is a restart.
fn desktop(dir: &Path, revision: u32, current: &str) -> AppState {
    let mut config = round_1();
    config.data_dir = dir.to_string_lossy().into_owned();
    config.revision = revision;
    for l in &mut config.laydowns {
        l.current = l.id.0 == current;
    }
    gungnir_config::validate(&config).expect("the round-1 baseline is valid");
    let mut state = AppState::with_config(config).expect("the desktop starts");
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(100.0),
    });
    state.set_session_authority(Box::new(LocalAccountAuthority::new(Box::new(
        InMemoryAccountStore::new(vec![
            Account {
                operator: COMMANDER,
                role: Role::Commander,
                phc: hash_passphrase(PASSPHRASE).expect("hashed"),
            },
            Account {
                operator: OPERATOR,
                role: Role::Operator,
                phc: hash_passphrase(PASSPHRASE).expect("hashed"),
            },
        ]),
    ))));
    state
}

fn sign_in(state: &mut AppState, who: OperatorId) {
    state.sign_out();
    state
        .sign_in(&LocalAccountAuthority::credential(who, PASSPHRASE))
        .expect("signed in");
}

/// Both radars searching, as round 1 sites them.
fn radars_searching(state: &mut AppState) {
    for id in [1, 2] {
        state
            .sensors
            .set_mode(SensorId(id), SensorMode::Search)
            .expect("standby to search is permitted");
    }
}

/// The live report's uncovered gap on the upper Vell approach, named as PN-11 names it.
fn the_uncovered_gap(state: &AppState) -> (usize, AcceptedGap) {
    let report = sustainment::coverage_report(state).expect("round 1 declares an origin");
    let names = sustainment::approach_names(state);
    report
        .gaps
        .iter()
        .enumerate()
        .find(|(_, g)| g.severity == GapSeverity::Uncovered)
        .map_or_else(
            || panic!("round 1 leaves the outer approach uncovered: {report:?}"),
            |(i, g)| {
                (
                    i,
                    gungnir_analytics::accepted_gap(g, &names[g.approach], report.parameters),
                )
            },
        )
}

fn planning_events(
    rx: &gungnir_eventing::Receiver<gungnir_eventing::Envelope>,
) -> Vec<PlanningEvent> {
    rx.try_iter()
        .filter_map(|env| match env.event {
            Event::Planning(e) => Some(e),
            _ => None,
        })
        .collect()
}

fn audited(state: &AppState) -> Vec<(Option<OperatorId>, String)> {
    state
        .audit
        .entries()
        .iter()
        .filter(|e| e.action == gungnir_security::actions::ACCEPT_COVERAGE_GAP)
        .map(|e| (e.operator, e.detail.clone()))
        .collect()
}

/// DN-33 §6: only a signed-in commander accepts, with a reason; anyone else is refused and
/// nothing is journaled or audited.
#[test]
fn a_commander_accepts_a_gap_and_nobody_else_can() {
    let dir = scratch("who");
    let mut state = desktop(&dir, 1, "current");
    radars_searching(&mut state);
    let events = state.events.subscribe();
    let (_, gap) = the_uncovered_gap(&state);

    // Nobody signed in, even with the commander's role selected.
    state.set_role(Role::Commander);
    assert_eq!(
        gap_acceptance::accept(&mut state, &gap, "the ridge closes it"),
        Err(AcceptError::NotSignedIn)
    );
    // An operator.
    sign_in(&mut state, OPERATOR);
    let refused = gap_acceptance::accept(&mut state, &gap, "the ridge closes it")
        .expect_err("an operator may not accept a gap");
    assert!(
        matches!(&refused, AcceptError::NotPermitted { role } if role == "Operator"),
        "{refused}"
    );
    assert!(
        refused.to_string().contains("only the commander"),
        "{refused}"
    );
    // A commander without a reason.
    sign_in(&mut state, COMMANDER);
    assert_eq!(
        gap_acceptance::accept(&mut state, &gap, "   "),
        Err(AcceptError::NoReason)
    );
    update::tick(&mut state);
    assert!(
        planning_events(&events).is_empty(),
        "a refusal was journaled"
    );
    assert!(audited(&state).is_empty(), "a refusal was audited");
    assert!(state.gap_acceptances.standing.is_empty());

    // The commander, with a reason.
    let id = gap_acceptance::accept(
        &mut state,
        &gap,
        "the upper Vell is covered by the harbour patrol until R2 is moved",
    )
    .expect("a signed-in commander accepts");
    update::tick(&mut state);
    let journaled = planning_events(&events);
    assert!(
        matches!(journaled.as_slice(), [PlanningEvent::GapAccepted(a)]
            if a.id == id
                && a.operator == COMMANDER.0.to_string()
                && a.role == "Commander"
                && a.revision == 1
                && a.laydown == Some(LaydownId("current".into()))
                && a.reason.contains("harbour patrol")),
        "{journaled:?}"
    );
    let entries = audited(&state);
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].0, Some(COMMANDER));
    assert!(entries[0].1.contains("upper Vell"), "{entries:?}");
    assert!(entries[0].1.contains("harbour patrol"), "{entries:?}");
    // Accepting it twice is refused naming the first.
    assert!(matches!(
        gap_acceptance::accept(&mut state, &gap, "again"),
        Err(AcceptError::AlreadyAccepted { id: held, .. }) if held == id
    ));
}

/// D-118: the accepted gap stays drawn, marked accepted, on PN-11, the viewport, the strip
/// and PN-16's current row, and still counts: the measure reports the accepted metres
/// inside the uncovered total. An option nobody accepted anything for shows none.
#[test]
fn an_accepted_gap_is_drawn_as_accepted_and_still_counts() {
    let dir = scratch("drawn");
    let mut state = desktop(&dir, 1, "current");
    radars_searching(&mut state);
    let (index, gap) = the_uncovered_gap(&state);
    let report = sustainment::coverage_report(&state).expect("measured");
    let in_force = Some(LaydownId("current".into()));
    let before = gap_acceptance::measure(&state, &report, in_force.as_ref());
    assert_eq!(before.accepted_segments, 0);

    sign_in(&mut state, COMMANDER);
    gap_acceptance::accept(&mut state, &gap, "accepted for the exercise").expect("accepted");
    update::tick(&mut state);
    assert_eq!(state.gap_acceptances.standing.len(), 1, "it holds");

    // The measure: the same totals, with the accepted part inside them.
    let after = gap_acceptance::measure(&state, &report, in_force.as_ref());
    assert!(
        (after.uncovered_m - before.uncovered_m).abs() < 1e-9,
        "never subtracted"
    );
    assert_eq!(
        after.segments, before.segments,
        "an accepted gap still counts"
    );
    assert_eq!(after.accepted_segments, 1);
    assert!((after.uncovered_accepted_m - gap.length_m()).abs() < 1e-9);

    // The viewport: every gap still drawn, this one marked.
    let names = sustainment::approach_names(&state);
    let accepted = sustainment::live_accepted(&state, &report);
    let lines = sustainment::gap_polylines(&names, &report, &accepted);
    assert_eq!(lines.len(), report.gaps.len(), "no gap is hidden");
    assert!(lines[index].accepted && lines[index].uncovered);
    assert_eq!(lines.iter().filter(|l| l.accepted).count(), 1);

    // The strip.
    match sustainment::coverage_status(&state, Some(&report)) {
        gungnir_ui::panels::status_strip::CoverageStatus::Measured {
            uncovered_segments,
            accepted_segments,
            ..
        } => {
            assert!(uncovered_segments >= 1);
            assert_eq!(accepted_segments, 1);
        }
        other => panic!("measured coverage, got {other:?}"),
    }

    // PN-11, drawn: the gap line says ACCEPTED, with the reason, and the measure says the
    // accepted metres are inside the total.
    let probe = gungnir_ui::harness::RenderProbe::new();
    let mut draft = gungnir_ui::panels::coverage_layers::AcceptanceDraft::default();
    let (_, frame) = probe.draw(|ui| workspace::render_coverage_layers(ui, &state, &mut draft));
    assert!(frame.says("ACCEPTED"), "{}", frame.joined());
    assert!(
        frame.says("accepted for the exercise"),
        "{}",
        frame.joined()
    );
    assert!(
        frame.says("An accepted gap still counts"),
        "{}",
        frame.joined()
    );

    // PN-16: the current row names its accepted segment; the options, none.
    let rows = match sustainment::planning_rows(&state) {
        sustainment::PlanningRows::Rows(rows) => rows,
        sustainment::PlanningRows::Empty { reason } => panic!("{reason}"),
    };
    for row in &rows {
        let gungnir_ui::panels::planning::LaydownCoverage::Computed {
            accepted_segments,
            accepted_uncovered_m,
            uncovered_m,
            ..
        } = &row.coverage
        else {
            panic!("round 1's coverage computes: {:?}", row.coverage);
        };
        if row.current {
            assert_eq!(*accepted_segments, 1, "the laydown in force: {row:?}");
            assert!(*accepted_uncovered_m > 0.0 && accepted_uncovered_m <= uncovered_m);
        } else {
            assert_eq!(*accepted_segments, 0, "an option: {row:?}");
        }
    }
    let (_, frame) = probe.draw(|ui| {
        workspace::render_panel(ui, gungnir_workflow::PanelId::Planning, &state);
    });
    assert!(frame.says("accepted"), "{}", frame.joined());
}

/// DN-33 §5: a changed shape re-opens it by itself -- journaled, audited against nobody,
/// said on PN-11 with why -- and the re-opened gap can be accepted again.
#[test]
fn a_changed_shape_re_opens_the_acceptance() {
    let dir = scratch("shape");
    let mut state = desktop(&dir, 1, "current");
    radars_searching(&mut state);
    let events = state.events.subscribe();
    let (_, gap) = the_uncovered_gap(&state);
    sign_in(&mut state, COMMANDER);
    let id = gap_acceptance::accept(&mut state, &gap, "accepted for the exercise").expect("ok");
    update::tick(&mut state);
    update::tick(&mut state);
    assert_eq!(state.gap_acceptances.standing.len(), 1, "nothing changed");

    // R2 drops to standby: the uncovered stretch grows toward the harbour.
    state
        .sensors
        .set_mode(SensorId(2), SensorMode::Standby)
        .expect("search to standby is permitted");
    update::tick(&mut state);
    assert!(state.gap_acceptances.standing.is_empty(), "it re-opened");
    let reopened = &state.gap_acceptances.reopened;
    assert_eq!(reopened.len(), 1);
    let ReopenedBecause::ShapeChanged { now } = &reopened[0].because else {
        panic!("a shape change, got {:?}", reopened[0].because);
    };
    assert!(
        now.iter()
            .any(|g| g.severity == GapSeverity::Uncovered && g.length_m() > gap.length_m()),
        "{now:?}"
    );
    let journaled = planning_events(&events);
    assert!(
        journaled.iter().any(|e| matches!(e,
            PlanningEvent::GapAcceptanceReopened { acceptance, because: ReopenedBecause::ShapeChanged { .. }, .. }
                if *acceptance == id)),
        "{journaled:?}"
    );
    let entries = audited(&state);
    assert_eq!(entries.len(), 2, "accepted, then re-opened: {entries:?}");
    assert_eq!(entries[1].0, None, "nobody re-opened it");
    assert!(entries[1].1.contains("re-opened"), "{entries:?}");

    let probe = gungnir_ui::harness::RenderProbe::new();
    let mut draft = gungnir_ui::panels::coverage_layers::AcceptanceDraft::default();
    let (_, frame) = probe.draw(|ui| workspace::render_coverage_layers(ui, &state, &mut draft));
    assert!(frame.says("Re-opened this session"), "{}", frame.joined());
    assert!(frame.says("the gap there is now"), "{}", frame.joined());
}

/// DN-33 §5 and §8 rule 6: an acceptance is on the record across a restart. It stands when
/// nothing changed; a baseline at the next revision re-opens it; so does another laydown
/// marked current -- each with its reason, on the new session's record.
#[test]
fn a_new_revision_or_another_laydown_in_force_re_opens_it_and_nothing_else_does() {
    let dir = scratch("restart");
    let accept_one = |state: &mut AppState| {
        radars_searching(state);
        let (_, gap) = the_uncovered_gap(state);
        sign_in(state, COMMANDER);
        let id = gap_acceptance::accept(state, &gap, "accepted for the exercise").expect("ok");
        update::tick(state);
        state.save_session().expect("saved");
        id
    };

    // Session one accepts it.
    let first = {
        let mut state = desktop(&dir, 1, "current");
        accept_one(&mut state)
    };

    // Restart, nothing changed: it is on the record and still holds once the radars are
    // searching again.
    {
        let mut state = desktop(&dir, 1, "current");
        assert_eq!(state.gap_acceptances.standing.len(), 1, "recovered");
        assert_eq!(state.gap_acceptances.standing[0].0.id, first);
        radars_searching(&mut state);
        update::tick(&mut state);
        assert_eq!(state.gap_acceptances.standing.len(), 1, "nothing changed");
        state.save_session().expect("saved");
    }

    // Restart at the next revision: re-opened, and a new acceptance takes a new number.
    let second = {
        let mut state = desktop(&dir, 2, "current");
        let events = state.events.subscribe();
        radars_searching(&mut state);
        update::tick(&mut state);
        assert!(state.gap_acceptances.standing.is_empty());
        assert_eq!(
            state.gap_acceptances.reopened[0].because,
            ReopenedBecause::RevisionChanged { from: 1, to: 2 }
        );
        assert!(planning_events(&events).iter().any(|e| matches!(e,
            PlanningEvent::GapAcceptanceReopened { acceptance, .. } if *acceptance == first)));
        let second = accept_one(&mut state);
        assert!(second > first, "{second} after {first}");
        second
    };

    // Restart with laydown `c` in force at the same revision: re-opened.
    let mut state = desktop(&dir, 2, "c");
    assert_eq!(
        state
            .gap_acceptances
            .standing
            .iter()
            .map(|(a, _)| a.id)
            .collect::<Vec<_>>(),
        vec![second],
        "the first was re-opened on the record and stays so"
    );
    radars_searching(&mut state);
    update::tick(&mut state);
    assert!(state.gap_acceptances.standing.is_empty());
    assert_eq!(
        state.gap_acceptances.reopened[0].because,
        ReopenedBecause::LaydownChanged {
            from: Some(LaydownId("current".into())),
            to: Some(LaydownId("c".into())),
        }
    );
    assert!(
        state.alerts.iter().any(|a| a.contains("re-opened")),
        "{:?}",
        state.alerts
    );
}

/// DN-33 §8 rule 7: retention keeps a standing acceptance's session, so ageing the
/// journal cannot un-accept a gap.
#[test]
fn retention_keeps_a_standing_acceptance() {
    let dir = scratch("retention");
    let session = {
        let mut state = desktop(&dir, 1, "current");
        radars_searching(&mut state);
        let (_, gap) = the_uncovered_gap(&state);
        sign_in(&mut state, COMMANDER);
        gap_acceptance::accept(&mut state, &gap, "accepted for the exercise").expect("ok");
        update::tick(&mut state);
        state.save_session().expect("saved");
        state.session().expect("a live session")
    };
    let state = desktop(&dir, 1, "current");
    assert!(
        gungnir_app::retention::protected(&state).contains(&session),
        "the session holding the acceptance is not protected"
    );
}

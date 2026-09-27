// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A decision on a plan is told where the laydown in force stands, and acknowledges it
//! when it was never rehearsed or was rehearsed under something other than what is
//! running (GAP-107, D-119, D-120; `docs/design/DN-26-laydown-options.md` §11).
//!
//! Advisory and acknowledged, never a gate: nothing refuses a plan for an unrehearsed
//! laydown, but accepting or overriding one without ticking the sentence is refused and
//! records nothing, and the sentence ticked travels with the decision -- on the record,
//! on `CommandEvent::Decided`, and in the audit entry.
//!
//! Round 1's committed baseline and laydowns (`testdata/usability/round-1.json`), and its
//! committed recording, TT-11 (GAP-147), which the rehearsal re-observes.

use gungnir_app::state::AppState;
use gungnir_app::{decisions, laydown_rehearsal, rehearsal_standing, update, workspace};
use gungnir_command::{ApprovalWorkflow, CommandError, OperatorDecision, Submission};
use gungnir_config::ConfigBaseline;
use gungnir_eventing::Event;
use gungnir_model::events::CommandEvent;
use gungnir_model::{
    Acknowledgement, EffectorLayer, LaydownId, MissionTime, PendingApprovalId, PlanId, PlanView,
    TestTrackNumber,
};
use gungnir_policy::PolicyVerdict;
use gungnir_security::{AuditLog, Role};
use gungnir_time::ReplayClockAuthority;
use gungnir_ui::panels::approval_queue::PendingId;
use rehearsal_standing::InForce;
use std::path::{Path, PathBuf};

fn testdata_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata")
}

fn round_1() -> ConfigBaseline {
    let text = std::fs::read_to_string(testdata_root().join("usability/round-1.json"))
        .expect("round-1.json is committed");
    serde_json::from_str(&text).expect("round-1.json parses")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gungnir-advisory-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Round 1 journaling into `dir`, changed by `change`; a second call with the same `dir`
/// is a restart. The desktop acts as the operator round 1 gives the point layer to.
fn desktop(dir: &Path, change: impl FnOnce(&mut ConfigBaseline)) -> AppState {
    let mut config = round_1();
    config.data_dir = dir.to_string_lossy().into_owned();
    change(&mut config);
    gungnir_config::validate(&config).expect("a valid round-1 baseline");
    let mut state = AppState::with_config(config).expect("the desktop starts");
    state.clock = Box::new(ReplayClockAuthority {
        current: MissionTime(100.0),
    });
    state.set_role(Role::Operator);
    state
}

/// A point-layer plan waiting for the operator.
fn queued(state: &mut AppState, plan: u128) -> PendingApprovalId {
    state
        .desk
        .approvals
        .submit_for_approval(Submission {
            plan: PlanView {
                id: PlanId(plan),
                ..PlanView::default()
            },
            verdict: PolicyVerdict::RequiresHumanApproval,
            submitted: MissionTime(100.0),
            layer: EffectorLayer::Point,
            priority: 0.0,
            role: "Operator".into(),
        })
        .expect("queued")
}

fn acknowledgements_decided(
    rx: &gungnir_eventing::Receiver<gungnir_eventing::Envelope>,
) -> Vec<Vec<Acknowledgement>> {
    rx.try_iter()
        .filter_map(|env| match env.event {
            Event::Command(CommandEvent::Decided { acknowledged, .. }) => Some(acknowledged),
            _ => None,
        })
        .collect()
}

/// Rehearse the laydown in force against TT-11 the way PN-16's run does, and put the run on
/// the record.
fn rehearse_current(state: &mut AppState) {
    let current = state
        .config
        .laydowns
        .iter()
        .find(|l| l.current)
        .expect("round 1 marks one current")
        .clone();
    let record = laydown_rehearsal::run(
        &testdata_root(),
        TestTrackNumber(11),
        &current,
        &state.config,
        state.clock.now(),
    )
    .expect("round 1's laydown in force rehearses against TT-11");
    rehearsal_standing::record(state, record.stamp());
    state.rehearsal_records.insert(current.id.clone(), record);
}

/// DN-26 §11 items 5 and 6: never rehearsed is said on PN-16 and PN-07; accepting or
/// overriding without the tick is refused and records nothing; with it, the sentence goes
/// onto the record, the journal and the audit entry. A rejection asks nothing.
#[test]
// One decision walked from the panels to the record; split, each part would repeat the
// set-up and the queue.
#[allow(clippy::too_many_lines)]
fn a_laydown_never_rehearsed_is_acknowledged_before_a_plan_is_acted_on() {
    let dir = scratch("never");
    let mut state = desktop(&dir, |_| {});
    let events = state.events.subscribe();

    let standing = rehearsal_standing::in_force(&state);
    assert_eq!(
        standing,
        InForce::NeverRehearsed {
            laydown: LaydownId("current".into())
        }
    );
    let advisory = rehearsal_standing::advisory(&state).expect("it asks");
    assert_eq!(advisory.subject, "rehearsal");
    assert!(
        advisory.statement.contains("never been rehearsed"),
        "{}",
        advisory.statement
    );

    // PN-16 says it, first.
    let probe = gungnir_ui::harness::RenderProbe::new();
    let (_, frame) = probe.draw(|ui| {
        workspace::render_panel(ui, gungnir_workflow::PanelId::Planning, &state);
    });
    assert!(frame.says("never been rehearsed"), "{}", frame.joined());
    assert!(frame.says("Nothing refuses a plan"), "{}", frame.joined());

    // PN-07 says it, in its own section, with the tick.
    let item = queued(&mut state, 7);
    state.select_approval(PendingId(item.0));
    let mut dialog = state.dialog.clone();
    let (_, frame) = probe.draw(|ui| workspace::render_decision_dialog(ui, &state, &mut dialog));
    assert!(
        frame.says("Rehearsal of the laydown in force"),
        "{}",
        frame.joined()
    );
    assert!(frame.says("never been rehearsed"), "{}", frame.joined());

    // Refused without the tick; nothing recorded, nothing published, nothing audited.
    let refused = decisions::decide(&mut state, PendingId(item.0), OperatorDecision::Accepted)
        .expect_err("an unacknowledged advisory refuses an acceptance");
    assert!(
        matches!(&refused, CommandError::Unacknowledged { statement, .. } if *statement == advisory.statement),
        "{refused}"
    );
    assert!(state.desk.approvals.records().is_empty());
    assert!(state.desk.approvals.queue().iter().any(|i| i.id == item));

    // A tick given against some other sentence does not count.
    state.dialog.rehearsal_acknowledged = Some("an older sentence".into());
    assert!(matches!(
        decisions::decide(&mut state, PendingId(item.0), OperatorDecision::Accepted),
        Err(CommandError::Unacknowledged { .. })
    ));
    update::tick(&mut state);
    assert!(acknowledgements_decided(&events).is_empty());
    assert!(
        !state
            .audit
            .entries()
            .iter()
            .any(|e| e.action == gungnir_security::actions::DECIDE_PLAN),
        "a refused decision was audited"
    );

    // A rejection acts on nothing and asks nothing.
    let other = queued(&mut state, 8);
    decisions::decide(
        &mut state,
        PendingId(other.0),
        OperatorDecision::Rejected {
            reason: "not this one".into(),
        },
    )
    .expect("a rejection needs no acknowledgement");
    assert!(state.desk.approvals.records()[0].acknowledged.is_empty());

    // With the tick against this sentence: recorded, published and audited with it.
    state.select_approval(PendingId(item.0));
    state.dialog.rehearsal_acknowledged = Some(advisory.statement.clone());
    decisions::decide(&mut state, PendingId(item.0), OperatorDecision::Accepted)
        .expect("acknowledged, the acceptance is recorded");
    let record = state
        .desk
        .approvals
        .records()
        .iter()
        .find(|r| r.item == Some(item))
        .expect("the acceptance is on the record");
    assert_eq!(record.acknowledged, vec![advisory.clone()]);
    update::tick(&mut state);
    let decided = acknowledgements_decided(&events);
    assert_eq!(
        decided,
        vec![Vec::new(), vec![advisory.clone()]],
        "{decided:?}"
    );
    let entry = state
        .audit
        .entries()
        .iter()
        .rev()
        .find(|e| e.action == gungnir_security::actions::DECIDE_PLAN)
        .expect("the decision is audited");
    assert!(
        entry
            .detail
            .contains("acknowledged rehearsal: The laydown in force"),
        "{}",
        entry.detail
    );
    assert_eq!(
        state.dialog.rehearsal_acknowledged, None,
        "the dialog closed on the decision, and its tick with it"
    );
}

/// DN-26 §11 items 2 to 4, against a real rehearsal of TT-11: rehearsed under what is
/// running asks nothing; it is still on the record after a restart; a revision that
/// changed nothing a rehearsal runs under asks nothing and says so; a policy change is
/// named, with the revisions, and asks again.
#[test]
fn a_laydown_rehearsed_under_what_is_running_asks_nothing_and_a_policy_change_asks_again() {
    let dir = scratch("rehearsed");
    {
        let mut state = desktop(&dir, |_| {});
        rehearse_current(&mut state);
        let standing = rehearsal_standing::in_force(&state);
        assert!(
            matches!(&standing, InForce::Stands { stamp, .. }
                if stamp.stamp.scenario == TestTrackNumber(11) && stamp.stamp.revision == 1),
            "{standing:?}"
        );
        assert!(!standing.asks());
        assert_eq!(rehearsal_standing::advisory(&state), None);
        let sentence = standing.sentence(state.session()).expect("said");
        assert!(sentence.contains("under what is running now"), "{sentence}");

        // A decision asks nothing and records nothing acknowledged.
        let item = queued(&mut state, 9);
        decisions::decide(&mut state, PendingId(item.0), OperatorDecision::Accepted)
            .expect("nothing to acknowledge");
        assert!(state.desk.approvals.records()[0].acknowledged.is_empty());
        update::tick(&mut state);
        state.save_session().expect("saved");
    }

    // Restart at revision 2, changing only what a rehearsal does not take.
    {
        let state = desktop(&dir, |c| {
            c.revision = 2;
            c.endpoints.push(gungnir_config::EndpointConfig {
                name: "harbour-master-watch-desk".into(),
                kind: "warning".into(),
                address: "https://example.invalid/warn".into(),
            });
        });
        let standing = rehearsal_standing::in_force(&state);
        assert!(!standing.asks(), "{standing:?}");
        let sentence = standing.sentence(state.session()).expect("said");
        assert!(
            sentence.contains("revision 2 changed nothing a rehearsal runs under"),
            "{sentence}"
        );
        // PN-16 knows it was rehearsed, and does not make up the figures.
        assert!(matches!(
            state.row_rehearsal(&LaydownId("current".into()), true),
            gungnir_ui::panels::planning::RowRehearsal::RehearsedEarlier {
                scenario: TestTrackNumber(11),
                ..
            }
        ));
    }

    // Restart at revision 3 with the point layer's decision window changed: the policy the
    // rehearsal ran under is not the one running, and a decision asks again.
    let mut state = desktop(&dir, |c| {
        c.revision = 3;
        c.policy
            .decisions
            .expiry_s
            .insert(EffectorLayer::Point, 120.0);
    });
    let standing = rehearsal_standing::in_force(&state);
    let InForce::RehearsedUnderOther { parts, latest, .. } = &standing else {
        panic!("rehearsed under another policy: {standing:?}");
    };
    assert_eq!(parts, &vec!["policy"]);
    assert_eq!(latest.stamp.revision, 1);
    let advisory = rehearsal_standing::advisory(&state).expect("it asks again");
    assert!(
        advisory.statement.contains("the policy it ran under")
            && advisory.statement.contains("revision 3"),
        "{}",
        advisory.statement
    );
    let item = queued(&mut state, 10);
    assert!(matches!(
        decisions::decide(&mut state, PendingId(item.0), OperatorDecision::Accepted),
        Err(CommandError::Unacknowledged { .. })
    ));
}

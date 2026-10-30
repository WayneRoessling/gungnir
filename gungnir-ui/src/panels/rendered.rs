// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! What the panels actually draw (GAP-071/GAP-075 follow-up).
//!
//! Every other test in this crate asserts on a view struct: that `EmptyBecause` holds
//! the right sentence, that `accept_enabled` returns false. None of them checked that
//! the sentence reaches the screen. A panel could hold a perfect view and draw none of
//! it, and every test would still pass.
//!
//! These run each panel through [`crate::harness::RenderProbe`] -- a real egui frame,
//! laid out, with no window, GPU or display -- and assert on the text it painted. The
//! claims being checked are the ones that would be worst to get wrong: the sentences
//! that stop a true-but-misleading screen from being read the easy way.

#![cfg(test)]

use crate::harness::RenderProbe;
use crate::panels::status_strip::{BaselineValidity, EncryptionState, OperatorLine};
use crate::panels::unavailable::{Section, Unavailable};
use crate::theme;
use gungnir_model::Vocabulary;

/// The defaults, which is what an unconfigured deployment shows.
fn vocabulary() -> Vocabulary {
    Vocabulary::default()
}

/// PN-10's delivery path: commands are recorded and published, and no adapter carries
/// them (GAP-001).
const CONTROL_PATH: Unavailable<'static> = Unavailable {
    owner: "gungnir-sensor-management",
    gap: "GAP-001",
};

/// PN-15 keeps its requirements in memory for the session (GAP-005).
const REQUIREMENT_PERSISTENCE: Unavailable<'static> = Unavailable {
    owner: "gungnir-store",
    gap: "GAP-005",
};

/// The status these tests give PN-11's laydown-preview line when they are not
/// exercising it directly: nothing selected, the state every one of these fixtures is
/// in unless a test says otherwise.
const LAYDOWN_COMPARISON: crate::panels::coverage_layers::LaydownComparison<'static> =
    crate::panels::coverage_layers::LaydownComparison::NothingSelected;

/// The dead-spot controls these tests give PN-11 when they are not exercising them
/// (GAP-246): shown, one height, no DEM.
const DEAD_SPOTS: crate::panels::coverage_layers::DeadSpotsView<'static> =
    crate::panels::coverage_layers::DeadSpotsView {
        visible: true,
        heights_agl_m: &[100.0],
        selected_height: 0,
        sensors: &[],
        for_sensor: None,
        status: crate::panels::coverage_layers::DeadSpotStatus::NotComputed {
            reason: "no DEM is configured in this baseline (terrain)",
        },
    };

/// PN-06's whole reason for existing: an empty queue that says *why*. The reason has to
/// be on screen, not merely in the view struct.
#[test]
fn the_empty_approval_queue_draws_its_reason() {
    use crate::panels::approval_queue::{
        render_approval_queue, ApprovalQueueView, EmptyBecause, QueueOrder,
    };

    let view = ApprovalQueueView {
        rows: &[],
        order: QueueOrder::TimeThenPriority,
        empty_because: EmptyBecause::NoPlanProduced {
            because: "the allocator has produced no assignment",
        },
        selected: None,
        may_decide: true,
        role: "Operator",
        handoffs: &[],
        now: gungnir_model::MissionTime(0.0),
        authority: crate::panels::approval_queue::QueueAuthority::ThisDesktop,
        decided: &[],
        cannot_decide: None,
        held_layers: &[],
    };
    let probe = RenderProbe::new();
    let (clicked, frame) =
        probe.draw(|ui| render_approval_queue(ui, &theme::Palette::day(), &view));

    assert_eq!(
        clicked,
        Some(None),
        "nothing was clicked, so nothing selected"
    );
    assert!(
        frame.says("the allocator has produced no assignment"),
        "the queue drew no reason for being empty: {}",
        frame.joined()
    );
    assert!(
        frame.says("not the same as nothing needing a decision"),
        "the queue did not draw what its emptiness is *not*: {}",
        frame.joined()
    );
}

/// **PN-06's rule: a decided handoff stays visible until an effector has it** (GAP-040).
///
/// Rendered against the worst case rather than the easy one -- an empty queue, which is
/// the state this build is always in and the state a decided handoff arrives in, since
/// the item leaves the queue the instant the decision is recorded. If the section were
/// drawn inside the non-empty branch this would pass on a view struct and draw nothing.
///
/// The delivered handoff is asserted *absent*: a list that never lets go is a list nobody
/// reads, and "until delivered" is a rule in both directions.
#[test]
fn the_approval_queue_keeps_a_handoff_visible_until_it_is_delivered() {
    use crate::panels::approval_queue::{
        render_approval_queue, ApprovalQueueView, EmptyBecause, QueueOrder,
    };
    use crate::panels::handoff::HandoffRow;
    use gungnir_model::handoff::DeliveryState;
    use gungnir_model::MissionTime;

    let manual = DeliveryState::Manual;
    let queued = DeliveryState::Undelivered {
        since: MissionTime(100.0),
    };
    let done = DeliveryState::Delivered {
        at: MissionTime(110.0),
    };
    let row = |decision: u128, endpoint, delivery| HandoffRow {
        decision: gungnir_model::DecisionId(decision),
        plan: gungnir_model::PlanId(decision + 500),
        endpoint,
        operator: "nobody signed in",
        role: "Operator",
        issued: MissionTime(100.0),
        delivery,
        reports: &[],
    };
    let handoffs = [
        row(11, None, &manual),
        row(12, Some("battery-2"), &queued),
        row(13, Some("battery-3"), &done),
    ];
    let view = ApprovalQueueView {
        rows: &[],
        order: QueueOrder::TimeThenPriority,
        empty_because: EmptyBecause::NothingPending,
        selected: None,
        may_decide: true,
        role: "Operator",
        handoffs: &handoffs,
        now: MissionTime(142.0),
        authority: crate::panels::approval_queue::QueueAuthority::ThisDesktop,
        decided: &[],
        cannot_decide: None,
        held_layers: &[],
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_approval_queue(ui, &theme::Palette::day(), &view));

    assert!(
        frame.says("Decided, not yet delivered"),
        "the queue emptied and took the outstanding handoffs with it: {}",
        frame.joined()
    );
    assert!(
        frame.says("radio call"),
        "the manual handoff vanished, which is the case `needs_attention` would hide: {}",
        frame.joined()
    );
    assert!(
        frame.says("not a fault"),
        "the manual handoff was not stated as the arrangement it is: {}",
        frame.joined()
    );
    assert!(
        frame.says("for 42 s"),
        "the undelivered handoff drew no age: {}",
        frame.joined()
    );
    assert!(
        !frame.says("Delivered to battery-3"),
        "a delivered handoff stayed on the operator's list: {}",
        frame.joined()
    );
}

/// The other half of the rule: with nothing outstanding the section draws nothing at all,
/// so it never becomes the standing line an operator learns to skip.
#[test]
fn the_approval_queue_is_silent_when_every_handoff_is_delivered() {
    use crate::panels::approval_queue::{
        render_approval_queue, ApprovalQueueView, EmptyBecause, QueueOrder,
    };
    use crate::panels::handoff::HandoffRow;
    use gungnir_model::handoff::DeliveryState;
    use gungnir_model::MissionTime;

    let done = DeliveryState::Delivered {
        at: MissionTime(110.0),
    };
    let handoffs = [HandoffRow {
        decision: gungnir_model::DecisionId(11),
        plan: gungnir_model::PlanId(511),
        endpoint: Some("battery-2"),
        operator: "operator 7",
        role: "Operator",
        issued: MissionTime(100.0),
        delivery: &done,
        reports: &[],
    }];
    let view = ApprovalQueueView {
        rows: &[],
        order: QueueOrder::TimeThenPriority,
        empty_because: EmptyBecause::NothingPending,
        selected: None,
        may_decide: true,
        role: "Operator",
        handoffs: &handoffs,
        now: MissionTime(142.0),
        authority: crate::panels::approval_queue::QueueAuthority::ThisDesktop,
        decided: &[],
        cannot_decide: None,
        held_layers: &[],
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_approval_queue(ui, &theme::Palette::day(), &view));
    assert!(
        !frame.says("Decided, not yet delivered"),
        "an empty outstanding list drew a heading: {}",
        frame.joined()
    );
}

/// **D-61, on screen**: PN-06 names a minted plan by its short tag, the random end of the
/// UUID, and never draws the whole identifier; PN-07 draws the whole identifier with a
/// control that copies it, because the decision dialog is where a person quotes the plan
/// to somebody else (GAP-130). A seed's plan number is drawn as the seed gives it.
#[test]
// One test end to end, because what it asserts is that the *same* identifier is drawn
// two different ways on two panels: splitting it would leave each half unable to say
// what the other drew. It crossed the pedantic line limit when GAP-133 gave both views
// their new fields.
#[allow(clippy::too_many_lines)]
fn the_queue_draws_a_plans_tag_and_the_dialog_its_whole_identifier() {
    use crate::panels::approval_queue::{
        render_approval_queue, ApprovalQueueView, EmptyBecause, PendingId, QueueOrder, QueueRow,
        TimeRemaining, Verdict,
    };
    use crate::panels::decision_dialog::{
        render_decision_dialog, DecisionDialogState, DecisionDialogView, DecisionRoute,
        OperatorIdentity,
    };
    use crate::panels::identifier::COPY_LABEL;
    use gungnir_model::PlanId;

    // Two v7-shaped identifiers minted in the same millisecond: the leading digits agree
    // and only the end tells them apart, which is why the tag is the end.
    let minted = PlanId(0x0199_5a3b_7c2d_7e4f_8a1b_2c3d_9f3a_61c2);
    let sibling = PlanId(0x0199_5a3b_7c2d_7e4f_8a1b_2c3d_0b77_4e10);
    let full = "01995a3b-7c2d-7e4f-8a1b-2c3d9f3a61c2";
    let row = |id: u128, plan_id: PlanId| QueueRow {
        id: PendingId(id),
        plan_id,
        assignments: 1,
        verdict: Verdict::RequiresHumanApproval,
        time_remaining: TimeRemaining::Seconds(40.0),
        pre_delegated: false,
        may_decide: true,
        offered_to: &[],
        escalated_from: None,
        basis: gungnir_model::PlanBasis::Exact,
    };
    let rows = [
        row(1, minted),
        row(2, sibling),
        row(3, PlanId(1183)), // a rehearsal seed's plan
    ];
    let queue = ApprovalQueueView {
        rows: &rows,
        order: QueueOrder::TimeThenPriority,
        empty_because: EmptyBecause::NothingPending,
        selected: None,
        may_decide: true,
        role: "Operator",
        handoffs: &[],
        now: gungnir_model::MissionTime(0.0),
        authority: crate::panels::approval_queue::QueueAuthority::ThisDesktop,
        decided: &[],
        cannot_decide: None,
        held_layers: &[],
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_approval_queue(ui, &theme::Palette::day(), &queue));
    assert!(
        frame.says("#\u{2026}9f3a61c2") && frame.says("#\u{2026}0b774e10"),
        "PN-06 did not tag its rows by the end of each identifier: {}",
        frame.joined()
    );
    assert!(
        !frame.says(full) && !frame.says("01995a3b"),
        "PN-06 drew a whole identifier, or its timestamp end: {}",
        frame.joined()
    );
    assert!(
        frame.says("#1183"),
        "a seed's plan was not drawn as the seed gives it: {}",
        frame.joined()
    );

    let under_decision = row(1, minted);
    let dialog = DecisionDialogView {
        row: &under_decision,
        rationale: Err(Unavailable {
            owner: "gungnir-assessment",
            gap: "GAP-028",
        }),
        alternatives: Section::Unavailable(Unavailable {
            owner: "gungnir-decision",
            gap: "GAP-032",
        }),
        cost: Err(Unavailable {
            owner: "gungnir-assessment",
            gap: "GAP-028",
        }),
        degraded: &[],
        engines: &["control status", "authority"],
        caveats: &[],
        operator: OperatorIdentity::Unattributed {
            role: "Operator",
            gap: "GAP-057",
        },
        may_accept: true,
        may_override: true,
        route: DecisionRoute::ThisDesktop,
        answer: None,
        rehearsal: None,
    };
    let mut state = DecisionDialogState::default();
    let (_, frame) =
        probe.draw(|ui| render_decision_dialog(ui, &theme::Palette::day(), &dialog, &mut state));
    let at_id = frame.position_of(full).unwrap_or_else(|| {
        panic!(
            "PN-07 did not draw the whole identifier: {}",
            frame.joined()
        )
    });
    let at_copy = frame
        .position_of(COPY_LABEL)
        .unwrap_or_else(|| panic!("PN-07 drew no copy control: {}", frame.joined()));
    assert_eq!(
        at_copy,
        at_id + 1,
        "the copy control is not beside the identifier it copies: {}",
        frame.joined()
    );
}

/// D-61 on PN-20: the handoff record carries the whole decision and plan identifiers, each
/// with a copy control, because it is the after-action account a person quotes from.
#[test]
fn the_handoff_record_draws_whole_identifiers_with_copy_controls() {
    use crate::panels::audit::{render_audit, AuditView, SessionLine, SignInDraft};
    use crate::panels::handoff::HandoffRow;
    use gungnir_model::handoff::DeliveryState;
    use gungnir_model::{DecisionId, MissionTime, PlanId};

    let manual = DeliveryState::Manual;
    let handoffs = [HandoffRow {
        decision: DecisionId(0x0199_5a3b_7c2d_7e4f_8a1b_2c3d_9f3a_61c2),
        plan: PlanId(0x0199_5a3b_7c2c_7a00_9b00_0000_1111_2222),
        endpoint: None,
        operator: "operator 7",
        role: "Operator",
        issued: MissionTime(100.0),
        delivery: &manual,
        reports: &[],
    }];
    let view = AuditView {
        session: SessionLine::NobodySignedIn,
        accounts: Ok(&[]),
        audit: &[],
        handoffs: &handoffs,
        now: MissionTime(200.0),
        can_sign_in: true,
        can_assign_roles: true,
        record: crate::panels::audit::AuditRecordView::NOT_VERIFIED,
        node: None,
    };
    let probe = RenderProbe::new();
    let mut draft = SignInDraft::default();
    let (_, frame) = probe.draw(|ui| render_audit(ui, &theme::Palette::day(), &view, &mut draft));
    assert!(
        frame.says("Decision \u{2026}9f3a61c2"),
        "the record's headline did not tag the decision: {}",
        frame.joined()
    );
    for full in [
        "01995a3b-7c2d-7e4f-8a1b-2c3d9f3a61c2",
        "01995a3b-7c2c-7a00-9b00-000011112222",
    ] {
        let at = frame
            .position_of(full)
            .unwrap_or_else(|| panic!("PN-20 did not draw {full}: {}", frame.joined()));
        assert!(
            frame
                .texts
                .get(at + 1)
                .is_some_and(|t| t == crate::panels::identifier::COPY_LABEL),
            "no copy control beside {full}: {}",
            frame.joined()
        );
    }
}

/// The safety property from PN-07's module documentation, checked against draw order
/// rather than asserted in a comment: accept comes after reject and override, so
/// neither the reflexive click nor the reflexive keyboard traversal lands on it.
#[test]
fn the_decision_dialog_draws_accept_last() {
    use crate::panels::approval_queue::{PendingId, QueueRow, TimeRemaining, Verdict};
    use crate::panels::decision_dialog::{
        render_decision_dialog, DecisionDialogState, DecisionDialogView, DecisionRoute,
        OperatorIdentity,
    };

    let row = QueueRow {
        id: PendingId(1),
        plan_id: gungnir_model::PlanId(9),
        assignments: 2,
        verdict: Verdict::RequiresHumanApproval,
        time_remaining: TimeRemaining::Seconds(18.0),
        pre_delegated: false,
        may_decide: true,
        offered_to: &[],
        escalated_from: None,
        basis: gungnir_model::PlanBasis::Exact,
    };
    let view = DecisionDialogView {
        row: &row,
        rationale: Err(Unavailable {
            owner: "gungnir-assessment",
            gap: "GAP-028",
        }),
        alternatives: Section::Unavailable(Unavailable {
            owner: "gungnir-decision",
            gap: "GAP-032",
        }),
        cost: Err(Unavailable {
            owner: "gungnir-assessment",
            gap: "GAP-028",
        }),
        degraded: &[],
        engines: &["control status", "authority"],
        caveats: &[],
        operator: OperatorIdentity::Unattributed {
            role: "Operator",
            gap: "GAP-057",
        },
        may_accept: true,
        may_override: true,
        route: DecisionRoute::ThisDesktop,
        answer: None,
        rehearsal: None,
    };

    let probe = RenderProbe::new();
    let mut state = DecisionDialogState::default();
    let (choice, frame) =
        probe.draw(|ui| render_decision_dialog(ui, &theme::Palette::day(), &view, &mut state));

    assert_eq!(
        choice,
        Some(None),
        "a dialog that was drawn and not clicked returned a decision"
    );

    let reject = frame
        .position_of("Reject plan")
        .unwrap_or_else(|| panic!("no reject control drawn: {}", frame.joined()));
    let over = frame
        .position_of("Override with a substitute")
        .unwrap_or_else(|| panic!("no override control drawn: {}", frame.joined()));
    let accept = frame
        .position_of("Accept plan")
        .unwrap_or_else(|| panic!("no accept control drawn: {}", frame.joined()));
    assert!(
        reject < accept && over < accept,
        "accept was not drawn last: reject at {reject}, override at {over}, accept at \
         {accept}"
    );

    // And the attribution warning is on screen every time, not only in the type.
    assert!(
        frame.says("no operator identity"),
        "the dialog did not say the record will name nobody: {}",
        frame.joined()
    );
}

/// With a degraded condition in force the dialog draws the gate rather than silently
/// disabling a button.
#[test]
fn a_degraded_decision_draws_why_accept_is_shut() {
    use crate::panels::approval_queue::{PendingId, QueueRow, TimeRemaining, Verdict};
    use crate::panels::decision_dialog::{
        render_decision_dialog, DecisionDialogState, DecisionDialogView, DecisionRoute, Degraded,
        OperatorIdentity,
    };

    let row = QueueRow {
        id: PendingId(1),
        plan_id: gungnir_model::PlanId(9),
        assignments: 1,
        verdict: Verdict::RequiresHumanApproval,
        time_remaining: TimeRemaining::NoExpiryConfigured,
        pre_delegated: false,
        may_decide: true,
        offered_to: &[],
        escalated_from: None,
        basis: gungnir_model::PlanBasis::Exact,
    };
    let degraded = [Degraded {
        subsystem: "tracking",
        detail: "the tracking pipeline is not running",
    }];
    let unavailable = Unavailable {
        owner: "gungnir-assessment",
        gap: "GAP-028",
    };
    let view = DecisionDialogView {
        row: &row,
        rationale: Err(unavailable),
        alternatives: Section::Unavailable(unavailable),
        cost: Err(unavailable),
        degraded: &degraded,
        engines: &["control status"],
        caveats: &["the no-go geofence check, because no fences are configured"],
        operator: OperatorIdentity::Unattributed {
            role: "Operator",
            gap: "GAP-057",
        },
        may_accept: true,
        may_override: false,
        route: DecisionRoute::ThisDesktop,
        answer: None,
        rehearsal: None,
    };

    let probe = RenderProbe::new();
    let mut state = DecisionDialogState::default();
    let (_, frame) =
        probe.draw(|ui| render_decision_dialog(ui, &theme::Palette::day(), &view, &mut state));

    assert!(
        frame.says("Acknowledge the degraded conditions"),
        "the dialog shut accept without saying why: {}",
        frame.joined()
    );
    assert!(
        frame.says("the tracking pipeline is not running"),
        "the degraded condition itself was not drawn: {}",
        frame.joined()
    );
    assert!(
        frame.says("Could not fail"),
        "the vacuous policy check was not drawn: {}",
        frame.joined()
    );
    assert!(
        frame.says("Overriding needs a higher authority"),
        "a role that may not override was given no reason: {}",
        frame.joined()
    );
}

/// PN-12's governing sentence. It is drawn before anything else because it decides how
/// everything below should be read.
#[test]
fn the_replay_panel_draws_that_it_does_not_rebuild_the_picture() {
    use crate::panels::replay::{render_replay, PlayRate, ReplayView};

    let view = ReplayView {
        sessions: &[],
        open: None,
        rate: PlayRate::Paused,
        reconstruction: Unavailable {
            owner: "gungnir-replay",
            gap: "GAP-045",
        },
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_replay(ui, &theme::Palette::day(), &view));

    assert!(
        frame.says("does not rebuild"),
        "the replay panel did not warn that the picture stays live: {}",
        frame.joined()
    );
    assert!(frame.says("GAP-045"), "{}", frame.joined());
    let warning = frame
        .position_of("does not rebuild")
        .unwrap_or_else(|| panic!("{}", frame.joined()));
    let sessions = frame
        .position_of("Sessions in this journal")
        .unwrap_or_else(|| panic!("{}", frame.joined()));
    assert!(
        warning < sessions,
        "the warning must come before the controls it governs"
    );
}

/// PN-13 keeps an expiry apart from a rejection on the page, not only in the count.
#[test]
fn the_reports_panel_draws_expiries_as_not_rejections() {
    use crate::panels::reports::{
        render_reports, CountLine, ExportState, MeasureLine, MeasureLineValue, ReportsView,
        ReviewDraft, ReviewView,
    };

    let counts = [
        CountLine {
            label: "Decisions",
            value: 3,
            note: Some("a person chose"),
        },
        CountLine {
            label: "Expired",
            value: 2,
            note: Some("not rejections: nobody decided"),
        },
    ];
    let measures = [
        MeasureLine {
            id: "MOE-01".into(),
            name: "Defended-asset protection".into(),
            target: "0.95".into(),
            value: MeasureLineValue::NotComputable {
                reason: "needs the scenario's ground truth".into(),
            },
            note: None,
        },
        MeasureLine {
            id: "MOE-02".into(),
            name: "No fratricide".into(),
            target: "0".into(),
            value: MeasureLineValue::NoInstances {
                of: "engagements".into(),
            },
            note: None,
        },
        MeasureLine {
            id: "MOE-13".into(),
            name: "Intelligence product timeliness".into(),
            target: "0.95 of scheduled".into(),
            value: MeasureLineValue::Fraction {
                value: 2.0 / 3.0,
                numerator: 2,
                denominator: 3,
            },
            note: None,
        },
    ];
    let view = ReportsView {
        session: Some(42),
        counts: Some(&counts),
        first_event_s: Some(0.0),
        last_event_s: Some(10.0),
        metrics: Err(Unavailable {
            owner: "gungnir-scenario",
            gap: "GAP-045",
        }),
        measures: Some(&measures),
        export: ExportState::Available {
            marking: "Internal",
            path: "reports/",
            inputs: "3 internal, 0 to parties, 0 to all peers",
        },
        last_export: None,
        nothing_recorded: false,
        review: ReviewView {
            case: None,
            cannot_open: None,
        },
        order_of_battle: None,
        pattern_of_life: None,
    };
    let probe = RenderProbe::new();
    let mut draft = ReviewDraft::default();
    let (_, frame) = probe.draw(|ui| render_reports(ui, &theme::Palette::day(), &view, &mut draft));

    assert!(frame.says("not rejections"), "{}", frame.joined());
    // GAP-047: a computed measure shows its basis and target; a refused one its reason.
    assert!(frame.says("0.67 (2 of 3)"), "{}", frame.joined());
    assert!(frame.says("target 0.95"), "{}", frame.joined());
    assert!(frame.says("not computable: needs"), "{}", frame.joined());
    assert!(
        frame.says("no engagements in this session"),
        "{}",
        frame.joined()
    );
    assert!(
        frame.says("42"),
        "the report did not name the journal its figures came from: {}",
        frame.joined()
    );
    // The catalogue is its own section with a target on every row, so a measure cannot
    // be read as a count (GAP-047 built it; the separation is what the probe checks).
    assert!(
        frame.says("Measures") && frame.says("Event counts") && frame.says("target 0"),
        "the measures catalogue was not distinguished from the counts: {}",
        frame.joined()
    );
    // GAP-062: the marking travels inside the file, and the panel says what produced it.
    assert!(
        frame.says("Marking from 3 internal"),
        "the marking's inputs were not drawn: {}",
        frame.joined()
    );
}

/// PN-13 draws the pattern of life with its denominator (GAP-025, DN-19): an hour with
/// six sightings in it means one thing out of forty sessions and another out of two, and
/// a busiest hour drawn without the sessions behind it is the chart that gets quoted as
/// a habit.
#[test]
fn the_reports_panel_draws_the_pattern_of_life_with_its_denominator() {
    use crate::panels::reports::{
        render_reports, ExportState, PatternOfLifeLine, ReportsView, ReviewDraft, ReviewView,
    };

    let view = ReportsView {
        session: Some(42),
        counts: None,
        first_event_s: None,
        last_event_s: None,
        metrics: Err(Unavailable {
            owner: "gungnir-scenario",
            gap: "GAP-045",
        }),
        measures: None,
        export: ExportState::Available {
            marking: "Internal",
            path: "reports/",
            inputs: "0 internal, 0 to parties, 0 to all peers",
        },
        last_export: None,
        nothing_recorded: false,
        review: ReviewView {
            case: None,
            cannot_open: None,
        },
        order_of_battle: None,
        pattern_of_life: Some(PatternOfLifeLine {
            sessions: 40,
            sessions_with_activity: 6,
            busiest: Some((14, 11)),
            routes: 2,
            caveat: None,
        }),
    };
    let probe = RenderProbe::new();
    let mut draft = ReviewDraft::default();
    let (_, frame) = probe.draw(|ui| render_reports(ui, &theme::Palette::day(), &view, &mut draft));

    assert!(frame.says("Pattern of life"), "{}", frame.joined());
    assert!(
        frame.says("6 of 40 session(s)"),
        "the busiest hour was drawn without its denominator: {}",
        frame.joined()
    );
    assert!(frame.says("14:00"), "{}", frame.joined());
    assert!(frame.says("2 recurring route(s)"), "{}", frame.joined());

    // Nothing seen is drawn as that, not as a pattern of no activity.
    let quiet = ReportsView {
        pattern_of_life: Some(PatternOfLifeLine {
            sessions: 40,
            sessions_with_activity: 0,
            busiest: None,
            routes: 0,
            caveat: Some("session 7: the journal could not be read"),
        }),
        ..view
    };
    let (_, frame) =
        probe.draw(|ui| render_reports(ui, &theme::Palette::day(), &quiet, &mut draft));
    assert!(
        frame.says("Nothing was seen in any of the 40 session(s)"),
        "{}",
        frame.joined()
    );
    assert!(
        frame.says("the journal could not be read"),
        "a partial fold was drawn as a complete one: {}",
        frame.joined()
    );
}

/// PN-14's most important sentence: applying does not swap the baseline under the
/// running session.
#[test]
fn the_config_editor_draws_when_an_apply_takes_effect() {
    use crate::panels::config_editor::{
        render_config_editor, ApplyState, Candidate, ConfigEditorView, ConfigSection,
        GovernedProfiles, Validation,
    };

    let sections = [ConfigSection {
        name: "Control status",
        count: Some(2),
        summary: "per effector layer",
    }];
    let view = ConfigEditorView {
        version: 1,
        revision: 0,
        sections: &sections,
        in_force: Validation::Valid,
        validity: None,
        candidate: Some(Candidate {
            path: "baseline.json",
            version: 1,
            revision: 0,
            validation: Validation::NotRun,
        }),
        apply: ApplyState::PersistOnly,
        audit: &[],
        profiles: GovernedProfiles::NothingDeclared,
        editing: Unavailable {
            owner: "gungnir-ui",
            gap: "GAP-071",
        },
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_config_editor(ui, &theme::Palette::day(), &view));

    assert!(
        frame.says("until the desktop is restarted"),
        "the panel did not say when an apply takes effect: {}",
        frame.joined()
    );
    assert!(
        frame.says("not validated"),
        "an unvalidated candidate was not marked as such: {}",
        frame.joined()
    );
    assert!(
        frame.says("validate it before applying"),
        "the panel shut apply without saying why: {}",
        frame.joined()
    );
}

/// The track table's score column says it is unavailable rather than showing zeros.
#[test]
fn the_track_table_draws_the_score_column_as_unavailable() {
    use crate::panels::track_table::{render_track_table, TrackTableView};
    use gungnir_model::MissionTime;

    let vocab = vocabulary();
    let view = TrackTableView {
        tracks: &[],
        now: MissionTime(0.0),
        assignments: &[],
        scores: Err(Unavailable {
            owner: "gungnir-assessment",
            gap: "GAP-028",
        }),
        selected: None,
        msl_heights: &[],
        unknown_heights: &[],
        vocabulary: &vocab,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_track_table(ui, &theme::Palette::day(), &view));

    assert!(
        frame.says("Score (n/a)"),
        "the score column did not mark itself unavailable: {}",
        frame.joined()
    );
    assert!(frame.says("GAP-028"), "{}", frame.joined());
}

/// The status strip's alert count says the lifecycle states are not tracked, rather
/// than printing three zeros that would read as "nothing new".
#[test]
fn the_status_strip_draws_an_unclassified_alert_total() {
    use crate::panels::status_strip::{
        render_status_strip, AlertSummary, BackendStatus, ClockSource, StatusStripView,
    };
    use gungnir_model::{MissionTime, SystemHealth};

    let vocab = vocabulary();
    let view = StatusStripView {
        identity: crate::panels::status_strip::IdentityLine::None,
        encryption: EncryptionState::NotConfigured,
        validity: BaselineValidity::NoWindowConfigured,
        profile: None,
        backend: BackendStatus::Embedded,
        session: None,
        mission_time: MissionTime(12.0),
        clock_source: ClockSource::Wall,
        clock_skew_s: None,
        health: SystemHealth::default(),
        control_status: &[],
        delegations: &[],
        alerts: AlertSummary::Unclassified { total: 4 },
        role: "Operator",
        vocabulary: &vocab,
        coverage: crate::panels::status_strip::CoverageStatus::NoApproaches,
        rehearsal: None,
        operator: OperatorLine::NobodySignedIn,
        queue: crate::panels::approval_queue::QueueAuthority::ThisDesktop,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_status_strip(ui, &theme::Palette::day(), &view));

    assert!(
        frame.says("Alerts 4"),
        "the strip drew no alert total: {}",
        frame.joined()
    );
    assert!(
        !frame.says("0 new"),
        "the strip printed lifecycle counts it does not have: {}",
        frame.joined()
    );
}

/// PN-04's three unavailable sections name their crates on screen, so an operator sees
/// what is missing rather than an empty card.
#[test]
fn the_evidence_card_draws_which_crate_owes_each_section() {
    use crate::panels::track_detail::{render_evidence_card, EvidenceCardView};
    use gungnir_model::{
        Classification, MissionTime, Provenance, Quality, Releasability, TrackId, TrackStatus,
        TrackView,
    };

    let track = TrackView {
        id: TrackId(7),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::zeros(),
        covariance: nalgebra::SMatrix::identity(),
        classification: Classification::Hostile,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(0.0),
        releasability: Releasability::default(),
    };
    let vocab = vocabulary();
    let view = EvidenceCardView {
        track: &track,
        evidence: Section::Unavailable(Unavailable {
            owner: "gungnir-identification",
            gap: "GAP-010",
        }),
        lineage: Section::Unavailable(Unavailable {
            owner: "gungnir-identity",
            gap: "GAP-019",
        }),
        factors: Section::Unavailable(Unavailable {
            owner: "gungnir-assessment",
            gap: "GAP-028",
        }),
        designation_available: false,
        vocabulary: &vocab,
        approach: crate::panels::unavailable::Section::Empty {
            reason: "not predicted in this test",
        },
        warnings: &[],
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_evidence_card(ui, &theme::Palette::day(), &view));

    for gap in ["GAP-010", "GAP-019", "GAP-028"] {
        assert!(
            frame.says(gap),
            "the card did not name {gap} on screen: {}",
            frame.joined()
        );
    }
    assert!(
        frame.says("Internal only"),
        "the releasability marking was not drawn: {}",
        frame.joined()
    );
    assert!(
        frame.says("No designation control"),
        "the card offered no explanation for the missing control: {}",
        frame.joined()
    );
}

/// Every panel draws without panicking at a narrow width, which is what a docked slot
/// in a rearranged tree actually gives them.
///
/// The queue is given an outstanding handoff rather than none, because a narrow pane is
/// exactly where PN-06's *stays visible until delivered* section would be lost: laid out
/// past the right edge, egui stops painting a widget entirely, so a section that
/// overflowed would disappear rather than clip.
#[test]
fn the_panels_survive_a_narrow_slot() {
    use crate::panels::approval_queue::{
        render_approval_queue, ApprovalQueueView, EmptyBecause, QueueOrder,
    };
    use crate::panels::handoff::HandoffRow;
    use crate::panels::track_table::{render_track_table, TrackTableView};
    use gungnir_model::handoff::DeliveryState;
    use gungnir_model::MissionTime;

    let probe = RenderProbe::with_size(220.0, 400.0);
    let manual = DeliveryState::Manual;
    let handoffs = [HandoffRow {
        decision: gungnir_model::DecisionId(11),
        plan: gungnir_model::PlanId(511),
        endpoint: None,
        operator: "nobody signed in",
        role: "Analyst",
        issued: MissionTime(100.0),
        delivery: &manual,
        reports: &[],
    }];
    let queue = ApprovalQueueView {
        rows: &[],
        order: QueueOrder::TimeOnly {
            priority: Unavailable {
                owner: "gungnir-assessment",
                gap: "GAP-028",
            },
        },
        empty_because: EmptyBecause::NothingPending,
        selected: None,
        may_decide: false,
        role: "Analyst",
        handoffs: &handoffs,
        now: MissionTime(142.0),
        authority: crate::panels::approval_queue::QueueAuthority::ThisDesktop,
        decided: &[],
        cannot_decide: None,
        held_layers: &[],
    };
    let (_, frame) = probe.draw(|ui| render_approval_queue(ui, &theme::Palette::day(), &queue));
    assert!(!frame.texts.is_empty(), "the queue drew nothing at 220 px");
    assert!(
        frame.says("Decided, not yet delivered"),
        "the outstanding handoff was laid out off the pane at 220 px: {}",
        frame.joined()
    );

    let vocab = vocabulary();
    let table = TrackTableView {
        tracks: &[],
        now: MissionTime(0.0),
        assignments: &[],
        scores: Err(Unavailable {
            owner: "gungnir-assessment",
            gap: "GAP-028",
        }),
        selected: None,
        msl_heights: &[],
        unknown_heights: &[],
        vocabulary: &vocab,
    };
    let (_, frame) = probe.draw(|ui| render_track_table(ui, &theme::Palette::day(), &table));
    assert!(!frame.texts.is_empty(), "the table drew nothing at 220 px");
}

/// GAP-070's closing action, as a rendered assertion: every term the interface can show
/// resolves to a word, and none of them is a Rust variant name.
///
/// The two that matter are checked by name because the doctrine term differs from the
/// variant: the NATO standard identity is *Friend*, and the joint term is *weapons
/// free*. A screen showing `Friendly` or `Free` would be showing an identifier from a
/// type definition.
#[test]
fn the_track_table_draws_the_deployments_words() {
    use crate::panels::track_table::{render_track_table, TrackTableView};
    use gungnir_model::{
        Classification, MissionTime, Provenance, Quality, Releasability, TrackId, TrackStatus,
        TrackView, Vocabulary,
    };
    use std::collections::BTreeMap;

    fn track(id: u64, classification: Classification) -> TrackView {
        TrackView {
            id: TrackId(id),
            status: TrackStatus::Confirmed,
            state: nalgebra::SVector::zeros(),
            covariance: nalgebra::SMatrix::identity(),
            classification,
            provenance: Provenance::default(),
            quality: Quality::default(),
            mission_time: MissionTime(0.0),
            releasability: Releasability::default(),
        }
    }
    let tracks = [
        track(1, Classification::Friendly),
        track(2, Classification::Hostile),
    ];

    let defaults = Vocabulary::default();
    let view = TrackTableView {
        tracks: &tracks,
        now: MissionTime(0.0),
        assignments: &[],
        scores: Err(Unavailable {
            owner: "gungnir-assessment",
            gap: "GAP-028",
        }),
        selected: None,
        msl_heights: &[],
        unknown_heights: &[],
        vocabulary: &defaults,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_track_table(ui, &theme::Palette::day(), &view));
    assert!(
        frame.says("Friend"),
        "the table drew no affiliation for a friendly track: {}",
        frame.joined()
    );
    assert!(
        !frame.says("Friendly"),
        "the table drew the Rust variant name rather than the NATO identity: {}",
        frame.joined()
    );

    // And a deployment's own word reaches the screen.
    let renamed = Vocabulary {
        overrides: BTreeMap::from([("classification.friendly".to_owned(), "Blue".to_owned())]),
    };
    let view = TrackTableView {
        vocabulary: &renamed,
        ..view
    };
    let (_, frame) = probe.draw(|ui| render_track_table(ui, &theme::Palette::day(), &view));
    assert!(
        frame.says("Blue"),
        "an override did not reach the screen: {}",
        frame.joined()
    );
    assert!(
        frame.says("Hostile"),
        "overriding one term changed another: {}",
        frame.joined()
    );
}

/// GAP-196 (D-124): a track whose last Category 129 report left its height above mean
/// sea level shows its U marked "MSL" in PN-03, and the other tracks' U is unmarked.
#[test]
fn the_track_table_marks_a_height_above_mean_sea_level() {
    use crate::panels::track_table::{render_track_table, TrackTableView};
    use gungnir_model::{
        Classification, MissionTime, Provenance, Quality, Releasability, TrackId, TrackStatus,
        TrackView,
    };

    let track = |id: u64| TrackView {
        id: TrackId(id),
        status: TrackStatus::Confirmed,
        state: nalgebra::SVector::zeros(),
        covariance: nalgebra::SMatrix::identity(),
        classification: Classification::Unknown,
        provenance: Provenance::default(),
        quality: Quality::default(),
        mission_time: MissionTime(0.0),
        releasability: Releasability::default(),
    };
    let tracks = [track(1), track(2), track(3)];
    let vocab = Vocabulary::default();
    let flagged = [TrackId(2)];
    // GAP-199 (D-130): and one whose last report gave no height anybody could place is
    // marked "unknown".
    let unknown = [TrackId(3)];
    let view = TrackTableView {
        tracks: &tracks,
        now: MissionTime(0.0),
        assignments: &[],
        scores: Err(Unavailable {
            owner: "gungnir-assessment",
            gap: "GAP-028",
        }),
        selected: None,
        msl_heights: &flagged,
        unknown_heights: &unknown,
        vocabulary: &vocab,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_track_table(ui, &theme::Palette::day(), &view));
    let marked = frame.texts.iter().filter(|t| t.ends_with(" MSL")).count();
    assert_eq!(marked, 1, "{}", frame.joined());
    let unknown = frame
        .texts
        .iter()
        .filter(|t| t.ends_with(" unknown"))
        .count();
    assert_eq!(unknown, 1, "{}", frame.joined());
}

/// The status strip shows the joint control-status term, which is the case where the
/// variant name reads closest to its own opposite.
#[test]
fn the_status_strip_draws_the_joint_control_status_terms() {
    use crate::panels::status_strip::{
        render_status_strip, AlertSummary, BackendStatus, ClockSource, ControlStatusLine,
        StatusStripView,
    };
    use gungnir_model::{
        EffectorLayer, MissionTime, SystemHealth, Vocabulary, WeaponsControlStatus,
    };

    let lines = [
        ControlStatusLine {
            layer: EffectorLayer::Point,
            status: WeaponsControlStatus::Free,
            configured: true,
            set_by: None,
            can_change: false,
        },
        ControlStatusLine {
            layer: EffectorLayer::SelfDefence,
            status: WeaponsControlStatus::Hold,
            configured: false,
            set_by: None,
            can_change: false,
        },
    ];
    let vocab = Vocabulary::default();
    let view = StatusStripView {
        identity: crate::panels::status_strip::IdentityLine::None,
        encryption: EncryptionState::NotConfigured,
        validity: BaselineValidity::NoWindowConfigured,
        profile: None,
        backend: BackendStatus::Embedded,
        session: None,
        mission_time: MissionTime(0.0),
        clock_source: ClockSource::Wall,
        clock_skew_s: None,
        health: SystemHealth::default(),
        control_status: &lines,
        delegations: &[],
        alerts: AlertSummary::Unclassified { total: 0 },
        role: "Operator",
        vocabulary: &vocab,
        coverage: crate::panels::status_strip::CoverageStatus::NoApproaches,
        rehearsal: None,
        operator: OperatorLine::NobodySignedIn,
        queue: crate::panels::approval_queue::QueueAuthority::ThisDesktop,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_status_strip(ui, &theme::Palette::day(), &view));

    assert!(
        frame.says("Weapons free"),
        "the strip drew a bare 'Free', which reads as its own opposite: {}",
        frame.joined()
    );
    assert!(
        frame.says("Point defence"),
        "the strip drew the layer's variant name: {}",
        frame.joined()
    );
    assert!(
        !frame.says("SelfDefence"),
        "a Rust identifier reached the screen: {}",
        frame.joined()
    );
}

/// PN-10's central claim (DN-11 §5 rule 1): a mode that was asked for is not the mode
/// the sensor is in. Both have to be on screen, distinguishable, or the panel is the
/// health flag that lies.
#[test]
fn a_requested_mode_is_drawn_beside_the_confirmed_one_not_instead_of_it() {
    use crate::panels::sensor_management::{
        render_sensor_management, SensorManagementView, SensorRow, TaskProgress,
    };
    use gungnir_model::SensorMode;

    let vocabulary = vocabulary();
    let rows = [SensorRow {
        id: 1,
        modality: "radar",
        mode: SensorMode::Standby,
        requested: Some(SensorMode::Search),
        task: Some(TaskProgress::Issued),
        calibration_version: "v1",
        max_range_m: 50_000.0,
        contributing: false,
        controllable: true,
    }];
    let view = SensorManagementView {
        sensors: &rows,
        recommendations: Err("not evaluated in this test"),
        may_task: true,
        role: "Sensor manager",
        control_path: CONTROL_PATH,
        vocabulary: &vocabulary,
        last_error: None,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_sensor_management(ui, &theme::Palette::day(), &view));

    assert!(
        frame.says("asked:"),
        "the requested mode was not drawn as a request: {}",
        frame.joined()
    );
    // The confirmed word must still be there. A panel that replaced it with the request
    // would pass a test that only looked for "Search".
    assert!(
        frame.says(vocabulary.sensor_mode(SensorMode::Standby)),
        "the confirmed mode vanished when a request was outstanding: {}",
        frame.joined()
    );
    // And it contributes nothing. A request is not a fact, so the coverage column must
    // not credit it.
    assert!(
        !frame.says("contributing"),
        "a sensor with a merely requested Search was drawn as contributing: {}",
        frame.joined()
    );
}

/// Commanding and recording are two different acts, and the screen has to say which is
/// which. If both columns were called the same thing, the record could not tell an
/// operator later which of the two they did.
#[test]
fn commanding_and_recording_are_labelled_apart() {
    use crate::panels::sensor_management::{
        render_sensor_management, SensorManagementView, SensorRow,
    };
    use gungnir_model::SensorMode;

    let vocabulary = vocabulary();
    let rows = [SensorRow {
        id: 1,
        modality: "radar",
        mode: SensorMode::Standby,
        requested: None,
        task: None,
        calibration_version: "v1",
        max_range_m: 50_000.0,
        contributing: false,
        controllable: false,
    }];
    let view = SensorManagementView {
        sensors: &rows,
        recommendations: Err("not evaluated in this test"),
        may_task: true,
        role: "Sensor manager",
        control_path: CONTROL_PATH,
        vocabulary: &vocabulary,
        last_error: None,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_sensor_management(ui, &theme::Palette::day(), &view));

    assert!(
        frame.says("Command") && frame.says("Record observed"),
        "the two actions were not labelled apart: {}",
        frame.joined()
    );
    // The default baseline configures no control endpoint, so the panel must say the
    // sensors cannot be commanded rather than offering controls that would be refused.
    assert!(
        frame.says("No sensor has a control endpoint configured"),
        "an uncontrollable sensor set drew no reason: {}",
        frame.joined()
    );
    assert!(
        frame.says("no command issued"),
        "a sensor with no task drew nothing in the task column: {}",
        frame.joined()
    );
}

/// A refusal reaches the screen with the adapter's own words, and a timeout does not
/// read as a refusal. Nobody said no to a timed-out command, and after-action review
/// turns on the difference.
#[test]
fn a_refusal_and_a_timeout_draw_as_different_things() {
    use crate::panels::sensor_management::{
        render_sensor_management, SensorManagementView, SensorRow, TaskProgress,
    };
    use gungnir_model::SensorMode;

    let vocabulary = vocabulary();
    let base = SensorRow {
        id: 1,
        modality: "radar",
        mode: SensorMode::Standby,
        requested: None,
        task: None,
        calibration_version: "v1",
        max_range_m: 50_000.0,
        contributing: false,
        controllable: true,
    };
    let rows = [
        SensorRow {
            id: 1,
            task: Some(TaskProgress::Failed {
                reason: "transmitter inhibited",
            }),
            ..base
        },
        SensorRow {
            id: 2,
            task: Some(TaskProgress::Unacknowledged),
            ..base
        },
    ];
    let view = SensorManagementView {
        sensors: &rows,
        recommendations: Err("not evaluated in this test"),
        may_task: true,
        role: "Sensor manager",
        control_path: CONTROL_PATH,
        vocabulary: &vocabulary,
        last_error: None,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_sensor_management(ui, &theme::Palette::day(), &view));

    assert!(
        frame.says("transmitter inhibited"),
        "the adapter's reason did not reach the screen: {}",
        frame.joined()
    );
    assert!(
        frame.says("no answer inside the window"),
        "a timed-out command drew no state: {}",
        frame.joined()
    );
    // No variant names anywhere: the failure this guards against is somebody reaching
    // for `format!("{:?}")` on a task state.
    for identifier in ["Unacknowledged", "Acknowledged", "TaskState"] {
        assert!(
            !frame.says(identifier),
            "the Rust identifier {identifier} reached the screen: {}",
            frame.joined()
        );
    }
}

/// PN-15's central claim, and DN-11's: a sensor taking a command is not an answer. The
/// words have to make that unmistakable on screen, because "a sensor is on it" is
/// exactly what somebody skimming would read as "handled".
#[test]
fn a_tasked_requirement_does_not_draw_as_answered() {
    use crate::panels::requirements::{
        render_requirements, AreaChoice, Draft, ListOrigin, Progress, RequirementRow,
        RequirementsView, SensorChoice, Standing,
    };
    use gungnir_model::AssetPriority;

    let rows = [RequirementRow {
        id: 1,
        title: "identify the contact in the harbour",
        priority: AssetPriority::High,
        standing: Standing::Tasked {
            by: "the role on watch; nobody was signed in",
        },
        progress: Progress::Working,
        tasks: 1,
        time_remaining_s: Some(600.0),
    }];
    let areas = [AreaChoice {
        name: "the harbour",
    }];
    let sensors = [SensorChoice {
        id: 1,
        modality: "radar",
        controllable: true,
    }];
    let view = RequirementsView {
        origin: ListOrigin::NothingStated,
        rows: &rows,
        areas: Ok(&areas),
        sensors: &sensors,
        may_concur: true,
        role: "SensorManager",
        operator_session: false,
        persistence: REQUIREMENT_PERSISTENCE,
        last_error: None,
    };
    let mut draft = Draft::default();
    let probe = RenderProbe::new();
    let (_, frame) =
        probe.draw(|ui| render_requirements(ui, &theme::Palette::day(), &view, &mut draft));

    assert!(
        frame.says("not yet answered"),
        "a tasked requirement drew nothing distinguishing it from an answered one: {}",
        frame.joined()
    );
    // And the concurrence does not read as a person, because nobody was signed in.
    assert!(
        frame.says("nobody was signed in"),
        "an unattributed concurrence drew as though somebody had signed in: {}",
        frame.joined()
    );
    // No variant names anywhere.
    for identifier in ["Working", "Tasked", "AssetPriority", "High"] {
        assert!(
            !frame.says(identifier),
            "the Rust identifier {identifier} reached the screen: {}",
            frame.joined()
        );
    }
}

/// A lapse is not a decline, and a requirement with no deadline is not overdue. Both
/// are distinctions an after-action review turns on, and both are easy to draw wrong.
#[test]
fn a_lapse_and_a_missing_deadline_each_draw_as_themselves() {
    use crate::panels::requirements::{
        render_requirements, AreaChoice, Draft, ListOrigin, Progress, RequirementRow,
        RequirementsView, Standing,
    };
    use gungnir_model::AssetPriority;

    let base = RequirementRow {
        id: 1,
        title: "identify the contact",
        priority: AssetPriority::Medium,
        standing: Standing::Lapsed,
        progress: Progress::Untasked,
        tasks: 0,
        time_remaining_s: Some(-30.0),
    };
    let rows = [
        base,
        RequirementRow {
            id: 2,
            standing: Standing::Stated,
            time_remaining_s: None,
            ..base
        },
    ];
    let areas = [AreaChoice {
        name: "the harbour",
    }];
    let view = RequirementsView {
        origin: ListOrigin::NothingStated,
        rows: &rows,
        areas: Ok(&areas),
        sensors: &[],
        may_concur: true,
        role: "SensorManager",
        operator_session: false,
        persistence: REQUIREMENT_PERSISTENCE,
        last_error: None,
    };
    let mut draft = Draft::default();
    let probe = RenderProbe::new();
    let (_, frame) =
        probe.draw(|ui| render_requirements(ui, &theme::Palette::day(), &view, &mut draft));

    assert!(
        frame.says("nobody deciding"),
        "a lapse drew no reason: {}",
        frame.joined()
    );
    assert!(
        !frame.says("declined"),
        "a lapse drew as a decline: {}",
        frame.joined()
    );
    // A requirement nobody set a deadline for is not blank and not overdue.
    assert!(
        frame.says("no time set"),
        "a requirement with no deadline drew an empty cell: {}",
        frame.joined()
    );
}

/// The authority split MT-08 turns on. An analyst is told they may state a requirement
/// and not concur with tasking it, rather than being given a control that would be
/// refused.
#[test]
fn an_analyst_is_told_which_half_of_the_workflow_is_theirs() {
    use crate::panels::requirements::{
        render_requirements, AreaChoice, Draft, ListOrigin, Progress, RequirementRow,
        RequirementsView, SensorChoice, Standing,
    };
    use gungnir_model::AssetPriority;

    let rows = [RequirementRow {
        id: 1,
        title: "identify the contact",
        priority: AssetPriority::Medium,
        standing: Standing::Stated,
        progress: Progress::Untasked,
        tasks: 0,
        time_remaining_s: None,
    }];
    let areas = [AreaChoice {
        name: "the harbour",
    }];
    let sensors = [SensorChoice {
        id: 1,
        modality: "radar",
        controllable: false,
    }];
    let view = RequirementsView {
        origin: ListOrigin::NothingStated,
        rows: &rows,
        areas: Ok(&areas),
        sensors: &sensors,
        may_concur: false,
        role: "IntelligenceAnalyst",
        operator_session: false,
        persistence: REQUIREMENT_PERSISTENCE,
        last_error: None,
    };
    let mut draft = Draft {
        selected: Some(1),
        ..Draft::default()
    };
    let probe = RenderProbe::new();
    let (_, frame) =
        probe.draw(|ui| render_requirements(ui, &theme::Palette::day(), &view, &mut draft));

    assert!(
        frame.says("not authority they hold"),
        "the analyst was not told why they cannot concur: {}",
        frame.joined()
    );
    assert!(
        frame.says("a reason is required"),
        "the decline control did not say what it needs: {}",
        frame.joined()
    );
}

/// A deployment with no defended assets gets a stated reason rather than an empty
/// picker: a requirement over nowhere is worse than none.
#[test]
fn with_no_areas_the_form_says_why_instead_of_offering_one() {
    use crate::panels::requirements::{
        render_requirements, CannotState, Draft, ListOrigin, RequirementsView,
    };

    let view = RequirementsView {
        origin: ListOrigin::NothingStated,
        rows: &[],
        areas: Err(CannotState::NoAreas),
        sensors: &[],
        may_concur: true,
        role: "SensorManager",
        operator_session: false,
        persistence: REQUIREMENT_PERSISTENCE,
        last_error: None,
    };
    let mut draft = Draft::default();
    let probe = RenderProbe::new();
    let (action, frame) =
        probe.draw(|ui| render_requirements(ui, &theme::Palette::day(), &view, &mut draft));

    // `Some(None)`: the panel drew and returned no action. `None` would mean it never
    // ran at all, which is a different failure.
    assert_eq!(action, Some(None), "the empty form produced an action");
    assert!(
        frame.says("no defended assets"),
        "an empty picker drew no reason: {}",
        frame.joined()
    );
    assert!(
        frame.says("Nothing has been asked for yet"),
        "an empty list drew nothing: {}",
        frame.joined()
    );
}

/// DN-22 §5 applied to a security feature: **a system that claimed encryption it was not
/// performing would be worse than one that admits it is not.** The strip has to say so,
/// and has to distinguish a deployment that never configured it from one whose keystore
/// it cannot reach -- only the second is a fault an administrator must act on.
#[test]
fn the_status_strip_says_when_the_journal_is_not_encrypted() {
    use crate::panels::status_strip::{
        render_status_strip, AlertSummary, BackendStatus, ClockSource, CoverageStatus,
        EncryptionState, StatusStripView,
    };

    let vocabulary = vocabulary();
    let base = || StatusStripView {
        identity: crate::panels::status_strip::IdentityLine::None,
        backend: BackendStatus::Embedded,
        session: None,
        mission_time: gungnir_model::MissionTime(0.0),
        clock_source: ClockSource::Wall,
        clock_skew_s: None,
        health: gungnir_model::SystemHealth::default(),
        control_status: &[],
        delegations: &[],
        alerts: AlertSummary::Unclassified { total: 0 },
        role: "Operator",
        vocabulary: &vocabulary,
        coverage: CoverageStatus::NoApproaches,
        encryption: EncryptionState::NotConfigured,
        validity: BaselineValidity::NoWindowConfigured,
        profile: None,
        rehearsal: None,
        operator: OperatorLine::NobodySignedIn,
        queue: crate::panels::approval_queue::QueueAuthority::ThisDesktop,
    };

    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_status_strip(ui, &theme::Palette::day(), &base()));
    assert!(
        frame.says("not encrypted"),
        "an unencrypted journal drew nothing: {}",
        frame.joined()
    );

    // The fault case says more, because it is the one somebody must act on.
    let view = StatusStripView {
        encryption: EncryptionState::UnavailableWritingPlaintext {
            reason: "the keystore is locked",
        },
        ..base()
    };
    let (_, frame) = probe.draw(|ui| render_status_strip(ui, &theme::Palette::day(), &view));
    assert!(frame.says("keystore is locked"), "{}", frame.joined());
    assert!(frame.says("NOT encrypted"), "{}", frame.joined());

    // And when it is on, it says so once and without alarm.
    let view = StatusStripView {
        encryption: EncryptionState::Active,
        ..base()
    };
    let (_, frame) = probe.draw(|ui| render_status_strip(ui, &theme::Palette::day(), &view));
    assert!(frame.says("journal encrypted"), "{}", frame.joined());
    assert!(!frame.says("not encrypted"), "{}", frame.joined());
}

/// PN-01's operator line (GAP-057): who is signed in and for how long, and the two
/// ways nobody is, each in its own words.
#[test]
fn the_strip_says_who_is_signed_in_and_when_nobody_is() {
    use crate::panels::status_strip::{
        render_status_strip, AlertSummary, BackendStatus, BaselineValidity, ClockSource,
        CoverageStatus, EncryptionState, OperatorLine, StatusStripView,
    };
    let vocabulary = gungnir_model::Vocabulary::default();
    let base = |operator| StatusStripView {
        identity: crate::panels::status_strip::IdentityLine::None,
        backend: BackendStatus::Embedded,
        session: None,
        mission_time: gungnir_model::MissionTime(0.0),
        clock_source: ClockSource::Wall,
        clock_skew_s: None,
        health: gungnir_model::SystemHealth::default(),
        control_status: &[],
        delegations: &[],
        alerts: AlertSummary::Unclassified { total: 0 },
        role: "Operator",
        vocabulary: &vocabulary,
        coverage: CoverageStatus::NoApproaches,
        encryption: EncryptionState::NotConfigured,
        validity: BaselineValidity::NoWindowConfigured,
        profile: None,
        rehearsal: None,
        operator,
        queue: crate::panels::approval_queue::QueueAuthority::ThisDesktop,
    };
    let probe = RenderProbe::new();
    let draw = |line| {
        let (_, frame) =
            probe.draw(|ui| render_status_strip(ui, &theme::Palette::day(), &base(line)));
        frame.joined()
    };
    let text = draw(OperatorLine::SignedIn {
        operator: 7,
        role: "Supervisor",
        expires_in_s: Some(3725.0),
    });
    assert!(
        text.contains("Operator 7 (Supervisor), 1h 02m left"),
        "{text}"
    );
    let text = draw(OperatorLine::SignedIn {
        operator: 7,
        role: "Operator",
        expires_in_s: None,
    });
    assert!(
        text.contains("Operator 7 (Operator)") && !text.contains("left"),
        "{text}"
    );
    let text = draw(OperatorLine::NobodySignedIn);
    assert!(text.contains("unattributed"), "{text}");
    let text = draw(OperatorLine::Expired { operator: 7 });
    assert!(text.contains("expired"), "{text}");
    let text = draw(OperatorLine::StoreUnavailable);
    assert!(text.contains("nobody can sign in"), "{text}");
}

/// PN-01's identity element (GAP-211): the days left on this desktop's certificate, and
/// each way it can be in trouble in its own words; nothing at all with no identity.
#[test]
fn the_strip_says_how_long_this_desktops_identity_has_left() {
    use crate::panels::status_strip::{
        render_status_strip, AlertSummary, BackendStatus, BaselineValidity, ClockSource,
        CoverageStatus, EncryptionState, IdentityLine, OperatorLine, StatusStripView,
    };
    let vocabulary = gungnir_model::Vocabulary::default();
    let base = |identity| StatusStripView {
        identity,
        backend: BackendStatus::Embedded,
        session: None,
        mission_time: gungnir_model::MissionTime(0.0),
        clock_source: ClockSource::Wall,
        clock_skew_s: None,
        health: gungnir_model::SystemHealth::default(),
        control_status: &[],
        delegations: &[],
        alerts: AlertSummary::Unclassified { total: 0 },
        role: "Operator",
        vocabulary: &vocabulary,
        coverage: CoverageStatus::NoApproaches,
        encryption: EncryptionState::NotConfigured,
        validity: BaselineValidity::NoWindowConfigured,
        profile: None,
        rehearsal: None,
        operator: OperatorLine::NobodySignedIn,
        queue: crate::panels::approval_queue::QueueAuthority::ThisDesktop,
    };
    let probe = RenderProbe::new();
    let draw = |line| {
        let (_, frame) =
            probe.draw(|ui| render_status_strip(ui, &theme::Palette::day(), &base(line)));
        frame.joined()
    };
    let issued = |days_left, renewal_failure| IdentityLine::Issued {
        name: "desktop-0123456789abcdef",
        days_left,
        renewal_failure,
        warn_days: 7,
    };
    assert!(draw(issued(Some(87), None)).contains("identity 87 d"));
    assert!(draw(issued(Some(5), None)).contains("identity 5 d left"));
    let text = draw(issued(Some(12), Some("the keystore is locked")));
    assert!(text.contains("identity NOT re-issued, 12 d left"), "{text}");
    assert!(draw(issued(Some(-1), None)).contains("identity EXPIRED"));
    assert!(!draw(IdentityLine::None).contains("identity"));
}

/// PN-09's geoid lines (GAP-108, GAP-197) as a deployment with no grid installed draws it; the
/// health-panel tests below are about other lines and take this one as it stands.
const NO_GEOID_GRID: &[crate::panels::sensor_health::GeoidLine<'static>] =
    &[crate::panels::sensor_health::GeoidLine {
        model: "EGM2008",
        verified: false,
        detail: "EGM2008 geoid grid: none installed",
    }];

/// **A silent sensor means four different things and only two of them need somebody.**
/// PN-09 has to keep them apart: collapsing them into a health dot is how a scheduled
/// outage becomes an unnoticed hole, and how a real failure gets shrugged off as "that's
/// the maintenance window" (DN-21 §5, GAP-054).
/// GAP-205: PN-09 says which sensor disciplines the clock and what it is doing, in every
/// state, and says when nothing does.
#[test]
fn the_health_panel_says_which_sensor_disciplines_the_clock() {
    use crate::panels::sensor_health::{
        render_sensor_health, ClockSyncLine, DisciplineLine, SensorHealthView,
    };
    use crate::panels::status_strip::EncryptionState;
    use gungnir_model::{ClockDisciplineState as S, MissionTime};

    let health = gungnir_model::SystemHealth::default();
    let probe = RenderProbe::new();
    let draw = |discipline: Option<DisciplineLine>| {
        let (_, frame) = probe.draw(|ui| {
            render_sensor_health(
                ui,
                &theme::Palette::day(),
                &SensorHealthView {
                    health: &health,
                    encryption: EncryptionState::Active,
                    sensors: &[],
                    clocks: ClockSyncLine {
                        sources_observed: 0,
                        sources_out_of_sync: 0,
                        max_skew_s: 0.0,
                        discipline,
                    },
                    late_data: crate::panels::sensor_health::LateDataLine::default(),
                    detectors: &[],
                    terrain: crate::panels::sensor_health::TerrainLine {
                        masking: false,
                        detail: "no terrain configured",
                    },
                    geoid: NO_GEOID_GRID,
                    point_cloud_registration:
                        crate::panels::sensor_health::PointCloudRegistrationLine::NotConfigured,
                    feeds: &[],
                    cooperative_feeds: &[],
                    peers: &[],
                    bearing_feeds: &[],
                    bearing_pipeline: crate::panels::sensor_health::BearingPipelineLine::default(),
                    exchange: None,
                },
            );
        });
        frame.joined()
    };
    let on = |state| Some(DisciplineLine { sensor: 4, state });

    let text = draw(None);
    assert!(text.contains("not disciplined from any sensor"), "{text}");
    let text = draw(on(S::Waiting));
    assert!(text.contains("disciplined by sensor 4: waiting"), "{text}");
    let text = draw(on(S::Locked {
        offset_s: -39.98,
        samples: 12,
        last_sample: MissionTime(100.0),
    }));
    assert!(
        text.contains("locked; host behind by 39.980 s over 12 report(s)"),
        "{text}"
    );
    let text = draw(on(S::Holdover {
        offset_s: -39.98,
        last_sample: MissionTime(100.0),
    }));
    assert!(text.contains("holding over since T+100 s"), "{text}");
    let text = draw(on(S::Refused { offset_s: 7200.0 }));
    assert!(
        text.contains("discipline refused: sensor 4 reads 7200 s"),
        "{text}"
    );
}
#[test]
#[allow(clippy::too_many_lines)]
fn the_health_panel_tells_planned_downtime_from_failure() {
    use crate::panels::sensor_health::{
        render_sensor_health, ClockSyncLine, DetectorLine, FeedLine, SensorHealthLine,
        SensorHealthView, SensorPresence,
    };
    use crate::panels::status_strip::EncryptionState;

    let health = gungnir_model::SystemHealth::default();
    let probe = RenderProbe::new();
    // GAP-008: nothing heard from yet, which the panel must say rather than "no skew".
    let clocks = ClockSyncLine {
        sources_observed: 0,
        sources_out_of_sync: 0,
        max_skew_s: 0.0,
        discipline: None,
    };

    // GAP-021: an unconfigured detector is listed as off, and one that is configured but
    // cannot evaluate says why rather than passing for running.
    let detectors = [
        DetectorLine {
            name: "feed",
            state: None,
        },
        DetectorLine {
            name: "cooperative",
            state: Some(Some("no cooperative decoder exists (GAP-010)")),
        },
    ];

    let draw_with = |lines: &[SensorHealthLine<'_>], feeds: &[FeedLine<'_>]| {
        let (_, frame) = probe.draw(|ui| {
            render_sensor_health(
                ui,
                &theme::Palette::day(),
                &SensorHealthView {
                    health: &health,
                    encryption: EncryptionState::Active,
                    sensors: lines,
                    clocks,
                    late_data: crate::panels::sensor_health::LateDataLine::default(),
                    detectors: &detectors,
                    terrain: crate::panels::sensor_health::TerrainLine {
                        masking: false,
                        detail: "no terrain configured",
                    },
                    geoid: NO_GEOID_GRID,
                    point_cloud_registration:
                        crate::panels::sensor_health::PointCloudRegistrationLine::NotConfigured,
                    feeds,
                    cooperative_feeds: &[],
                    peers: &[],
                    bearing_feeds: &[],
                    bearing_pipeline: crate::panels::sensor_health::BearingPipelineLine::default(),
                    exchange: None,
                },
            );
        });
        frame.joined()
    };
    let draw = |lines: &[SensorHealthLine<'_>]| draw_with(lines, &[]);

    // GAP-001: a bound feed that has heard nothing is not health, and one hearing an
    // unreadable stream says how much did not decode.
    let text = draw_with(
        &[],
        &[FeedLine {
            name: "ridge",
            datagrams: 0,
            detections: 0,
            service_reports: 0,
            not_decoded: 0,
            unknown_radar: 0,
            uas_heights_geoid_corrected: 0,
            uas_heights_msl_uncorrected: 0,
            uas_heights_terrain_placed: 0,
            uas_heights_unknown: 0,
        }],
    );
    assert!(
        text.contains("ridge: bound, nothing received yet"),
        "{text}"
    );
    let text = draw_with(
        &[],
        &[FeedLine {
            name: "ridge",
            datagrams: 120,
            detections: 400,
            service_reports: 12,
            not_decoded: 3,
            unknown_radar: 1,
            uas_heights_geoid_corrected: 0,
            uas_heights_msl_uncorrected: 0,
            uas_heights_terrain_placed: 0,
            uas_heights_unknown: 0,
        }],
    );
    assert!(
        text.contains("400 detections") && text.contains("3 not decoded"),
        "{text}"
    );
    // GAP-196: a UAS height the picture holds above mean sea level is a counted loss on
    // the feed's own line, and a corrected one is said too.
    let text = draw_with(
        &[],
        &[FeedLine {
            name: "uas-gateway",
            datagrams: 10,
            detections: 10,
            service_reports: 0,
            not_decoded: 0,
            unknown_radar: 0,
            uas_heights_geoid_corrected: 4,
            uas_heights_msl_uncorrected: 6,
            uas_heights_terrain_placed: 0,
            uas_heights_unknown: 0,
        }],
    );
    assert!(
        text.contains("4 UAS heights corrected to the ellipsoid (EGM2008)")
            && text.contains("6 UAS heights above mean sea level, not corrected"),
        "{text}"
    );
    // GAP-199: a height above ground placed on the DEM is said, and one nobody could
    // place is a counted loss.
    let text = draw_with(
        &[],
        &[FeedLine {
            name: "uas-gateway",
            datagrams: 10,
            detections: 10,
            service_reports: 0,
            not_decoded: 0,
            unknown_radar: 0,
            uas_heights_geoid_corrected: 0,
            uas_heights_msl_uncorrected: 0,
            uas_heights_terrain_placed: 3,
            uas_heights_unknown: 7,
        }],
    );
    assert!(
        text.contains("3 UAS heights above ground placed on the DEM")
            && text.contains("7 UAS heights unknown, held at 0"),
        "{text}"
    );

    // Down inside a window: expected, and the reason is on the panel so nobody has to go
    // and ask which outage this is.
    let text = draw(&[SensorHealthLine {
        id: 1,
        modality: "radar",
        presence: SensorPresence::InMaintenance {
            until: gungnir_model::MissionTime(7200.0),
            reason: "antenna swap",
        },
    }]);
    assert!(text.contains("planned maintenance"), "{text}");
    assert!(text.contains("antenna swap"), "{text}");

    // Down with no window: a fault, and it says so in different words.
    let text = draw(&[SensorHealthLine {
        id: 1,
        modality: "radar",
        presence: SensorPresence::Failed,
    }]);
    assert!(text.contains("off the air"), "{text}");
    assert!(
        !text.contains("planned maintenance"),
        "an unplanned outage was drawn as planned: {text}"
    );

    // The window closed and it never came back. Louder than either.
    let text = draw(&[SensorHealthLine {
        id: 1,
        modality: "radar",
        presence: SensorPresence::Overrun {
            window_closed: gungnir_model::MissionTime(3600.0),
            reason: "antenna swap",
        },
    }]);
    assert!(text.contains("did not return"), "{text}");

    // No sensors configured is a deployment state, not a blank.
    let text = draw(&[]);
    assert!(text.contains("No sensors are configured"), "{text}");
    // And clock sync over no sources says so, rather than claiming agreement.
    assert!(text.contains("no source heard from yet"), "{text}");
    assert!(text.contains("feed: off"), "{text}");
    assert!(
        text.contains("cannot run: no cooperative decoder"),
        "{text}"
    );
}

/// **GAP-096: the line that did not exist.** Before this, `sapient_stats` was written
/// every frame and read by nothing, and the pipeline's five bearing counters had no
/// consumer either -- so a bound spotter/acoustic/passive-RF feed and an unbound one
/// looked identical on this panel: neither drew a line. Proves both halves land on
/// screen from synthetic `SapientFeedStats`-shaped and `PipelineStats`-shaped state: a
/// feed's own counters, and the pipeline's, drawn beside it.
#[test]
fn the_health_panel_draws_the_bearing_feed_line_and_the_pipeline_counters() {
    use crate::panels::sensor_health::{
        render_sensor_health, BearingFeedLine, BearingPipelineLine, ClockSyncLine, SensorHealthView,
    };
    use crate::panels::status_strip::EncryptionState;

    let health = gungnir_model::SystemHealth::default();
    let probe = RenderProbe::new();

    // Nothing bound: no line at all, exactly as a radar or an AIS feed draws none.
    let (_, frame) = probe.draw(|ui| {
        render_sensor_health(
            ui,
            &theme::Palette::day(),
            &SensorHealthView {
                health: &health,
                encryption: EncryptionState::Active,
                sensors: &[],
                clocks: ClockSyncLine {
                    sources_observed: 0,
                    sources_out_of_sync: 0,
                    max_skew_s: 0.0,
                    discipline: None,
                },
                late_data: crate::panels::sensor_health::LateDataLine::default(),
                detectors: &[],
                terrain: crate::panels::sensor_health::TerrainLine {
                    masking: false,
                    detail: "no terrain configured",
                },
                geoid: NO_GEOID_GRID,
                point_cloud_registration:
                    crate::panels::sensor_health::PointCloudRegistrationLine::NotConfigured,
                feeds: &[],
                cooperative_feeds: &[],
                peers: &[],
                bearing_feeds: &[],
                bearing_pipeline: BearingPipelineLine::default(),
                exchange: None,
            },
        );
    });
    assert!(
        !frame.says("Spotter / acoustic / passive-RF"),
        "an unconfigured deployment must not draw a bearing feed section: {}",
        frame.joined()
    );

    // A spotter feed that has reported, and the pipeline's own tally of what happened
    // to the bearings among what every bound feed reported.
    let (_, frame) = probe.draw(|ui| {
        render_sensor_health(
            ui,
            &theme::Palette::day(),
            &SensorHealthView {
                health: &health,
                encryption: EncryptionState::Active,
                sensors: &[],
                clocks: ClockSyncLine {
                    sources_observed: 0,
                    sources_out_of_sync: 0,
                    max_skew_s: 0.0,
                    discipline: None,
                },
                late_data: crate::panels::sensor_health::LateDataLine::default(),
                detectors: &[],
                terrain: crate::panels::sensor_health::TerrainLine {
                    masking: false,
                    detail: "no terrain configured",
                },
                geoid: NO_GEOID_GRID,
                point_cloud_registration:
                    crate::panels::sensor_health::PointCloudRegistrationLine::NotConfigured,
                feeds: &[],
                cooperative_feeds: &[],
                peers: &[],
                bearing_feeds: &[BearingFeedLine {
                    name: "north-ridge-spotter",
                    messages: 40,
                    bearings: 37,
                    ranged: 1,
                    positions: 0,
                    refused: 2,
                }],
                bearing_pipeline: BearingPipelineLine {
                    offered: 37,
                    updated: 5,
                    retained: 30,
                    expired: 12,
                    refused: 2,
                },
                exchange: None,
            },
        );
    });
    let text = frame.joined();
    assert!(
        text.contains("north-ridge-spotter: 40 messages, 37 bearings"),
        "the feed's own counters did not reach the screen: {text}"
    );
    assert!(
        text.contains(
            "Bearing pipeline: 37 offered, 5 updated a track, 30 retained, \
                        12 expired, 2 refused"
        ),
        "the pipeline's five bearing counters did not reach the screen: {text}"
    );
}

/// **GAP-114: the late-data policy and what it did reach PN-09.** The policy in force is
/// named, each outcome it counted is drawn, and a pipeline this console does not run is
/// said to be governed by the node's baseline rather than labelled with this console's.
#[test]
fn the_health_panel_draws_the_late_data_policy_and_its_counters() {
    use crate::panels::sensor_health::{
        late_data_sentence, render_sensor_health, BearingPipelineLine, ClockSyncLine, LateDataLine,
        PointCloudRegistrationLine, SensorHealthView,
    };
    use crate::panels::status_strip::EncryptionState;

    let buffered = LateDataLine {
        policy: Some(gungnir_model::LateDataPolicy::BufferAndReorder {
            max_lateness_s: 2.0,
        }),
        accepted: 120,
        reordered: 14,
        too_late: 3,
        accepted_late: 0,
        not_finite: 0,
    };
    let health = gungnir_model::SystemHealth::default();
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| {
        render_sensor_health(
            ui,
            &theme::Palette::day(),
            &SensorHealthView {
                health: &health,
                encryption: EncryptionState::Active,
                sensors: &[],
                clocks: ClockSyncLine {
                    sources_observed: 0,
                    sources_out_of_sync: 0,
                    max_skew_s: 0.0,
                    discipline: None,
                },
                late_data: buffered,
                detectors: &[],
                terrain: crate::panels::sensor_health::TerrainLine {
                    masking: false,
                    detail: "no terrain configured",
                },
                geoid: NO_GEOID_GRID,
                point_cloud_registration: PointCloudRegistrationLine::NotConfigured,
                feeds: &[],
                cooperative_feeds: &[],
                peers: &[],
                bearing_feeds: &[],
                bearing_pipeline: BearingPipelineLine::default(),
                exchange: None,
            },
        );
    });
    assert!(
        frame.says(
            "Late data: reordered within 2.0 s, dropped beyond; 120 taken, 14 reordered, \
             3 dropped as late"
        ),
        "the policy and its counters did not reach the screen: {}",
        frame.joined()
    );

    let reject = LateDataLine {
        policy: Some(gungnir_model::LateDataPolicy::Reject),
        too_late: 2,
        ..LateDataLine::default()
    };
    assert!(late_data_sentence(&reject).starts_with("Late data: out-of-order data dropped;"));
    // A replay's policy, and a time nobody could read, are said rather than folded away.
    let replay = LateDataLine {
        policy: Some(gungnir_model::LateDataPolicy::AcceptAsIs),
        accepted_late: 4,
        not_finite: 1,
        ..LateDataLine::default()
    };
    let text = late_data_sentence(&replay);
    assert!(
        text.contains("replay and testing only")
            && text.contains("4 applied late as delivered")
            && text.contains("1 with no readable time"),
        "{text}"
    );
    let node = LateDataLine::default();
    assert!(
        late_data_sentence(&node).contains("set by the linked node's baseline"),
        "a node's pipeline was labelled with this console's policy"
    );
}

/// **GAP-024: the line that used to not exist.** `FusionBackend::engine_for` could
/// silently fall back from the GPU path to the CPU reference, and nothing told an
/// operator which one actually ran -- exactly the kind of quiet fallback this
/// workspace's health flags exist to refuse. Proves all four states reach the screen
/// from synthetic `PointCloudRegistrationLine` values alone -- no `FusionBackend`, no
/// `wgpu`, no device, per the workspace rule against ever constructing a real GPU
/// device in a test.
#[test]
fn the_point_cloud_registration_line_names_the_backend_and_why() {
    use crate::panels::sensor_health::{
        render_sensor_health, BearingPipelineLine, ClockSyncLine, PointCloudRegistrationLine,
        SensorHealthView,
    };
    use crate::panels::status_strip::EncryptionState;

    let health = gungnir_model::SystemHealth::default();
    let probe = RenderProbe::new();
    let draw = |line: PointCloudRegistrationLine<'_>| {
        let (_, frame) = probe.draw(|ui| {
            render_sensor_health(
                ui,
                &theme::Palette::day(),
                &SensorHealthView {
                    health: &health,
                    encryption: EncryptionState::Active,
                    sensors: &[],
                    clocks: ClockSyncLine {
                        sources_observed: 0,
                        sources_out_of_sync: 0,
                        max_skew_s: 0.0,
                        discipline: None,
                    },
                    late_data: crate::panels::sensor_health::LateDataLine::default(),
                    detectors: &[],
                    terrain: crate::panels::sensor_health::TerrainLine {
                        masking: false,
                        detail: "no terrain configured",
                    },
                    geoid: NO_GEOID_GRID,
                    point_cloud_registration: line,
                    feeds: &[],
                    cooperative_feeds: &[],
                    peers: &[],
                    bearing_feeds: &[],
                    bearing_pipeline: BearingPipelineLine::default(),
                    exchange: None,
                },
            );
        });
        frame.joined()
    };

    // No pair configured at all: said plainly, never drawn empty or left to imply a
    // backend that never ran (GAP-024's own rule, mirroring GAP-098's for the pair).
    let text = draw(PointCloudRegistrationLine::NotConfigured);
    assert!(
        text.contains("Point-cloud registration: no source/target pair configured"),
        "{text}"
    );

    // The GPU path, unremarkable and said so plainly.
    let text = draw(PointCloudRegistrationLine::Gpu);
    assert!(
        text.contains("Point-cloud registration: GPU path"),
        "{text}"
    );
    assert!(
        !text.contains("CPU fallback"),
        "the GPU path must not also read as a fallback: {text}"
    );

    // The CPU fallback, named as a fallback and with the reason it fell back -- never
    // left to look like the GPU path quietly worked.
    let text = draw(PointCloudRegistrationLine::CpuFallback {
        reason: "no suitable GPU adapter",
    });
    assert!(
        text.contains("Point-cloud registration: CPU fallback (no suitable GPU adapter)"),
        "{text}"
    );

    // The one tick between a pair loading and a backend resolving: still honest, never
    // claiming either backend has actually run yet.
    let text = draw(PointCloudRegistrationLine::Pending);
    assert!(
        text.contains("Point-cloud registration: pair loaded, backend not yet resolved"),
        "{text}"
    );
}

/// **GAP-146: a console the node will not take products from is told so, once, on
/// PN-09.** The line reaches the screen under its own heading, and appears exactly once;
/// with no node linked there is no section at all, rather than one that implies
/// publishing is happening somewhere.
#[test]
fn the_health_panel_says_once_that_this_console_may_not_publish() {
    use crate::panels::sensor_health::{
        render_sensor_health, BearingPipelineLine, ClockSyncLine, ExchangeLine, ExchangeStanding,
        PointCloudRegistrationLine, SensorHealthView,
    };
    use crate::panels::status_strip::EncryptionState;

    let health = gungnir_model::SystemHealth::default();
    let probe = RenderProbe::new();
    let draw = |exchange: Option<ExchangeLine<'_>>| {
        let (_, frame) = probe.draw(|ui| {
            render_sensor_health(
                ui,
                &theme::Palette::day(),
                &SensorHealthView {
                    health: &health,
                    encryption: EncryptionState::Active,
                    sensors: &[],
                    clocks: ClockSyncLine {
                        sources_observed: 0,
                        sources_out_of_sync: 0,
                        max_skew_s: 0.0,
                        discipline: None,
                    },
                    late_data: crate::panels::sensor_health::LateDataLine::default(),
                    detectors: &[],
                    terrain: crate::panels::sensor_health::TerrainLine {
                        masking: false,
                        detail: "no terrain configured",
                    },
                    geoid: NO_GEOID_GRID,
                    point_cloud_registration: PointCloudRegistrationLine::NotConfigured,
                    feeds: &[],
                    cooperative_feeds: &[],
                    peers: &[],
                    bearing_feeds: &[],
                    bearing_pipeline: BearingPipelineLine::default(),
                    exchange,
                },
            );
        });
        frame
    };

    let frame = draw(None);
    assert!(
        !frame.says("Coalition exchange"),
        "an unlinked console drew an exchange section: {}",
        frame.joined()
    );

    let sentence = "Not publishing to coalition exchange: the node refused this console (403)";
    let frame = draw(Some(ExchangeLine {
        standing: ExchangeStanding::Refused,
        text: sentence,
    }));
    assert!(frame.says("Coalition exchange"), "{}", frame.joined());
    assert_eq!(
        frame.joined().matches(sentence).count(),
        1,
        "the refusal is said once: {}",
        frame.joined()
    );
}

/// **An unacknowledged handover has to look unacknowledged.** MOE-13 counts the ones taken
/// by name, and a panel that showed the summary without saying nobody has it would make an
/// incomplete handover indistinguishable from a finished one (DN-21 §5, GAP-054).
#[test]
fn the_handover_says_when_nobody_has_taken_the_watch() {
    use crate::panels::commander_summary::{
        render_commander_summary, CommanderSummaryView, HandoverView, QueueStats,
    };
    use crate::panels::unavailable::Unavailable;

    let vocabulary = vocabulary();
    let handover = HandoverView {
        period: (
            gungnir_model::MissionTime(0.0),
            gungnir_model::MissionTime(3600.0),
        ),
        open_alerts: 2,
        pending_approvals: 1,
        expired_approvals: 0,
        sensors_degraded: 1,
        maintenance_outstanding: 1,
        acknowledged_by: None,
        outstanding_work: true,
        not_assembled: &[Unavailable {
            owner: "gungnir-intercept-service",
            gap: "GAP-043",
        }],
    };
    let view = CommanderSummaryView {
        handover: Some(handover),
        queue: Ok(QueueStats {
            pending: 1,
            decided_this_session: 0,
            expired: 0,
            escalated: 0,
        }),
        delegations: &[],
        accepted_gaps: Err(Unavailable {
            owner: "gungnir-analytics",
            gap: "GAP-006",
        }),
        plan: None,
        outcomes: Err(Unavailable {
            owner: "gungnir-intercept-service",
            gap: "GAP-043",
        }),
        controls_available: false,
        vocabulary: &vocabulary,
        exposure: Err("not evaluated in this test"),
        warnings: crate::panels::commander_summary::WarningCounts::default(),
        queue_authority: crate::panels::approval_queue::QueueAuthority::ThisDesktop,
        decisions_by_role: &[],
        control_status_changes: &[],
    };

    let probe = RenderProbe::new();
    let mut notes = String::new();
    let (_, frame) =
        probe.draw(|ui| render_commander_summary(ui, &theme::Palette::day(), &view, &mut notes));
    let text = frame.joined();
    assert!(text.contains("Watch handover"), "{text}");
    assert!(
        text.contains("this handover is incomplete"),
        "an unacknowledged handover did not say so: {text}"
    );
    assert!(
        text.contains("unfinished work"),
        "the watch was handed over without saying it was unfinished: {text}"
    );
    // A section this build cannot assemble is named, never a blank line that reads as
    // "all clear".
    assert!(text.contains("GAP-043"), "{text}");

    // Once taken, it says who has it and stops saying it is incomplete.
    let taken = HandoverView {
        acknowledged_by: Some(("Supervisor", gungnir_model::MissionTime(3600.0))),
        ..handover
    };
    let view = CommanderSummaryView {
        handover: Some(taken),
        ..view
    };
    let (_, frame) =
        probe.draw(|ui| render_commander_summary(ui, &theme::Palette::day(), &view, &mut notes));
    let text = frame.joined();
    assert!(text.contains("Taken by Supervisor"), "{text}");
    assert!(!text.contains("this handover is incomplete"), "{text}");
}

/// **A connected light beside a picture nobody has heard from is the failure D-23 exists
/// for.** The strip says how long ago the node was last heard, and says it loudly once the
/// silence is longer than a beat should be.
#[test]
fn the_status_strip_says_how_long_since_the_node_was_heard() {
    use crate::panels::status_strip::{
        render_status_strip, AlertSummary, BackendStatus, ClockSource, CoverageStatus,
        EncryptionState, LinkFreshness, StatusStripView,
    };
    let vocabulary = vocabulary();
    let base = |backend| StatusStripView {
        identity: crate::panels::status_strip::IdentityLine::None,
        backend,
        session: None,
        mission_time: gungnir_model::MissionTime(0.0),
        clock_source: ClockSource::Wall,
        clock_skew_s: None,
        health: gungnir_model::SystemHealth::default(),
        control_status: &[],
        delegations: &[],
        alerts: AlertSummary::Unclassified { total: 0 },
        role: "Operator",
        vocabulary: &vocabulary,
        coverage: CoverageStatus::NoApproaches,
        encryption: EncryptionState::Active,
        validity: BaselineValidity::NoWindowConfigured,
        profile: None,
        rehearsal: None,
        operator: OperatorLine::NobodySignedIn,
        queue: crate::panels::approval_queue::QueueAuthority::ThisDesktop,
    };
    let node = |connected, freshness| BackendStatus::Node {
        endpoint: "node.local:7410",
        connected,
        queued: 0,
        dropped: 0,
        freshness,
    };
    let probe = RenderProbe::new();

    let (_, frame) = probe.draw(|ui| {
        render_status_strip(
            ui,
            &theme::Palette::day(),
            &base(node(true, LinkFreshness::Heard { age_s: 0.8 })),
        );
    });
    assert!(frame.says("heard 0.8 s ago"), "{}", frame.joined());

    let (_, frame) = probe.draw(|ui| {
        render_status_strip(
            ui,
            &theme::Palette::day(),
            &base(node(true, LinkFreshness::Overdue { age_s: 5.2 })),
        );
    });
    assert!(frame.says("nothing heard for 5.2 s"), "{}", frame.joined());

    // Detached already says so; a second number would compete with it.
    let (_, frame) = probe.draw(|ui| {
        render_status_strip(
            ui,
            &theme::Palette::day(),
            &base(node(false, LinkFreshness::Overdue { age_s: 40.0 })),
        );
    });
    assert!(frame.says("Detached"), "{}", frame.joined());
    assert!(!frame.says("nothing heard for"), "{}", frame.joined());
}

/// **"No change helps" and "could not evaluate" are different answers** (DN-13 §5 rule 3
/// and its degradation clause, GAP-037), and PN-10 draws them as different sentences.
#[test]
fn the_sensor_panel_tells_no_improvement_from_not_evaluated() {
    use crate::panels::sensor_management::{
        render_sensor_management, RecommendationLine, SensorManagementView,
    };
    use crate::panels::unavailable::Unavailable;
    let vocabulary = vocabulary();
    let base = |recommendations| SensorManagementView {
        sensors: &[],
        recommendations,
        may_task: true,
        role: "SensorManager",
        control_path: Unavailable {
            owner: "gungnir-ingest",
            gap: "GAP-001",
        },
        vocabulary: &vocabulary,
        last_error: None,
    };
    let probe = RenderProbe::new();

    let (_, frame) = probe.draw(|ui| {
        render_sensor_management(ui, &theme::Palette::day(), &base(Ok(&[])));
    });
    assert!(
        frame.says("No mode change improves coverage"),
        "{}",
        frame.joined()
    );

    let (_, frame) = probe.draw(|ui| {
        render_sensor_management(
            ui,
            &theme::Palette::day(),
            &base(Err("no approaches are declared")),
        );
    });
    assert!(
        frame.says("Not evaluated: no approaches"),
        "{}",
        frame.joined()
    );
    assert!(!frame.says("No mode change improves"), "{}", frame.joined());

    let line = RecommendationLine {
        sensor: 2,
        from: "Standby",
        to: gungnir_model::SensorMode::Search,
        uncovered_closed_m: 1800.0,
        redundancy_lost_m: 0.0,
    };
    let (_, frame) = probe.draw(|ui| {
        render_sensor_management(
            ui,
            &theme::Palette::day(),
            &base(Ok(std::slice::from_ref(&line))),
        );
    });
    assert!(frame.says("closes 1800 m"), "{}", frame.joined());
}

/// **PN-17 never adds the two evidence columns together** (DN-06 §5, GAP-043), and
/// says what a track-inferred outcome rests on.
#[test]
fn outcomes_keep_track_inferred_and_corroborated_apart() {
    use crate::panels::commander_summary::{
        render_commander_summary, CommanderSummaryView, OutcomeCounts, QueueStats,
    };
    let vocabulary = vocabulary();
    let view = CommanderSummaryView {
        handover: None,
        queue: Ok(QueueStats {
            pending: 0,
            decided_this_session: 2,
            expired: 0,
            escalated: 0,
        }),
        delegations: &[],
        accepted_gaps: Err(Unavailable {
            owner: "gungnir-analytics",
            gap: "GAP-006",
        }),
        plan: None,
        outcomes: Ok(OutcomeCounts {
            open: 1,
            effective_corroborated: 0,
            effective_track_inferred: 2,
            ineffective_corroborated: 0,
            ineffective_track_inferred: 0,
            indeterminate: 1,
            aborted: 0,
        }),
        controls_available: false,
        vocabulary: &vocabulary,
        exposure: Err("not evaluated in this test"),
        warnings: crate::panels::commander_summary::WarningCounts::default(),
        queue_authority: crate::panels::approval_queue::QueueAuthority::ThisDesktop,
        decisions_by_role: &[],
        control_status_changes: &[],
    };
    let probe = RenderProbe::new();
    let mut notes = String::new();
    let (_, frame) =
        probe.draw(|ui| render_commander_summary(ui, &theme::Palette::day(), &view, &mut notes));
    assert!(
        frame.says("0 corroborated, 2 track-inferred"),
        "{}",
        frame.joined()
    );
    assert!(frame.says("Indeterminate: 1"), "{}", frame.joined());
    assert!(frame.says("dropped by the tracker"), "{}", frame.joined());
    assert!(
        !frame.says("2 effective"),
        "the columns were summed: {}",
        frame.joined()
    );

    let none = CommanderSummaryView {
        outcomes: Ok(OutcomeCounts::default()),
        ..view
    };
    let (_, frame) =
        probe.draw(|ui| render_commander_summary(ui, &theme::Palette::day(), &none, &mut notes));
    assert!(
        frame.says("No engagements this session"),
        "{}",
        frame.joined()
    );
}

/// **A finding without a moment is drawn as an anecdote** (DN-20 §5), a seekable one
/// offers the seek, and a practice finding is never offered promotion (GAP-049).
#[test]
fn the_review_section_tells_a_seekable_finding_from_an_anecdote() {
    use crate::panels::reports::{
        render_reports, ExportState, FindingKindView, FindingLine, ReportsView, ReviewAction,
        ReviewCaseView, ReviewDraft, ReviewStateView, ReviewView,
    };
    let findings = [
        FindingLine {
            id: 1,
            summary: "the queue got behind".into(),
            kind: FindingKindView::SystemBehaviour,
            at_s: Some(120.0),
            promoted_to: None,
        },
        FindingLine {
            id: 2,
            summary: "the watch briefed well".into(),
            kind: FindingKindView::Practice,
            at_s: None,
            promoted_to: None,
        },
    ];
    let view = ReportsView {
        session: Some(3),
        counts: None,
        first_event_s: None,
        last_event_s: None,
        metrics: Err(Unavailable {
            owner: "gungnir-scenario",
            gap: "GAP-045",
        }),
        measures: None,
        export: ExportState::Available {
            marking: "Internal",
            path: "reports/",
            inputs: "3 internal, 0 to parties, 0 to all peers",
        },
        last_export: None,
        nothing_recorded: false,
        review: ReviewView {
            case: Some(ReviewCaseView {
                session: 3,
                state: ReviewStateView::Open,
                findings: &findings,
                open_actions: 0,
                replay_open: true,
            }),
            cannot_open: None,
        },
        order_of_battle: None,
        pattern_of_life: None,
    };
    let probe = RenderProbe::new();
    let mut draft = ReviewDraft::default();
    let (_, frame) = probe.draw(|ui| render_reports(ui, &theme::Palette::day(), &view, &mut draft));
    assert!(frame.says("at 120.0 s: seek"), "{}", frame.joined());
    assert!(
        frame.says("an anecdote, not seekable"),
        "{}",
        frame.joined()
    );
    assert!(frame.says("candidate gap"), "{}", frame.joined());
    assert_eq!(
        frame.joined().matches("Promote").count(),
        1,
        "only the system-behaviour finding may be promoted: {}",
        frame.joined()
    );
    let _ = ReviewAction::Conclude;

    let closed = ReportsView {
        review: ReviewView {
            case: None,
            cannot_open: Some("no session is open to review"),
        },
        order_of_battle: None,
        ..view
    };
    let (_, frame) =
        probe.draw(|ui| render_reports(ui, &theme::Palette::day(), &closed, &mut draft));
    assert!(
        frame.says("no session is open to review"),
        "{}",
        frame.joined()
    );
    assert!(!frame.says("Open a review"), "{}", frame.joined());
}

/// **PN-17 names the threatened asset and how soon** (GAP-026, DN-01), and when nothing
/// is scored it says why rather than listing nothing.
#[test]
fn the_commander_summary_lists_the_most_exposed_assets() {
    use crate::panels::commander_summary::{
        render_commander_summary, CommanderSummaryView, ExposureLine, OutcomeCounts, QueueStats,
    };
    let vocabulary = vocabulary();
    let lines = [ExposureLine {
        track: 42,
        asset: "the harbour",
        priority: "high",
        score: 0.81,
        time_to_impact_s: Some(95.0),
    }];
    let view = CommanderSummaryView {
        handover: None,
        queue: Ok(QueueStats {
            pending: 0,
            decided_this_session: 0,
            expired: 0,
            escalated: 0,
        }),
        delegations: &[],
        accepted_gaps: Err(Unavailable {
            owner: "gungnir-analytics",
            gap: "GAP-006",
        }),
        plan: None,
        outcomes: Ok(OutcomeCounts::default()),
        exposure: Ok(&lines),
        controls_available: false,
        vocabulary: &vocabulary,
        warnings: crate::panels::commander_summary::WarningCounts::default(),
        queue_authority: crate::panels::approval_queue::QueueAuthority::ThisDesktop,
        decisions_by_role: &[],
        control_status_changes: &[],
    };
    let probe = RenderProbe::new();
    let mut notes = String::new();
    let (_, frame) =
        probe.draw(|ui| render_commander_summary(ui, &theme::Palette::day(), &view, &mut notes));
    assert!(
        frame.says("track 42: the harbour (priority high)"),
        "{}",
        frame.joined()
    );
    assert!(frame.says("95 s to impact"), "{}", frame.joined());

    let unscored = CommanderSummaryView {
        exposure: Err("this deployment has declared no defended assets"),
        ..view
    };
    let (_, frame) = probe
        .draw(|ui| render_commander_summary(ui, &theme::Palette::day(), &unscored, &mut notes));
    assert!(
        frame.says("declared no defended assets"),
        "{}",
        frame.joined()
    );
}

/// **PN-05 lists every fires check with its result, and a failure is a word** (DN-05 §7,
/// GAP-036), never colour alone.
#[test]
fn the_intercept_panel_lists_fires_checks_with_failures_as_text() {
    use crate::panels::intercept_panel::{
        render_intercept_panel, Alternatives, FiresCheckLine, ShownPlan, Standing,
    };
    use gungnir_model::{
        FiresPlan, Geodetic, MissionTime, PlanId, PlanKind, PlanView, ResourceId, TrackId,
    };
    let plan = PlanView {
        id: PlanId(1),
        mission_time: MissionTime(0.0),
        kind: PlanKind::Fires(Box::new(FiresPlan {
            target: TrackId(7),
            target_position: Geodetic {
                lat_rad: 0.96,
                lon_rad: 0.21,
                alt_m: 0.0,
            },
            location_error_m: 40.0,
            firing_unit: ResourceId(3),
            time_on_target: None,
            deconfliction: gungnir_model::DeconflictionResult::default(),
        })),
        ..PlanView::default()
    };
    let checks = [
        FiresCheckLine {
            check: "location accuracy",
            passed: true,
            detail: "location error 40 m is within the limit",
        },
        FiresCheckLine {
            check: "no-fire areas",
            passed: false,
            detail: "no-fire areas unavailable, so this check could not be evaluated",
        },
    ];
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| {
        render_intercept_panel(
            ui,
            &theme::Palette::day(),
            ShownPlan {
                held: &[],
                plan: &plan,
                standing: Standing::Current,
            },
            &[],
            &checks,
            &[],
            &Alternatives::default(),
        );
    });
    assert!(
        frame.says("Target track 7, location error 40 m, firing unit 3"),
        "{}",
        frame.joined()
    );
    assert!(frame.says("PASSED location accuracy"), "{}", frame.joined());
    assert!(frame.says("FAILED no-fire areas"), "{}", frame.joined());
}

/// **A stale plan says it is stale, how old it is and why, above the plan** (GAP-119).
///
/// The plan drawn is the same either way -- that is the point: the last good plan is
/// kept, and before this the only thing that told an operator it no longer answered the
/// picture was a health flag on another panel.
#[test]
fn the_intercept_panel_says_a_stale_plan_is_stale_with_its_age() {
    use crate::panels::intercept_panel::{
        render_intercept_panel, Alternatives, ShownPlan, Standing,
    };
    use gungnir_model::{
        InterceptSolutionView, MissionTime, PlanKind, PlanView, ResourceId, TrackId,
    };
    let plan = PlanView {
        mission_time: MissionTime(1.0),
        kind: PlanKind::Intercept {
            solutions: vec![InterceptSolutionView {
                resource: ResourceId(40),
                track: TrackId(70),
                intercept_point: None,
                time_to_intercept_s: None,
            }],
        },
        ..PlanView::default()
    };
    let draw = |standing| {
        RenderProbe::new()
            .draw(|ui| {
                render_intercept_panel(
                    ui,
                    &theme::Palette::day(),
                    ShownPlan {
                        held: &[],
                        plan: &plan,
                        standing,
                    },
                    &[],
                    &[],
                    &[],
                    &Alternatives::default(),
                );
            })
            .1
    };

    let stale = draw(Standing::Stale {
        computed_at_s: 1.0,
        age_s: 2.5,
        reason: "the solve for the current picture (4 track(s), 3 ready resource(s)) did \
                 not finish inside its 4 ms budget; it is 12% done after 1 planning call(s)",
    });
    assert!(stale.says("STALE"), "{}", stale.joined());
    assert!(
        stale.says("computed at t = 1.0 s, 2.5 s before the planner was last asked"),
        "the age is not on the panel: {}",
        stale.joined()
    );
    assert!(
        stale.says("did not finish inside its 4 ms budget"),
        "{}",
        stale.joined()
    );
    // The plan itself is still drawn: stale is a label on it, not a reason to hide it.
    assert!(stale.says("Resource"), "{}", stale.joined());

    let current = draw(Standing::Current);
    assert!(!current.says("STALE"), "{}", current.joined());
    assert!(!current.says("NO PLAN"), "{}", current.joined());

    let none = draw(Standing::NoPlan {
        reason: "no plan has been received from the node",
    });
    assert!(none.says("NO PLAN"), "{}", none.joined());
    assert!(
        none.says("no plan has been received from the node"),
        "{}",
        none.joined()
    );

    // GAP-156: an interim answer says so as plainly, with how good it is known to be.
    let interim = draw(Standing::Interim {
        share: "worth at least 87% of the best plan's value",
        reason: "the exact solver takes at most 16 tracks",
    });
    assert!(interim.says("INTERIM"), "{}", interim.joined());
    assert!(
        interim.says("not the planner's optimum"),
        "{}",
        interim.joined()
    );
    assert!(interim.says("at least 87%"), "{}", interim.joined());
    assert!(interim.says("at most 16 tracks"), "{}", interim.joined());
    assert!(interim.says("Resource"), "the plan itself is still drawn");
}

/// **An interim plan carries its label wherever it is drawn** (GAP-156): on PN-05 under
/// a stale line, where the standing alone says only that it is stale, and on its PN-06
/// row and in PN-07.
#[test]
fn an_interim_plan_is_labelled_on_pn05_pn06_and_pn07() {
    use crate::panels::approval_queue::{
        render_approval_queue, ApprovalQueueView, EmptyBecause, PendingId, QueueOrder, QueueRow,
        TimeRemaining, Verdict,
    };
    use crate::panels::decision_dialog::{
        render_decision_dialog, DecisionDialogState, DecisionDialogView, DecisionRoute,
        OperatorIdentity,
    };
    use crate::panels::intercept_panel::{
        render_intercept_panel, Alternatives, ShownPlan, Standing,
    };
    use gungnir_model::{PlanBasis, PlanId, PlanView};
    let plan = PlanView {
        basis: PlanBasis::OneStep,
        ..PlanView::default()
    };
    let pn05 = RenderProbe::new()
        .draw(|ui| {
            render_intercept_panel(
                ui,
                &theme::Palette::day(),
                ShownPlan {
                    held: &[],
                    plan: &plan,
                    standing: Standing::Stale {
                        computed_at_s: 1.0,
                        age_s: 2.0,
                        reason: "behind",
                    },
                },
                &[],
                &[],
                &[],
                &Alternatives::default(),
            );
        })
        .1;
    assert!(pn05.says("STALE"), "{}", pn05.joined());
    assert!(pn05.says("INTERIM"), "{}", pn05.joined());

    let row = QueueRow {
        escalated_from: None,
        offered_to: &[],
        id: PendingId(1),
        plan_id: PlanId(7),
        assignments: 2,
        verdict: Verdict::RequiresHumanApproval,
        time_remaining: TimeRemaining::NoExpiryConfigured,
        pre_delegated: false,
        may_decide: true,
        basis: PlanBasis::OneStep,
    };
    let rows = [row];
    let queue = ApprovalQueueView {
        rows: &rows,
        order: QueueOrder::TimeThenPriority,
        empty_because: EmptyBecause::NothingPending,
        selected: None,
        may_decide: true,
        role: "Operator",
        handoffs: &[],
        now: gungnir_model::MissionTime(0.0),
        authority: crate::panels::approval_queue::QueueAuthority::ThisDesktop,
        decided: &[],
        cannot_decide: None,
        held_layers: &[],
    };
    let probe = RenderProbe::new();
    let (_, pn06) = probe.draw(|ui| render_approval_queue(ui, &theme::Palette::day(), &queue));
    assert!(pn06.says("INTERIM"), "{}", pn06.joined());

    let unavailable = Unavailable {
        owner: "gungnir-assessment",
        gap: "GAP-028",
    };
    let dialog = DecisionDialogView {
        row: &rows[0],
        rationale: Err(unavailable),
        alternatives: Section::Unavailable(unavailable),
        cost: Err(unavailable),
        degraded: &[],
        engines: &["control status"],
        caveats: &[],
        operator: OperatorIdentity::Unattributed {
            role: "Operator",
            gap: "GAP-057",
        },
        may_accept: true,
        may_override: false,
        route: DecisionRoute::ThisDesktop,
        answer: None,
        rehearsal: None,
    };
    let mut state = DecisionDialogState::default();
    let (_, pn07) =
        probe.draw(|ui| render_decision_dialog(ui, &theme::Palette::day(), &dialog, &mut state));
    assert!(pn07.says("INTERIM"), "{}", pn07.joined());
    assert!(
        pn07.says("could not answer the picture in time"),
        "{}",
        pn07.joined()
    );
}

/// **A refused alternative is drawn with its denial rather than filtered out** (GAP-032).
///
/// The failure this guards against is the quiet one: a panel that showed only the options
/// still open would leave an operator asking on the radio for the option policy had
/// already refused, and the answer would arrive later than this line does.
#[test]
fn the_intercept_panel_draws_a_refused_alternative_with_its_denial() {
    use crate::panels::intercept_panel::{
        render_intercept_panel, AlternativeLine, Alternatives, ShownPlan, Standing,
    };
    use gungnir_model::PlanView;
    let options = [
        AlternativeLine {
            rationale: "Alternative, if resource 1 were unavailable.\nresource 2 -> track 9",
            verdict: "cleared policy; needs a person to decide",
            denied: false,
            assignments: 1,
        },
        AlternativeLine {
            rationale: "Alternative, if resource 2 were unavailable.\nresource 3 -> track 9",
            verdict: "DENIED by policy: ResourceNotReady",
            denied: true,
            assignments: 1,
        },
    ];
    let what_if = AlternativeLine {
        rationale: "What if: 1 hypothetical track(s) in place of the 2 live ones.",
        verdict: "cleared policy; needs a person to decide",
        denied: false,
        assignments: 1,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| {
        render_intercept_panel(
            ui,
            &theme::Palette::day(),
            ShownPlan {
                held: &[],
                plan: &PlanView::default(),
                standing: Standing::Current,
            },
            &[],
            &[],
            &[],
            &Alternatives {
                options: &options,
                what_if: Some(what_if),
            },
        );
    });
    assert!(frame.says("Alternatives"), "{}", frame.joined());
    assert!(
        frame.says("DENIED by policy: ResourceNotReady"),
        "a refused alternative is not shown as refused: {}",
        frame.joined()
    );
    assert!(
        frame.says("if resource 1 were unavailable"),
        "{}",
        frame.joined()
    );
    assert!(frame.says("What if"), "{}", frame.joined());
}

/// **PN-20 tells the three reasons nobody is signed in apart** (DN-23 §5; GAP-057),
/// masks the passphrase, and never lists a hash.
#[test]
fn the_audit_panel_tells_the_three_reasons_apart() {
    use crate::panels::audit::{render_audit, AccountLine, AuditView, SessionLine, SignInDraft};
    let accounts = [AccountLine {
        operator: 7,
        role: "SensorManager",
    }];
    let probe = RenderProbe::new();
    let mut draft = SignInDraft::default();
    for (session, expect) in [
        (SessionLine::NobodySignedIn, "Nobody is signed in"),
        (
            SessionLine::Expired {
                operator: 7,
                at_s: 3600.0,
            },
            "session expired at 3600 s",
        ),
        (
            SessionLine::StoreUnavailable {
                reason: "the file is missing".into(),
            },
            "account store is unavailable: the file is missing",
        ),
        (
            SessionLine::SignedIn {
                operator: 7,
                role: "SensorManager".into(),
                expires_s: None,
            },
            "Signed in: operator 7 (SensorManager), until sign-out",
        ),
    ] {
        let view = AuditView {
            session,
            accounts: Ok(&accounts),
            audit: &[],
            handoffs: &[],
            now: gungnir_model::MissionTime(0.0),
            can_sign_in: true,
            can_assign_roles: true,
            record: crate::panels::audit::AuditRecordView::NOT_VERIFIED,
            node: None,
        };
        let (_, frame) =
            probe.draw(|ui| render_audit(ui, &theme::Palette::day(), &view, &mut draft));
        assert!(frame.says(expect), "{}", frame.joined());
        assert!(
            frame.says("operator 7: SensorManager"),
            "{}",
            frame.joined()
        );
        assert!(!frame.says("$argon2"), "a hash reached the screen");
    }
}

/// **PN-20's handoff rows: what was handed off, to whom, when, and what came back**
/// (GAP-040, DN-07 §7).
///
/// The delivered row is asserted *present* here, which is the opposite of PN-06's rule
/// and deliberately so: the operator's list is about what is still owed, and the
/// after-action account is about what happened.
#[test]
fn the_audit_panel_draws_what_was_handed_off_and_what_came_back() {
    use crate::panels::audit::{render_audit, AuditView, SessionLine, SignInDraft};
    use crate::panels::handoff::HandoffRow;
    use gungnir_model::handoff::{DeliveryState, EffectorReport};
    use gungnir_model::MissionTime;

    let delivered = DeliveryState::Delivered {
        at: MissionTime(110.0),
    };
    let manual = DeliveryState::Manual;
    let reports = [
        EffectorReport::Acknowledged {
            at: MissionTime(112.0),
        },
        EffectorReport::Completed {
            at: MissionTime(160.0),
            effective: false,
            detail: "round expended, target unaffected".into(),
        },
    ];
    let handoffs = [
        HandoffRow {
            decision: gungnir_model::DecisionId(11),
            plan: gungnir_model::PlanId(511),
            endpoint: Some("battery-2"),
            operator: "operator 7",
            role: "Supervisor",
            issued: MissionTime(100.0),
            delivery: &delivered,
            reports: &reports,
        },
        HandoffRow {
            decision: gungnir_model::DecisionId(12),
            plan: gungnir_model::PlanId(512),
            endpoint: None,
            operator: "nobody signed in",
            role: "Operator",
            issued: MissionTime(120.0),
            delivery: &manual,
            reports: &[],
        },
    ];
    let view = AuditView {
        session: SessionLine::NobodySignedIn,
        accounts: Ok(&[]),
        audit: &[],
        handoffs: &handoffs,
        now: MissionTime(200.0),
        can_sign_in: true,
        can_assign_roles: true,
        record: crate::panels::audit::AuditRecordView::NOT_VERIFIED,
        node: None,
    };
    let probe = RenderProbe::new();
    let mut draft = SignInDraft::default();
    let (_, frame) = probe.draw(|ui| render_audit(ui, &theme::Palette::day(), &view, &mut draft));

    // To whom, when, and by whose decision.
    assert!(frame.says("battery-2"), "{}", frame.joined());
    assert!(
        frame.says("operator 7 (Supervisor)"),
        "the attribution the record holds was not drawn: {}",
        frame.joined()
    );
    assert!(
        frame.says("nobody signed in"),
        "an unattributed decision was tidied into a role: {}",
        frame.joined()
    );
    assert!(frame.says("T+100 s"), "{}", frame.joined());
    // What came back, in sequence, including the outcome that matters most.
    assert!(frame.says("Acknowledged at T+112 s"), "{}", frame.joined());
    assert!(
        frame.says("ineffective"),
        "an ineffective engagement was not drawn as one: {}",
        frame.joined()
    );
    // The delivered row is here, unlike on PN-06.
    assert!(
        frame.says("Delivered to battery-2"),
        "the after-action account dropped a delivered handoff: {}",
        frame.joined()
    );
    // And the radio call is what it is, not a failure and not a silent endpoint.
    assert!(frame.says("not a fault"), "{}", frame.joined());
    assert!(frame.says("no return path"), "{}", frame.joined());
}

/// A blank "reported back" column has to say why it is blank: the route is built and no
/// effector fields it, which is D-08 rather than a hole in this desktop.
#[test]
fn the_audit_panel_says_why_nothing_has_reported_back() {
    use crate::panels::audit::{render_audit, AuditView, SessionLine, SignInDraft};
    use crate::panels::handoff::HandoffRow;
    use gungnir_model::handoff::DeliveryState;
    use gungnir_model::MissionTime;

    let queued = DeliveryState::Undelivered {
        since: MissionTime(100.0),
    };
    let handoffs = [HandoffRow {
        decision: gungnir_model::DecisionId(11),
        plan: gungnir_model::PlanId(511),
        endpoint: Some("battery-2"),
        operator: "operator 7",
        role: "Supervisor",
        issued: MissionTime(100.0),
        delivery: &queued,
        reports: &[],
    }];
    let view = AuditView {
        session: SessionLine::NobodySignedIn,
        accounts: Ok(&[]),
        audit: &[],
        handoffs: &handoffs,
        now: MissionTime(200.0),
        can_sign_in: true,
        can_assign_roles: true,
        record: crate::panels::audit::AuditRecordView::NOT_VERIFIED,
        node: None,
    };
    let probe = RenderProbe::new();
    let mut draft = SignInDraft::default();
    let (_, frame) = probe.draw(|ui| render_audit(ui, &theme::Palette::day(), &view, &mut draft));
    assert!(
        frame.says("D-08"),
        "the empty column named no reason: {}",
        frame.joined()
    );
    assert!(
        frame.says("for 100 s"),
        "the waiting handoff drew no age: {}",
        frame.joined()
    );
}

/// **A queue that has quietly stopped filling is the failure this element exists for.**
/// When the baseline in force is outside its validity window every plan is superseded, so
/// PN-06 looks calm and PN-07 looks finished -- exactly the same as a quiet sector. The
/// strip has to say which, and has to distinguish "not yet" from "expired" because they
/// are different situations with different fixes (DN-08 §7, GAP-052).
#[test]
fn the_status_strip_says_when_the_baseline_is_not_in_force() {
    use crate::panels::status_strip::{
        render_status_strip, AlertSummary, BackendStatus, ClockSource, CoverageStatus,
        EncryptionState, StatusStripView,
    };

    let vocabulary = vocabulary();
    let base = || StatusStripView {
        identity: crate::panels::status_strip::IdentityLine::None,
        backend: BackendStatus::Embedded,
        session: None,
        mission_time: gungnir_model::MissionTime(0.0),
        clock_source: ClockSource::Wall,
        clock_skew_s: None,
        health: gungnir_model::SystemHealth::default(),
        control_status: &[],
        delegations: &[],
        alerts: AlertSummary::Unclassified { total: 0 },
        role: "Operator",
        vocabulary: &vocabulary,
        coverage: CoverageStatus::NoApproaches,
        encryption: EncryptionState::Active,
        validity: BaselineValidity::NoWindowConfigured,
        profile: None,
        rehearsal: None,
        operator: OperatorLine::NobodySignedIn,
        queue: crate::panels::approval_queue::QueueAuthority::ThisDesktop,
    };
    let probe = RenderProbe::new();

    // The ordinary case draws nothing: almost no deployment configures a window, and an
    // element that said "baseline valid" on every frame would be read past.
    let (_, frame) = probe.draw(|ui| render_status_strip(ui, &theme::Palette::day(), &base()));
    assert!(!frame.says("baseline"), "{}", frame.joined());

    let expired = StatusStripView {
        validity: BaselineValidity::Expired {
            since: gungnir_model::MissionTime(3600.0),
        },
        ..base()
    };
    let (_, frame) = probe.draw(|ui| render_status_strip(ui, &theme::Palette::day(), &expired));
    assert!(frame.says("expired"), "{}", frame.joined());
    assert!(
        frame.says("superseded"),
        "the strip said the baseline expired without saying what that does to plans: {}",
        frame.joined()
    );

    let not_yet = StatusStripView {
        validity: BaselineValidity::NotYet {
            from: gungnir_model::MissionTime(7200.0),
        },
        ..base()
    };
    let (_, frame) = probe.draw(|ui| render_status_strip(ui, &theme::Palette::day(), &not_yet));
    assert!(frame.says("not in force until"), "{}", frame.joined());
    assert!(
        !frame.says("expired"),
        "a baseline that has not started yet was reported as expired: {}",
        frame.joined()
    );

    // In force with an end says when, because that is the one an operator can plan around.
    let in_force = StatusStripView {
        validity: BaselineValidity::InForce {
            until: Some(gungnir_model::MissionTime(7200.0)),
        },
        ..base()
    };
    let (_, frame) = probe.draw(|ui| render_status_strip(ui, &theme::Palette::day(), &in_force));
    assert!(frame.says("valid until"), "{}", frame.joined());
    assert!(!frame.says("superseded"), "{}", frame.joined());
}

/// **The risk a visibility control on a coverage map carries**: turning a layer off makes
/// the map look like a sector with no gaps in it. PN-11 says so rather than leaving the
/// operator to remember.
#[test]
fn hiding_a_layer_says_the_map_is_not_showing_everything() {
    use crate::panels::coverage_layers::{
        render_coverage_layers, CoverageLayersView, HazardCurrency, LayerCounts, NothingToDraw,
    };

    let view = CoverageLayersView {
        rings_visible: true,
        gaps_visible: false,
        hazards_visible: true,
        counts: LayerCounts {
            rings: 3,
            gaps: 2,
            hazards: 0,
            geofences: 0,
            accepted_gaps: 0,
        },
        hazards: HazardCurrency {
            declared: 0,
            baseline_version: 1,
        },
        coverage: NothingToDraw::Available,
        comparison: LAYDOWN_COMPARISON,
        dead_spots: DEAD_SPOTS,
        geofences_visible: true,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_coverage_layers(ui, &theme::Palette::day(), &view));

    assert!(
        frame.says("A hidden layer is not an empty one"),
        "a hidden layer drew no warning: {}",
        frame.joined()
    );
    // And the count is on the control, so an operator can see what turning it back on
    // would show without turning it back on.
    assert!(frame.says("(2)"), "{}", frame.joined());

    // With everything visible the warning goes away: a strip that always warned would
    // train an operator to stop reading it.
    let view = CoverageLayersView {
        gaps_visible: true,
        ..view
    };
    let (_, frame) = probe.draw(|ui| render_coverage_layers(ui, &theme::Palette::day(), &view));
    assert!(!frame.says("A hidden layer is not"), "{}", frame.joined());
}

/// **A static hazard layer says how old it is** (DN-14 §5, GAP-017): the baseline
/// version, and when some declared hazards could not be placed, how many.
#[test]
fn the_hazard_layer_states_its_currency() {
    use crate::panels::coverage_layers::{
        render_coverage_layers, CoverageLayersView, HazardCurrency, LayerCounts, NothingToDraw,
    };
    let view = CoverageLayersView {
        rings_visible: true,
        gaps_visible: true,
        hazards_visible: true,
        counts: LayerCounts {
            rings: 0,
            gaps: 0,
            hazards: 2,
            geofences: 0,
            accepted_gaps: 0,
        },
        hazards: HazardCurrency {
            declared: 2,
            baseline_version: 7,
        },
        coverage: NothingToDraw::Available,
        comparison: LAYDOWN_COMPARISON,
        dead_spots: DEAD_SPOTS,
        geofences_visible: true,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_coverage_layers(ui, &theme::Palette::day(), &view));
    assert!(frame.says("Hazards and barriers (2)"), "{}", frame.joined());
    assert!(frame.says("baseline revision 7"), "{}", frame.joined());
    assert!(frame.says("static layer"), "{}", frame.joined());

    let unplaced = CoverageLayersView {
        counts: LayerCounts {
            rings: 0,
            gaps: 0,
            hazards: 0,
            geofences: 0,
            accepted_gaps: 0,
        },
        ..view
    };
    let (_, frame) = probe.draw(|ui| render_coverage_layers(ui, &theme::Palette::day(), &unplaced));
    assert!(frame.says("2 declared"), "{}", frame.joined());
    assert!(frame.says("0 placed"), "{}", frame.joined());
}

/// A layer with nothing in it says so, and is not the same as a layer switched off.
#[test]
fn an_empty_layer_and_a_hidden_one_read_differently() {
    use crate::panels::coverage_layers::{
        render_coverage_layers, CoverageLayersView, HazardCurrency, LayerCounts, NothingToDraw,
    };

    let view = CoverageLayersView {
        rings_visible: true,
        gaps_visible: true,
        hazards_visible: true,
        counts: LayerCounts {
            rings: 0,
            gaps: 0,
            hazards: 0,
            geofences: 0,
            accepted_gaps: 0,
        },
        hazards: HazardCurrency {
            declared: 0,
            baseline_version: 1,
        },
        coverage: NothingToDraw::Because {
            reason: "this deployment has declared no local frame origin",
        },
        comparison: LAYDOWN_COMPARISON,
        dead_spots: DEAD_SPOTS,
        geofences_visible: true,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_coverage_layers(ui, &theme::Palette::day(), &view));

    assert!(frame.says("nothing to draw"), "{}", frame.joined());
    assert!(frame.says("no local frame origin"), "{}", frame.joined());
    // Nothing is hidden here, so the hidden-layer warning must not appear: it would
    // send an operator looking for a toggle that is already on.
    assert!(!frame.says("A hidden layer is not"), "{}", frame.joined());
}

/// With no option selected on PN-16, the preview line says so rather than showing a
/// stale or invented comparison (GAP-087).
#[test]
fn the_comparison_status_says_nothing_is_selected_when_nothing_is() {
    use crate::panels::coverage_layers::{
        render_coverage_layers, CoverageLayersView, HazardCurrency, LayerCounts, NothingToDraw,
    };

    let view = CoverageLayersView {
        rings_visible: true,
        gaps_visible: true,
        hazards_visible: true,
        counts: LayerCounts {
            rings: 1,
            gaps: 0,
            hazards: 0,
            geofences: 0,
            accepted_gaps: 0,
        },
        hazards: HazardCurrency {
            declared: 0,
            baseline_version: 1,
        },
        coverage: NothingToDraw::Available,
        comparison: LAYDOWN_COMPARISON,
        dead_spots: DEAD_SPOTS,
        geofences_visible: true,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_coverage_layers(ui, &theme::Palette::day(), &view));
    assert!(frame.says("No option selected"), "{}", frame.joined());
}

/// Selecting an option on PN-16 reaches this status line with its own words for what
/// it is for, and how much it places -- not a bare identifier, and not silently
/// nothing (GAP-087).
#[test]
fn the_comparison_status_names_the_selected_options_intent_and_counts() {
    use crate::panels::coverage_layers::{
        render_coverage_layers, CoverageLayersView, HazardCurrency, LaydownComparison, LayerCounts,
        NothingToDraw,
    };

    let view = CoverageLayersView {
        rings_visible: true,
        gaps_visible: true,
        hazards_visible: true,
        counts: LayerCounts {
            rings: 1,
            gaps: 0,
            hazards: 0,
            geofences: 0,
            accepted_gaps: 0,
        },
        hazards: HazardCurrency {
            declared: 0,
            baseline_version: 1,
        },
        coverage: NothingToDraw::Available,
        comparison: LaydownComparison::Showing {
            intent: "weight the western flank",
            sensors: 3,
            resources: 1,
        },
        dead_spots: DEAD_SPOTS,
        geofences_visible: true,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_coverage_layers(ui, &theme::Palette::day(), &view));
    assert!(frame.says("weight the western flank"), "{}", frame.joined());
    assert!(frame.says("3 sensor"), "{}", frame.joined());
    assert!(frame.says("1 resource"), "{}", frame.joined());
}

/// GAP-246 (DN-34 §7 item 1): with no DEM, PN-11 says the dead spots are unknown, not
/// absent; with a field it says the measure, its age, its cell and that it is geometric
/// line of sight, a coarsened cell and a recompute in flight; and a hidden dead-spot layer
/// raises the hidden-layer sentence like any other.
#[test]
fn the_dead_spot_controls_say_what_the_field_is_or_why_there_is_none() {
    use crate::panels::coverage_layers::{
        render_coverage_layers, CoverageLayersView, DeadSpotStatus, DeadSpotsView, HazardCurrency,
        LayerCounts, NothingToDraw,
    };
    let view = CoverageLayersView {
        rings_visible: true,
        gaps_visible: true,
        hazards_visible: true,
        counts: LayerCounts {
            rings: 2,
            gaps: 0,
            hazards: 0,
            geofences: 0,
            accepted_gaps: 0,
        },
        hazards: HazardCurrency {
            declared: 0,
            baseline_version: 1,
        },
        coverage: NothingToDraw::Available,
        comparison: LAYDOWN_COMPARISON,
        dead_spots: DEAD_SPOTS,
        geofences_visible: true,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_coverage_layers(ui, &theme::Palette::day(), &view));
    assert!(frame.says("not computed"), "{}", frame.joined());
    assert!(frame.says("no DEM is configured"), "{}", frame.joined());
    assert!(frame.says("unknown"), "{}", frame.joined());
    assert!(!frame.says("A hidden layer is not"), "{}", frame.joined());

    let computed = CoverageLayersView {
        dead_spots: DeadSpotsView {
            heights_agl_m: &[30.0, 100.0, 300.0],
            selected_height: 1,
            sensors: &[(1, "S1 north radar"), (2, "S2 RF")],
            status: DeadSpotStatus::Computed {
                measure:
                    "dead spots at 100 m above ground: 12.3 km\u{b2} of 310.0 km\u{b2} claimed",
                age_s: 6.0,
                cell_m: 45.0,
                stale: true,
                clamped_from_m: Some(30.0),
                linked: false,
            },
            ..DEAD_SPOTS
        },
        ..view
    };
    let (_, frame) = probe.draw(|ui| render_coverage_layers(ui, &theme::Palette::day(), &computed));
    for want in [
        "12.3 km",
        "6 s ago",
        "45 m cells",
        "geometric line of sight",
        "30 m asked",
        "Recomputing",
        "300 m",
        "every active sensor",
        "Recompute dead spots",
    ] {
        assert!(frame.says(want), "{want}: {}", frame.joined());
    }

    let hidden = CoverageLayersView {
        dead_spots: DeadSpotsView {
            visible: false,
            ..DEAD_SPOTS
        },
        ..view
    };
    let (_, frame) = probe.draw(|ui| render_coverage_layers(ui, &theme::Palette::day(), &hidden));
    assert!(
        frame.says("A hidden layer is not an empty one"),
        "{}",
        frame.joined()
    );
}

/// PN-21 has to paint all four elements of AGPL section 0, not merely hold them.
///
/// This is the test that matters most in this file, and for an unusual reason: it is
/// the only one whose failure has a consequence outside the program. The additional
/// term in `LICENSE-ADDITIONAL-TERMS.md` section 1 requires a derivative work's
/// interface to preserve this attribution, and AGPL section 5(d) makes that binding on
/// a derivative *only if this interface displays the notices first*. A panel that held
/// the right constants and drew none of them would satisfy every unit test in
/// `about.rs`, look correct in review, and quietly release every future fork of Gungnir
/// from crediting anyone.
///
/// So the assertion is on painted text, element by element, with each failure naming
/// which of section 0's four it lost.
#[test]
fn the_about_panel_paints_every_appropriate_legal_notice() {
    use crate::panels::about::{
        render_about, AboutView, ADDITIONAL_TERMS, APPROPRIATE_LEGAL_NOTICES, COPYRIGHT,
    };

    let view = AboutView {
        version: "0.1.0",
        source_url: "https://example.invalid/gungnir",
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_about(ui, &theme::Palette::day(), &view));

    for notice in APPROPRIATE_LEGAL_NOTICES {
        // Painted text wraps, so the whole sentence is not one drawn string. The first
        // clause is: it is what identifies which notice reached the screen.
        let opening: String = notice
            .split_whitespace()
            .take(4)
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            frame.says(&opening),
            "AGPL section 0 requires this and PN-21 did not draw it: {notice}\n\ndrawn:\n{}",
            frame.joined()
        );
    }

    // The holder has to be named, not merely the word "Copyright": the section 7(b)
    // term is about *whose* attribution survives.
    assert!(
        frame.says("Roessling Digital Solutions LLC"),
        "{}",
        frame.joined()
    );
    assert!(frame.says("WITHOUT ANY"), "{}", frame.joined());
    assert!(
        frame.says("https://www.gnu.org/licenses/"),
        "{}",
        frame.joined()
    );
    // Section 7 asks that added terms be stated in the notice of what they govern.
    let terms_opening: String = ADDITIONAL_TERMS
        .split_whitespace()
        .take(4)
        .collect::<Vec<_>>()
        .join(" ");
    assert!(frame.says(&terms_opening), "{}", frame.joined());
    assert!(COPYRIGHT.contains("2026"));

    // Section 13's offer: an operator who cannot see where source comes from cannot
    // tell whether a modified build has honoured it.
    assert!(
        frame.says("https://example.invalid/gungnir"),
        "{}",
        frame.joined()
    );
}

/// GAP-248 (DN-34 §7 item 5): PN-16's dead-spot cell is the selected option's hidden area
/// within what it claims, at its height; a row not selected says selecting it computes it;
/// one that cannot be computed says why, never a zero; and a stale one says it is
/// recomputing.
#[test]
fn planning_draws_the_selected_options_dead_spots_and_why_the_others_have_none() {
    use crate::panels::planning::{
        render_planning, LaydownCoverage, LaydownRow, PlanningView, RowDeadSpots,
    };
    use gungnir_model::LaydownId;

    let row = |id: &str, current: bool, dead_spots: RowDeadSpots| LaydownRow {
        id: LaydownId(id.into()),
        intent: format!("option {id}"),
        current,
        coverage: LaydownCoverage::Computed {
            gap_segments: 0,
            uncovered_m: 0.0,
            delta_uncovered_m: None,
            accepted_segments: 0,
            accepted_uncovered_m: 0.0,
        },
        dead_spots,
        rehearsal: crate::panels::planning::RowRehearsal::NotRehearsed,
        first_engagement: crate::panels::planning::RowFirstEngagement::NotRehearsed,
    };
    let computed = RowDeadSpots::Computed {
        shadow_km2: 4.2,
        claimed_km2: 17.9,
        height_agl_m: 100.0,
        stale: false,
    };
    let rows = vec![
        row("current", true, RowDeadSpots::NotSelected),
        row("ridge", false, computed),
    ];
    let view = PlanningView {
        laydowns: Section::Present(&rows),
        terrain_model: "terrain-masked line of sight",
        approaches: &[],
        rehearsal: crate::panels::planning::RehearsalSection::NotYetRun,
        rehearsal_scenario: gungnir_model::TestTrackNumber(1),
        selected: Some(&rows[1].id),
        in_force: None,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_planning(ui, &theme::Palette::day(), &view));
    for want in [
        "Dead spots",
        "4.2 of 17.9 km\u{b2} hidden, 100 m above ground",
        "select to compute",
    ] {
        assert!(frame.says(want), "{want}: {}", frame.joined());
    }
    assert!(!frame.says("recomputing"), "{}", frame.joined());

    let rows = vec![
        row(
            "current",
            true,
            RowDeadSpots::NotComputed {
                reason: "no DEM is configured in this baseline (terrain)".into(),
            },
        ),
        row(
            "ridge",
            false,
            RowDeadSpots::Computed {
                stale: true,
                shadow_km2: 4.2,
                claimed_km2: 17.9,
                height_agl_m: 100.0,
            },
        ),
    ];
    let view = PlanningView {
        laydowns: Section::Present(&rows),
        ..view
    };
    let (_, frame) = probe.draw(|ui| render_planning(ui, &theme::Palette::day(), &view));
    assert!(
        frame.says("Not computed: no DEM is configured"),
        "{}",
        frame.joined()
    );
    assert!(frame.says("; recomputing"), "{}", frame.joined());
}

/// PN-16, the planning panel, actually paints the numbers a planner would read a
/// laydown by -- not just that a view struct holds them (GAP-087).
#[test]
fn planning_draws_computed_and_not_computed_rows_and_never_offers_to_adopt() {
    use crate::panels::planning::{render_planning, LaydownCoverage, LaydownRow, PlanningView};
    use gungnir_model::LaydownId;

    let rows = vec![
        LaydownRow {
            id: LaydownId("current".into()),
            intent: "cover the sea approach".into(),
            current: true,
            coverage: LaydownCoverage::Computed {
                gap_segments: 1,
                uncovered_m: 900.0,
                delta_uncovered_m: None,
                accepted_segments: 0,
                accepted_uncovered_m: 0.0,
            },
            dead_spots: crate::panels::planning::RowDeadSpots::NotSelected,
            rehearsal: crate::panels::planning::RowRehearsal::NotRehearsed,
            first_engagement: crate::panels::planning::RowFirstEngagement::NotRehearsed,
        },
        LaydownRow {
            id: LaydownId("west".into()),
            intent: "weight the western flank".into(),
            current: false,
            coverage: LaydownCoverage::Computed {
                gap_segments: 0,
                uncovered_m: 400.0,
                delta_uncovered_m: Some(-500.0),
                accepted_segments: 0,
                accepted_uncovered_m: 0.0,
            },
            dead_spots: crate::panels::planning::RowDeadSpots::NotSelected,
            rehearsal: crate::panels::planning::RowRehearsal::NotRehearsed,
            first_engagement: crate::panels::planning::RowFirstEngagement::NotRehearsed,
        },
        LaydownRow {
            id: LaydownId("untested".into()),
            intent: "a third option nobody has evaluated".into(),
            current: false,
            coverage: LaydownCoverage::NotComputed {
                reason: "no terrain loaded for this sector".into(),
            },
            dead_spots: crate::panels::planning::RowDeadSpots::NotSelected,
            rehearsal: crate::panels::planning::RowRehearsal::NotRehearsed,
            first_engagement: crate::panels::planning::RowFirstEngagement::NotRehearsed,
        },
    ];
    let view = PlanningView {
        laydowns: Section::Present(&rows),
        terrain_model: "flat-terrain line of sight",
        approaches: &[],
        rehearsal: crate::panels::planning::RehearsalSection::NotYetRun,
        rehearsal_scenario: gungnir_model::TestTrackNumber(1),
        selected: Some(&rows[1].id),
        in_force: None,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_planning(ui, &theme::Palette::day(), &view));

    // The current laydown is named as such, and the terrain model every row was
    // compared under is stated once rather than left for the reader to assume. A bare
    // "current" would be found in the column header ("Difference from current") and in
    // this fixture's own laydown id whether or not the marker was painted, so the check
    // is for the option label planning.rs builds for the current row: its id followed by
    // " (current)".
    assert!(frame.says("current (current)"), "{}", frame.joined());
    assert!(
        frame.says("flat-terrain line of sight"),
        "{}",
        frame.joined()
    );

    // Every number the computed rows carry reaches the screen, not just the view struct
    // (the PN-16 laydown options table row of `docs/verification-capability-table.md`
    // §2): both rows' gap segments and uncovered lengths, in the coverage label
    // planning.rs formats, and the alternative's difference.
    assert!(
        frame.says("1 gap segment(s), 900 m uncovered"),
        "{}",
        frame.joined()
    );
    assert!(
        frame.says("0 gap segment(s), 400 m uncovered"),
        "{}",
        frame.joined()
    );
    assert!(
        frame.says("500 m less gap than today"),
        "{}",
        frame.joined()
    );

    // The current row's difference cell says it is the current laydown rather than
    // showing a zero, which would read as an alternative that changes nothing. Nothing
    // else on this panel paints this marker.
    assert!(frame.says("-- (current)"), "{}", frame.joined());

    // A row that could not be evaluated says why, and is not drawn as a zero.
    assert!(frame.says("Not computed"), "{}", frame.joined());
    assert!(frame.says("no terrain loaded"), "{}", frame.joined());

    // DN-26 section 6 rule 4: no adoption path exists, and the panel says so rather
    // than leaving an operator to infer it from the absence of a button.
    assert!(
        frame.says("moving a sensor is a physical act"),
        "{}",
        frame.joined()
    );

    // The rehearsal section says a laydown is selected and not yet rehearsed, not
    // silently missing (GAP-045).
    assert!(frame.says("Not yet rehearsed"), "{}", frame.joined());
}

/// PN-16's label on a rehearsal (DN-32 section 6): every result says it was re-observed
/// from a recording, names the recording and each sensor's detection model, and the
/// table reads the run, naming the sensor a difference from the current laydown came
/// from (GAP-105).
// One scene, both halves of PN-16 read against each other; it crossed the pedantic line
// limit when GAP-020 gave the rows and the summary their first-engagement fields.
#[test]
#[allow(clippy::too_many_lines)]
fn a_rehearsal_is_labelled_re_observed_and_the_table_reads_the_run() {
    use crate::panels::planning::{
        render_planning, LaydownCoverage, LaydownRow, PlanningView, RehearsalFirstEngagement,
        RehearsalSection, RehearsalSummary, RehearsedSensor, RowFirstEngagement, RowRehearsal,
        VersusCurrent,
    };
    use gungnir_model::{LaydownId, TestTrackNumber};

    let coverage = LaydownCoverage::Computed {
        gap_segments: 2,
        uncovered_m: 7000.0,
        delta_uncovered_m: None,
        accepted_segments: 0,
        accepted_uncovered_m: 0.0,
    };
    let rows = vec![
        LaydownRow {
            id: LaydownId("current".into()),
            intent: "the deployment as sited".into(),
            current: true,
            coverage: coverage.clone(),
            dead_spots: crate::panels::planning::RowDeadSpots::NotSelected,
            rehearsal: RowRehearsal::Rehearsed {
                scenario: TestTrackNumber(1),
                detections: 120,
                tracks_formed: 5,
                versus_current: VersusCurrent::IsCurrent,
                ran_at: gungnir_model::MissionTime(0.0),
            },
            first_engagement: RowFirstEngagement::NotComputed {
                reason: "no approach is declared".into(),
            },
        },
        LaydownRow {
            id: LaydownId("c".into()),
            intent: "move S2 forward".into(),
            current: false,
            coverage,
            dead_spots: crate::panels::planning::RowDeadSpots::NotSelected,
            rehearsal: RowRehearsal::Rehearsed {
                scenario: TestTrackNumber(1),
                detections: 168,
                tracks_formed: 6,
                versus_current: VersusCurrent::Difference {
                    detections: 48,
                    sensors: vec![2],
                },
                ran_at: gungnir_model::MissionTime(0.0),
            },
            first_engagement: RowFirstEngagement::NotComputed {
                reason: "no approach is declared".into(),
            },
        },
    ];
    let view = PlanningView {
        laydowns: Section::Present(&rows),
        terrain_model: "flat-terrain line of sight",
        approaches: &[],
        rehearsal: RehearsalSection::Ran(RehearsalSummary {
            scenario: TestTrackNumber(1),
            seed: 1701,
            tracks_formed: 6,
            decisions_raised: 3,
            decisions_expired: 1,
            sensors: vec![
                RehearsedSensor {
                    sensor: 1,
                    detection_model: Some("radar.short".into()),
                    detections: 100,
                    false_alarms: 44,
                    delta_from_current: Some(0),
                },
                RehearsedSensor {
                    sensor: 2,
                    detection_model: Some("radar.short".into()),
                    detections: 68,
                    false_alarms: 40,
                    delta_from_current: Some(48),
                },
            ],
            recording_events_not_applied: 2,
            ran_at: gungnir_model::MissionTime(0.0),
            first_engagement: RehearsalFirstEngagement::NotComputed {
                reason: "no approach is declared".into(),
            },
        }),
        rehearsal_scenario: TestTrackNumber(1),
        selected: Some(&rows[1].id),
        in_force: None,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_planning(ui, &theme::Palette::day(), &view));

    assert!(
        frame.says("Re-observed from a recording: TT-01"),
        "{}",
        frame.joined()
    );
    assert!(frame.says("not sensor data"), "{}", frame.joined());
    assert!(frame.says("radar.short"), "{}", frame.joined());
    assert!(
        frame.says("TT-01: 168 detection(s), 6 track(s)"),
        "{}",
        frame.joined()
    );
    assert!(
        frame.says("48 more detection(s), from S2"),
        "{}",
        frame.joined()
    );
    assert!(frame.says("+48"), "{}", frame.joined());
    assert!(
        frame.says("name its own sensors and were not applied"),
        "{}",
        frame.joined()
    );
}

/// PN-16's first-engagement column paints what D-45 requires (GAP-020, DN-02 section 9):
/// the figure labelled as the worst case with its count, a difference from the current
/// laydown in words, an approach with no engagement said to be not computable rather
/// than drawn as a zero, and the rehearsal it came from -- recording, seed, when run --
/// with the track and time behind the worst case.
// One scene -- three rows, two approaches, the section -- so every state D-45 and DN-02
// section 9 name is drawn beside the others it must be told apart from.
#[test]
#[allow(clippy::too_many_lines)]
fn first_engagement_is_drawn_as_a_worst_case_with_its_count_and_never_as_zero() {
    use crate::panels::planning::{
        render_planning, ApproachEngagement, ApproachLine, LaydownCoverage, LaydownRow,
        PlanningView, RehearsalFirstEngagement, RehearsalSection, RehearsalSummary,
        RowFirstEngagement, RowRehearsal, VersusCurrent,
    };
    use gungnir_model::{LaydownId, MissionTime, ResourceId, TestTrackNumber, TrackId};

    let not_computable = ApproachEngagement::NotComputable {
        reason: "no recorded target on this approach was engaged in the run".into(),
    };
    let worst = |range_m: f64, predictions: usize, versus_current_m: Option<f64>| {
        ApproachEngagement::WorstCase {
            range_m,
            predictions,
            target: "TT01-stream-south-004".into(),
            track: TrackId(81),
            resource: ResourceId(2),
            proposed_at: MissionTime(376.0),
            versus_current_m,
        }
    };
    let rehearsed = |versus_current| RowRehearsal::Rehearsed {
        scenario: TestTrackNumber(1),
        detections: 408,
        tracks_formed: 30,
        versus_current,
        ran_at: MissionTime(125.0),
    };
    let coverage = LaydownCoverage::Computed {
        gap_segments: 1,
        uncovered_m: 1000.0,
        delta_uncovered_m: None,
        accepted_segments: 0,
        accepted_uncovered_m: 0.0,
    };
    let rows = vec![
        LaydownRow {
            id: LaydownId("current".into()),
            intent: "the deployment as sited".into(),
            current: true,
            coverage: coverage.clone(),
            dead_spots: crate::panels::planning::RowDeadSpots::NotSelected,
            rehearsal: rehearsed(VersusCurrent::IsCurrent),
            first_engagement: RowFirstEngagement::PerApproach(vec![
                worst(9_000.0, 3, None),
                not_computable.clone(),
            ]),
        },
        LaydownRow {
            id: LaydownId("b".into()),
            intent: "sea-weighted".into(),
            current: false,
            coverage: coverage.clone(),
            dead_spots: crate::panels::planning::RowDeadSpots::NotSelected,
            rehearsal: rehearsed(VersusCurrent::Difference {
                detections: 0,
                sensors: Vec::new(),
            }),
            first_engagement: RowFirstEngagement::PerApproach(vec![
                worst(18_000.0, 4, Some(9_000.0)),
                not_computable.clone(),
            ]),
        },
        LaydownRow {
            id: LaydownId("untried".into()),
            intent: "never rehearsed".into(),
            current: false,
            coverage,
            dead_spots: crate::panels::planning::RowDeadSpots::NotSelected,
            rehearsal: RowRehearsal::NotRehearsed,
            first_engagement: RowFirstEngagement::NotRehearsed,
        },
    ];
    let approaches = ["sea approach".to_string(), "upper Vell".to_string()];
    let view = PlanningView {
        laydowns: Section::Present(&rows),
        terrain_model: "flat-terrain line of sight",
        approaches: &approaches,
        rehearsal: RehearsalSection::Ran(RehearsalSummary {
            scenario: TestTrackNumber(1),
            seed: 1701,
            tracks_formed: 30,
            decisions_raised: 12,
            decisions_expired: 0,
            sensors: Vec::new(),
            recording_events_not_applied: 0,
            ran_at: MissionTime(125.0),
            first_engagement: RehearsalFirstEngagement::PerApproach {
                lines: vec![
                    ApproachLine {
                        approach: "sea approach".into(),
                        corridor_half_width_m: Some(5_000.0),
                        engagement: worst(18_000.0, 4, Some(9_000.0)),
                    },
                    ApproachLine {
                        approach: "upper Vell".into(),
                        corridor_half_width_m: None,
                        engagement: ApproachEngagement::NotComputable {
                            reason: "the approach declares no corridor width".into(),
                        },
                    },
                ],
                on_no_corridor: 2,
                clutter_pairings: 5,
                plans_proposed: 40,
                plans_not_offered: 3,
                not_offered_because: vec![("NoGoGeofence".into(), 3)],
            },
        }),
        rehearsal_scenario: TestTrackNumber(1),
        selected: Some(&rows[1].id),
        in_force: None,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_planning(ui, &theme::Palette::day(), &view));

    // One column per approach, each saying what it holds.
    assert!(
        frame.says("sea approach: first engagement (worst case)"),
        "{}",
        frame.joined()
    );
    assert!(
        frame.says("upper Vell: first engagement (worst case)"),
        "{}",
        frame.joined()
    );
    // D-45: labelled as the worst case, with its count, and compared in words.
    assert!(frame.says("worst 9.0 km (n = 3)"), "{}", frame.joined());
    assert!(
        frame.says("worst 18.0 km (n = 4); 9.0 km farther out than current"),
        "{}",
        frame.joined()
    );
    // No engagement is a reason, never a zero.
    assert!(
        frame.says("not computable: no recorded target on this approach was engaged in the run"),
        "{}",
        frame.joined()
    );
    assert!(!frame.says("0.0 km"), "{}", frame.joined());
    assert!(frame.says("not rehearsed"), "{}", frame.joined());
    // Provenance: the recording, its seed, and when the rehearsal was run.
    assert!(frame.says("run at T+125 s"), "{}", frame.joined());
    assert!(
        frame.says("Re-observed from a recording: TT-01 (seed 1701)"),
        "{}",
        frame.joined()
    );
    // The section names the corridor, the track and effector, and when in the run.
    assert!(
        frame.says("corridor 5.0 km either side"),
        "{}",
        frame.joined()
    );
    assert!(
        frame.says(
            "target TT01-stream-south-004 (track 81) against resource 2 proposed at T+376 s \
             of the recording"
        ),
        "{}",
        frame.joined()
    );
    assert!(
        frame.says("no corridor declared): not computable"),
        "{}",
        frame.joined()
    );
    assert!(
        frame.says("2 paired target(s) were in no declared corridor"),
        "{}",
        frame.joined()
    );
    assert!(
        frame.says("5 paired track(s) were none of the recording's targets (clutter)"),
        "{}",
        frame.joined()
    );
    // GAP-182: plans the deployment's own policy did not offer are said, with why.
    assert!(
        frame.says(
            "3 of the 40 plan(s) the run proposed were not offered for decision under this \
             deployment's policy (3 denied: NoGoGeofence)"
        ),
        "{}",
        frame.joined()
    );
    assert!(frame.says("the least ground range"), "{}", frame.joined());
    assert!(frame.says("own policy offered"), "{}", frame.joined());
}

/// An empty laydown table says why rather than drawing nothing (DN-26 section 8).
#[test]
fn planning_with_no_laydowns_declared_says_so() {
    use crate::panels::planning::{render_planning, PlanningView};

    let view = PlanningView {
        laydowns: Section::Empty {
            reason: "This deployment has declared no laydown alternatives.",
        },
        terrain_model: "flat-terrain line of sight",
        approaches: &[],
        rehearsal: crate::panels::planning::RehearsalSection::NothingSelected,
        rehearsal_scenario: gungnir_model::TestTrackNumber(1),
        selected: None,
        in_force: None,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_planning(ui, &theme::Palette::day(), &view));
    assert!(
        frame.says("declared no laydown alternatives"),
        "{}",
        frame.joined()
    );
}

/// **PN-20 says the audit record is damaged, naming the file, and never mutes it**
/// (GAP-163, D-104), and lists every segment with its state and an earlier one read back
/// (GAP-152, D-106). Since GAP-179 (D-116) it draws the node's record beside this
/// desktop's, on a desktop linked to one.
#[test]
// One PN-20 drawn whole: both records have to be on the same frame to show they sit side
// by side, and splitting the view's construction would split the frame.
#[allow(clippy::too_many_lines)]
fn the_audit_panel_names_a_cut_segment_and_shows_an_earlier_run() {
    use crate::panels::audit::{
        render_audit, AuditRecordView, AuditView, RecordSegmentLine, SessionLine, ShownSegment,
        SignInDraft,
    };
    use crate::panels::config_editor::AuditLine;

    let problems = [
        "audit-000002.jsonl was cut: the journal holds its head at entry 9 and \
                     6 remain, so 3 entries are missing"
            .to_owned(),
    ];
    let segments = [
        RecordSegmentLine {
            description: "audit-000001.jsonl: 4 entries, verified to the head the journal holds",
            sound: true,
            readable: true,
        },
        RecordSegmentLine {
            description: "audit-000002.jsonl: 6 entries, CUT: 3 entries missing",
            sound: false,
            readable: true,
        },
    ];
    let lines = [AuditLine {
        action: "session.sign_in",
        mission_time_s: 12,
        detail: "signed in as Supervisor",
        operator: Some(7),
    }];
    let node_problems = vec![
        "audit-000003.jsonl was cut: the journal holds its head at entry 11 and 9 remain, \
         so 2 entries are missing"
            .to_owned(),
    ];
    let node_segments = [
        RecordSegmentLine {
            description: "audit-000003.jsonl: 9 entries, CUT: 2 entries missing",
            sound: false,
            readable: true,
        },
        RecordSegmentLine {
            description: "audit-000004.jsonl: 1 entries, this run's, anchored as it is written",
            sound: true,
            readable: true,
        },
    ];
    let node_lines = [AuditLine {
        action: "audit.read",
        mission_time_s: 9,
        detail: "read the audit record's verification and segment list as Administrator",
        operator: Some(25),
    }];
    let view = AuditView {
        session: SessionLine::NobodySignedIn,
        accounts: Ok(&[]),
        audit: &[],
        handoffs: &[],
        now: gungnir_model::MissionTime(0.0),
        can_sign_in: true,
        can_assign_roles: true,
        record: AuditRecordView {
            summary: "The audit record is DAMAGED (verified at 3 s): 1 problem.",
            sound: false,
            problems: &problems,
            segments: &segments,
            shown: Some(ShownSegment {
                segment: "audit-000001.jsonl",
                lines: &lines,
                note: "Its chain verified as it was read.",
                sound: true,
            }),
            retention: "Audit segments are never purged: the baseline declares no retention.",
            // GAP-208: the key is configured and locked, so the unlock field is drawn.
            signing: "Audit anchors are UNSIGNED: no administrator has unlocked the \
                      audit-signing key this run.",
            signing_locked: true,
        },
        // GAP-179, D-116: the node's record beside this desktop's own.
        node: Some(crate::panels::audit::NodeAuditRecordView {
            node: "https://node.example:8443",
            summary: "The node's audit record is DAMAGED (verified at start at node T+4 s, \
                      read at T+9 s): 1 problem(s) across 2 segment(s).",
            sound: false,
            note: "",
            can_read: true,
            reading: false,
            problems: &node_problems,
            segments: &node_segments,
            shown: Some(crate::panels::audit::NodeShownPage {
                segment: "audit-000004.jsonl",
                lines: &node_lines,
                note: "Entries 1 to 1 of 1. Its chain verified as it was read.",
                sound: true,
                has_older: false,
                has_newer: false,
            }),
        }),
    };
    let probe = RenderProbe::new();
    let mut draft = SignInDraft::default();
    let (action, frame) =
        probe.draw(|ui| render_audit(ui, &theme::Palette::day(), &view, &mut draft));
    assert_eq!(action, Some(None), "drawing is not an act");
    for said in [
        "DAMAGED",
        "3 entries are missing",
        "audit-000002.jsonl: 6 entries, CUT: 3 entries missing",
        "Verify now",
        "audit-000001.jsonl (read only)",
        "signed in as Supervisor",
        "never purged",
        "The node's audit record (https://node.example:8443)",
        "audit-000003.jsonl: 9 entries, CUT: 2 entries missing",
        "Read the node's record",
        "Verify the node's record now",
        "Each read is recorded on the node's own audit record.",
        "audit-000004.jsonl on the node (read only)",
        "read the audit record's verification and segment list",
        // GAP-208.
        "Audit anchors are UNSIGNED",
        "Administrator's audit-signing passphrase:",
        "Unlock audit signing",
    ] {
        assert!(
            frame.says(said),
            "PN-20 did not draw {said:?}: {}",
            frame.joined()
        );
    }
}

/// A strip for the GAP-203 tests: everything but the control status lines at its
/// quietest.
fn strip_with<'a>(
    vocabulary: &'a Vocabulary,
    lines: &'a [crate::panels::status_strip::ControlStatusLine<'a>],
) -> crate::panels::status_strip::StatusStripView<'a> {
    use crate::panels::status_strip::{
        AlertSummary, BackendStatus, ClockSource, CoverageStatus, StatusStripView,
    };
    StatusStripView {
        identity: crate::panels::status_strip::IdentityLine::None,
        backend: BackendStatus::Embedded,
        session: None,
        mission_time: gungnir_model::MissionTime(0.0),
        clock_source: ClockSource::Wall,
        clock_skew_s: None,
        health: gungnir_model::SystemHealth::default(),
        control_status: lines,
        delegations: &[],
        alerts: AlertSummary::Unclassified { total: 0 },
        role: "Supervisor",
        vocabulary,
        coverage: CoverageStatus::NoApproaches,
        encryption: EncryptionState::Active,
        validity: BaselineValidity::NoWindowConfigured,
        profile: None,
        rehearsal: None,
        operator: OperatorLine::SignedIn {
            operator: 7,
            role: "Supervisor",
            expires_in_s: None,
        },
        queue: crate::panels::approval_queue::QueueAuthority::ThisDesktop,
    }
}

/// GAP-203, FL-08's first step: a chip the console may change is a control, and clicking
/// it is the strip's one action, naming the layer. A chip it may not change is drawn
/// with the same words and clicking it does nothing; the strip itself never changes a
/// status either way.
#[test]
fn a_changeable_control_status_chip_returns_its_layer_when_clicked() {
    use crate::panels::status_strip::{
        render_status_strip, ControlStatusLine, SetBy, StatusStripAction,
    };
    use gungnir_model::{EffectorLayer, MissionTime, WeaponsControlStatus};

    let vocabulary = vocabulary();
    let line = |can_change| {
        [ControlStatusLine {
            layer: EffectorLayer::Area,
            status: WeaponsControlStatus::Tight,
            configured: true,
            set_by: Some(SetBy {
                operator: "7",
                role: "Supervisor",
                at: MissionTime(120.0),
                reason: "civil traffic",
            }),
            can_change,
        }]
    };
    let chip = format!(
        "{} {}",
        vocabulary.layer(EffectorLayer::Area),
        vocabulary.control_status(WeaponsControlStatus::Tight)
    );
    let probe = RenderProbe::new();

    let changeable = line(true);
    let view = strip_with(&vocabulary, &changeable);
    let (drawn, frame) = probe.draw(|ui| render_status_strip(ui, &theme::Palette::day(), &view));
    assert_eq!(drawn, Some(None), "drawing is not a click");
    assert!(
        frame.says(&chip),
        "the chip was not drawn: {}",
        frame.joined()
    );
    let (clicked, frame) = probe
        .click(&chip, |ui| {
            render_status_strip(ui, &theme::Palette::day(), &view)
        })
        .unwrap_or_else(|frame| panic!("no chip to click: {}", frame.joined()));
    assert_eq!(
        clicked,
        Some(Some(StatusStripAction::ChangeControlStatus(
            EffectorLayer::Area
        ))),
        "clicking the chip did not name its layer: {}",
        frame.joined()
    );

    let fixed = line(false);
    let view = strip_with(&vocabulary, &fixed);
    let (clicked, frame) = probe
        .click(&chip, |ui| {
            render_status_strip(ui, &theme::Palette::day(), &view)
        })
        .unwrap_or_else(|frame| panic!("no chip drawn: {}", frame.joined()));
    assert_eq!(
        clicked,
        Some(None),
        "a chip the console may not change acted on a click: {}",
        frame.joined()
    );
    // And the words are the same either way: what a person reads does not depend on
    // what they may do. Who set it is on the hover, which the probe cannot raise, so
    // the sentence itself is checked in `status_strip::tests`.
    assert!(frame.says(&chip), "{}", frame.joined());
}

/// DN-09 §7's PN-17 row (GAP-203): the status changes in the period, with who made them,
/// newest first, the second person and the console said where the record has them; and
/// the empty state said rather than drawn as nothing.
#[test]
fn the_commander_summary_lists_control_status_changes_newest_first() {
    use crate::panels::commander_summary::{
        render_commander_summary, CommanderSummaryView, ControlStatusChangeLine, OutcomeCounts,
        QueueStats,
    };
    use gungnir_model::{EffectorLayer, MissionTime, WeaponsControlStatus};

    let vocabulary = vocabulary();
    let changes = [
        ControlStatusChangeLine {
            layer: EffectorLayer::Area,
            status: WeaponsControlStatus::Hold,
            previous: WeaponsControlStatus::Tight,
            operator: "7",
            role: "Supervisor",
            at: MissionTime(120.0),
            reason: "civil traffic in the sector",
            confirmed_by: None,
            origin: None,
        },
        ControlStatusChangeLine {
            layer: EffectorLayer::Point,
            status: WeaponsControlStatus::Free,
            previous: WeaponsControlStatus::Tight,
            operator: "7",
            role: "Supervisor",
            at: MissionTime(300.0),
            reason: "raid inbound",
            confirmed_by: Some("operator 9 (Commander)"),
            origin: Some("desktop-1"),
        },
    ];
    let view = CommanderSummaryView {
        handover: None,
        queue: Ok(QueueStats {
            pending: 0,
            decided_this_session: 0,
            expired: 0,
            escalated: 0,
        }),
        delegations: &[],
        accepted_gaps: Err(Unavailable {
            owner: "gungnir-analytics",
            gap: "GAP-006",
        }),
        plan: None,
        outcomes: Ok(OutcomeCounts::default()),
        exposure: Err("not evaluated in this test"),
        controls_available: false,
        vocabulary: &vocabulary,
        warnings: crate::panels::commander_summary::WarningCounts::default(),
        queue_authority: crate::panels::approval_queue::QueueAuthority::ThisDesktop,
        decisions_by_role: &[],
        control_status_changes: &changes,
    };
    let probe = RenderProbe::new();
    let mut notes = String::new();
    let (_, frame) =
        probe.draw(|ui| render_commander_summary(ui, &theme::Palette::day(), &view, &mut notes));
    assert!(
        frame.says("Control status changes this session"),
        "{}",
        frame.joined()
    );
    let first = frame
        .position_of(
            "Area defence: Weapons tight to Weapons hold by operator 7 (Supervisor) at \
             T+120 s: civil traffic in the sector",
        )
        .unwrap_or_else(|| panic!("the earlier change was not drawn: {}", frame.joined()));
    let second = frame
        .position_of(
            "Point defence: Weapons tight to Weapons free by operator 7 (Supervisor) at \
             T+300 s: raid inbound, confirmed by operator 9 (Commander), from desktop-1",
        )
        .unwrap_or_else(|| panic!("the later change was not drawn: {}", frame.joined()));
    assert!(
        second < first,
        "the changes were not drawn newest first: {}",
        frame.joined()
    );
    assert!(
        !frame.says("No control status change this session"),
        "{}",
        frame.joined()
    );

    let none = CommanderSummaryView {
        control_status_changes: &[],
        ..view
    };
    let (_, frame) =
        probe.draw(|ui| render_commander_summary(ui, &theme::Palette::day(), &none, &mut notes));
    assert!(
        frame.says("No control status change this session; the baseline's statuses are in force."),
        "an empty list drew nothing: {}",
        frame.joined()
    );
}

/// A status dialog for the GAP-203 tests, at its plainest.
fn dialog_view(
    vocabulary: &Vocabulary,
    current: gungnir_model::WeaponsControlStatus,
) -> crate::panels::control_status_dialog::ControlStatusDialogView<'_> {
    use crate::panels::control_status_dialog::{ControlStatusDialogView, Route};
    ControlStatusDialogView {
        layer: gungnir_model::EffectorLayer::Area,
        current,
        configured: true,
        last_change: None,
        would_withdraw: &[],
        queued_on_layer: 0,
        two_person: false,
        may_set: Ok(()),
        route: Route::ThisDesktop,
        vocabulary,
    }
}

/// FL-08's dialog (GAP-203): cancel is drawn before set, a drawn dialog returns nothing,
/// the status in force is shown and not offered, what each change would withdraw is
/// beside it, the route is said, and set stays shut for every incomplete state while
/// the dialog says what is missing.
#[test]
fn the_control_status_dialog_draws_set_last_and_shut_until_the_rule_is_met() {
    use crate::panels::control_status_dialog::{
        render_control_status_dialog, ControlStatusDialogState, Route,
    };
    use gungnir_model::{EffectorLayer, WeaponsControlStatus};

    let vocabulary = vocabulary();
    let withdraw = [
        (WeaponsControlStatus::Hold, 3),
        (WeaponsControlStatus::Free, 0),
    ];
    let view = crate::panels::control_status_dialog::ControlStatusDialogView {
        would_withdraw: &withdraw,
        queued_on_layer: 3,
        route: Route::Node {
            endpoint: "node.local:7410",
        },
        ..dialog_view(&vocabulary, WeaponsControlStatus::Tight)
    };
    let probe = RenderProbe::new();
    let day = theme::Palette::day();

    // Nothing chosen: shut, and the dialog says so.
    let mut state = ControlStatusDialogState::open(EffectorLayer::Area);
    let (action, frame) =
        probe.draw(|ui| render_control_status_dialog(ui, &day, &view, &mut state));
    assert_eq!(action, Some(None), "a drawn dialog returned an action");
    let cancel = frame
        .position_of("Cancel")
        .unwrap_or_else(|| panic!("no cancel control: {}", frame.joined()));
    let set = frame
        .position_of("Set control status")
        .unwrap_or_else(|| panic!("no set control: {}", frame.joined()));
    assert!(cancel < set, "set was not drawn last: {}", frame.joined());
    assert!(frame.says("Weapons tight in force"), "{}", frame.joined());
    assert!(frame.says("Weapons tight: in force"), "{}", frame.joined());
    assert!(frame.says("withdraws 3 queued items"), "{}", frame.joined());
    assert!(frame.says("withdraws nothing queued"), "{}", frame.joined());
    assert!(
        frame.says("Sent to the node at node.local:7410"),
        "{}",
        frame.joined()
    );
    assert!(
        frame.says("Set by the configuration baseline."),
        "{}",
        frame.joined()
    );
    assert!(
        frame.says("Choose a status other than the one in force."),
        "{}",
        frame.joined()
    );
    let (clicked, _) = probe
        .click("Set control status", |ui| {
            render_control_status_dialog(ui, &day, &view, &mut state)
        })
        .unwrap_or_else(|frame| panic!("{}", frame.joined()));
    assert_eq!(
        clicked,
        Some(None),
        "a disabled set control acted on a click"
    );

    // A target with no reason: still shut, and the sentence changes.
    state.target = Some(WeaponsControlStatus::Hold);
    let (_, frame) = probe.draw(|ui| render_control_status_dialog(ui, &day, &view, &mut state));
    assert!(frame.says("enter one"), "{}", frame.joined());
    let label = "Set Area defence to Weapons hold";
    let (clicked, _) = probe
        .click(label, |ui| {
            render_control_status_dialog(ui, &day, &view, &mut state)
        })
        .unwrap_or_else(|frame| panic!("{}", frame.joined()));
    assert_eq!(clicked, Some(None), "set acted without a reason");

    // A reason too: open, and a click returns the draft with the reason trimmed.
    state.reason = "  civil traffic  ".to_owned();
    let (clicked, frame) = probe
        .click(label, |ui| {
            render_control_status_dialog(ui, &day, &view, &mut state)
        })
        .unwrap_or_else(|frame| panic!("{}", frame.joined()));
    let Some(Some(crate::panels::control_status_dialog::ControlStatusDialogAction::Set(draft))) =
        clicked
    else {
        panic!(
            "set did not return the draft: {clicked:?}: {}",
            frame.joined()
        );
    };
    assert_eq!(draft.layer, EffectorLayer::Area);
    assert_eq!(draft.status, WeaponsControlStatus::Hold);
    assert_eq!(draft.reason, "civil traffic");
    assert_eq!(draft.confirmation, None);

    // Cancel returns cancel, whatever the state.
    let (clicked, _) = probe
        .click("Cancel", |ui| {
            render_control_status_dialog(ui, &day, &view, &mut state)
        })
        .unwrap_or_else(|frame| panic!("{}", frame.joined()));
    assert_eq!(
        clicked,
        Some(Some(
            crate::panels::control_status_dialog::ControlStatusDialogAction::Cancel
        ))
    );
}

/// The two-person rule (D-140) on screen: its section is drawn, set stays shut with the
/// passphrase empty, and the draft carries the second person's credential once it is
/// complete.
#[test]
fn the_control_status_dialog_asks_for_the_second_person_under_the_two_person_rule() {
    use crate::panels::control_status_dialog::{
        render_control_status_dialog, ControlStatusDialogAction, ControlStatusDialogState,
    };
    use gungnir_model::{EffectorLayer, WeaponsControlStatus};

    let vocabulary = vocabulary();
    let view = crate::panels::control_status_dialog::ControlStatusDialogView {
        two_person: true,
        ..dialog_view(&vocabulary, WeaponsControlStatus::Tight)
    };
    let probe = RenderProbe::new();
    let day = theme::Palette::day();
    let mut state = ControlStatusDialogState {
        target: Some(WeaponsControlStatus::Hold),
        reason: "civil traffic".to_owned(),
        confirm_operator: "9".to_owned(),
        ..ControlStatusDialogState::open(EffectorLayer::Area)
    };
    let label = "Set Area defence to Weapons hold";
    let (clicked, frame) = probe
        .click(label, |ui| {
            render_control_status_dialog(ui, &day, &view, &mut state)
        })
        .unwrap_or_else(|frame| panic!("{}", frame.joined()));
    assert!(
        frame.says("Two-person rule in force (D-140)"),
        "{}",
        frame.joined()
    );
    assert!(
        frame.says("be someone other than you"),
        "{}",
        frame.joined()
    );
    assert!(
        frame.says("operator id and passphrase are needed"),
        "{}",
        frame.joined()
    );
    assert_eq!(clicked, Some(None), "set acted with an empty passphrase");

    state.confirm_passphrase = "pw".to_owned();
    let (clicked, frame) = probe
        .click(label, |ui| {
            render_control_status_dialog(ui, &day, &view, &mut state)
        })
        .unwrap_or_else(|frame| panic!("{}", frame.joined()));
    let Some(Some(ControlStatusDialogAction::Set(draft))) = clicked else {
        panic!(
            "set did not return the draft: {clicked:?}: {}",
            frame.joined()
        );
    };
    let Some(confirmation) = draft.confirmation else {
        panic!("the rule was in force and the draft carries no confirmation");
    };
    assert_eq!(confirmation.operator, 9);
    assert_eq!(confirmation.passphrase, "pw");
    assert!(
        state.confirm_passphrase.is_empty(),
        "the state kept the passphrase after the draft took it"
    );
    // The passphrase itself is never painted.
    assert!(!frame.says("pw"), "{}", frame.joined());
}

/// Loosening is said in so many words for a change towards free, and not for one
/// towards hold; and the last change's operator and role are named above the choices.
#[test]
fn the_control_status_dialog_says_when_a_change_loosens_and_who_set_the_status() {
    use crate::panels::control_status_dialog::{
        render_control_status_dialog, ControlStatusDialogState, LastChange,
    };
    use gungnir_model::{EffectorLayer, MissionTime, WeaponsControlStatus};

    let vocabulary = vocabulary();
    let probe = RenderProbe::new();
    let day = theme::Palette::day();
    let loosening = "Loosening: engagements this layer could not propose become proposable.";

    // At hold, both other choices loosen.
    let at_hold = crate::panels::control_status_dialog::ControlStatusDialogView {
        last_change: Some(LastChange {
            operator: "7",
            role: "Supervisor",
            at: MissionTime(120.0),
            reason: "civil traffic",
            confirmed_by: Some("operator 9 (Commander)"),
        }),
        ..dialog_view(&vocabulary, WeaponsControlStatus::Hold)
    };
    let mut state = ControlStatusDialogState::open(EffectorLayer::Area);
    let (_, frame) = probe.draw(|ui| render_control_status_dialog(ui, &day, &at_hold, &mut state));
    assert_eq!(
        frame
            .texts
            .iter()
            .filter(|t| t.as_str() == loosening)
            .count(),
        2,
        "{}",
        frame.joined()
    );
    assert!(
        frame.says("Set to Weapons hold by operator 7 (Supervisor) at T+120 s: civil traffic"),
        "{}",
        frame.joined()
    );
    assert!(
        frame.says("Confirmed by operator 9 (Commander)."),
        "{}",
        frame.joined()
    );

    // At free, nothing loosens, and the unset layer is said to be at hold by default.
    let at_free = crate::panels::control_status_dialog::ControlStatusDialogView {
        configured: false,
        ..dialog_view(&vocabulary, WeaponsControlStatus::Free)
    };
    let (_, frame) = probe.draw(|ui| render_control_status_dialog(ui, &day, &at_free, &mut state));
    assert!(!frame.says(loosening), "{}", frame.joined());
    assert!(
        frame.says("Hold by default: no baseline sets this layer."),
        "{}",
        frame.joined()
    );
}

/// A console that may not set is told why, keeps reading the status in force, and can
/// never return a set, whatever is typed.
#[test]
fn the_control_status_dialog_draws_why_the_console_may_not_set_and_never_sets() {
    use crate::panels::control_status_dialog::{
        render_control_status_dialog, ControlStatusDialogState,
    };
    use gungnir_model::{EffectorLayer, WeaponsControlStatus};

    let vocabulary = vocabulary();
    let view = crate::panels::control_status_dialog::ControlStatusDialogView {
        may_set: Err("Nobody is signed in: a control status change is recorded against a person."),
        ..dialog_view(&vocabulary, WeaponsControlStatus::Tight)
    };
    let probe = RenderProbe::new();
    let day = theme::Palette::day();
    let mut state = ControlStatusDialogState {
        target: Some(WeaponsControlStatus::Hold),
        reason: "civil traffic".to_owned(),
        ..ControlStatusDialogState::open(EffectorLayer::Area)
    };
    let (clicked, frame) = probe
        .click("Set Area defence to Weapons hold", |ui| {
            render_control_status_dialog(ui, &day, &view, &mut state)
        })
        .unwrap_or_else(|frame| panic!("{}", frame.joined()));
    assert!(frame.says("Nobody is signed in"), "{}", frame.joined());
    assert!(
        frame.says("Weapons tight in force"),
        "the status in force vanished with the controls: {}",
        frame.joined()
    );
    assert_eq!(
        clicked,
        Some(None),
        "a console that may not set returned a set"
    );
}

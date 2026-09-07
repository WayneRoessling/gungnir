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

/// Comparing one laydown with another needs the planning surface, which no register
/// entry builds; GAP-055 tracks the unbuilt panels.
const LAYDOWN_COMPARISON: Unavailable<'static> = Unavailable {
    owner: "gungnir-ui",
    gap: "GAP-055",
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
    };
    let probe = RenderProbe::new();
    let (clicked, frame) = probe.draw(|ui| render_approval_queue(ui, &view));

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
    let row = |decision: u64, endpoint, delivery| HandoffRow {
        decision,
        plan: decision + 500,
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
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_approval_queue(ui, &view));

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
        decision: 11,
        plan: 511,
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
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_approval_queue(ui, &view));
    assert!(
        !frame.says("Decided, not yet delivered"),
        "an empty outstanding list drew a heading: {}",
        frame.joined()
    );
}

/// The safety property from PN-07's module documentation, checked against draw order
/// rather than asserted in a comment: accept comes after reject and override, so
/// neither the reflexive click nor the reflexive keyboard traversal lands on it.
#[test]
fn the_decision_dialog_draws_accept_last() {
    use crate::panels::approval_queue::{PendingId, QueueRow, TimeRemaining, Verdict};
    use crate::panels::decision_dialog::{
        render_decision_dialog, DecisionDialogState, DecisionDialogView, OperatorIdentity,
    };

    let row = QueueRow {
        id: PendingId(1),
        plan_id: 9,
        assignments: 2,
        verdict: Verdict::RequiresHumanApproval,
        time_remaining: TimeRemaining::Seconds(18.0),
        pre_delegated: false,
        may_decide: true,
        escalated_from: None,
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
    };

    let probe = RenderProbe::new();
    let mut state = DecisionDialogState::default();
    let (choice, frame) = probe.draw(|ui| render_decision_dialog(ui, &view, &mut state));

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
        render_decision_dialog, DecisionDialogState, DecisionDialogView, Degraded, OperatorIdentity,
    };

    let row = QueueRow {
        id: PendingId(1),
        plan_id: 9,
        assignments: 1,
        verdict: Verdict::RequiresHumanApproval,
        time_remaining: TimeRemaining::NoExpiryConfigured,
        pre_delegated: false,
        may_decide: true,
        escalated_from: None,
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
    };

    let probe = RenderProbe::new();
    let mut state = DecisionDialogState::default();
    let (_, frame) = probe.draw(|ui| render_decision_dialog(ui, &view, &mut state));

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
    let (_, frame) = probe.draw(|ui| render_replay(ui, &view));

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
    let (_, frame) = probe.draw(|ui| render_reports(ui, &view, &mut draft));

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
    let (_, frame) = probe.draw(|ui| render_reports(ui, &view, &mut draft));

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
    let (_, frame) = probe.draw(|ui| render_reports(ui, &quiet, &mut draft));
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
    let (_, frame) = probe.draw(|ui| render_config_editor(ui, &view));

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
        vocabulary: &vocab,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_track_table(ui, &view));

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
        encryption: EncryptionState::NotConfigured,
        validity: BaselineValidity::NoWindowConfigured,
        profile: None,
        backend: BackendStatus::Embedded,
        session: None,
        mission_time: MissionTime(12.0),
        clock_source: ClockSource::Wall,
        health: SystemHealth::default(),
        control_status: &[],
        delegations: &[],
        alerts: AlertSummary::Unclassified { total: 4 },
        role: "Operator",
        vocabulary: &vocab,
        coverage: crate::panels::status_strip::CoverageStatus::NoApproaches,
        rehearsal: None,
        operator: OperatorLine::NobodySignedIn,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_status_strip(ui, &view));

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
    let (_, frame) = probe.draw(|ui| render_evidence_card(ui, &view));

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
        decision: 11,
        plan: 511,
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
    };
    let (_, frame) = probe.draw(|ui| render_approval_queue(ui, &queue));
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
        vocabulary: &vocab,
    };
    let (_, frame) = probe.draw(|ui| render_track_table(ui, &table));
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
        vocabulary: &defaults,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_track_table(ui, &view));
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
    let (_, frame) = probe.draw(|ui| render_track_table(ui, &view));
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
        },
        ControlStatusLine {
            layer: EffectorLayer::SelfDefence,
            status: WeaponsControlStatus::Hold,
            configured: false,
        },
    ];
    let vocab = Vocabulary::default();
    let view = StatusStripView {
        encryption: EncryptionState::NotConfigured,
        validity: BaselineValidity::NoWindowConfigured,
        profile: None,
        backend: BackendStatus::Embedded,
        session: None,
        mission_time: MissionTime(0.0),
        clock_source: ClockSource::Wall,
        health: SystemHealth::default(),
        control_status: &lines,
        delegations: &[],
        alerts: AlertSummary::Unclassified { total: 0 },
        role: "Operator",
        vocabulary: &vocab,
        coverage: crate::panels::status_strip::CoverageStatus::NoApproaches,
        rehearsal: None,
        operator: OperatorLine::NobodySignedIn,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_status_strip(ui, &view));

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
    let (_, frame) = probe.draw(|ui| render_sensor_management(ui, &view));

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
    let (_, frame) = probe.draw(|ui| render_sensor_management(ui, &view));

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
    let (_, frame) = probe.draw(|ui| render_sensor_management(ui, &view));

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
    let (_, frame) = probe.draw(|ui| render_requirements(ui, &view, &mut draft));

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
    let (_, frame) = probe.draw(|ui| render_requirements(ui, &view, &mut draft));

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
    let (_, frame) = probe.draw(|ui| render_requirements(ui, &view, &mut draft));

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
    let (action, frame) = probe.draw(|ui| render_requirements(ui, &view, &mut draft));

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
        backend: BackendStatus::Embedded,
        session: None,
        mission_time: gungnir_model::MissionTime(0.0),
        clock_source: ClockSource::Wall,
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
    };

    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_status_strip(ui, &base()));
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
    let (_, frame) = probe.draw(|ui| render_status_strip(ui, &view));
    assert!(frame.says("keystore is locked"), "{}", frame.joined());
    assert!(frame.says("NOT encrypted"), "{}", frame.joined());

    // And when it is on, it says so once and without alarm.
    let view = StatusStripView {
        encryption: EncryptionState::Active,
        ..base()
    };
    let (_, frame) = probe.draw(|ui| render_status_strip(ui, &view));
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
        backend: BackendStatus::Embedded,
        session: None,
        mission_time: gungnir_model::MissionTime(0.0),
        clock_source: ClockSource::Wall,
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
    };
    let probe = RenderProbe::new();
    let draw = |line| {
        let (_, frame) = probe.draw(|ui| render_status_strip(ui, &base(line)));
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

/// **A silent sensor means four different things and only two of them need somebody.**
/// PN-09 has to keep them apart: collapsing them into a health dot is how a scheduled
/// outage becomes an unnoticed hole, and how a real failure gets shrugged off as "that's
/// the maintenance window" (DN-21 §5, GAP-054).
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
                &SensorHealthView {
                    health: &health,
                    encryption: EncryptionState::Active,
                    sensors: lines,
                    clocks,
                    detectors: &detectors,
                    terrain: crate::panels::sensor_health::TerrainLine {
                        masking: false,
                        detail: "no terrain configured",
                    },
                    feeds,
                    cooperative_feeds: &[],
                    peers: &[],
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
        }],
    );
    assert!(
        text.contains("400 detections") && text.contains("3 not decoded"),
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
    };

    let probe = RenderProbe::new();
    let mut notes = String::new();
    let (_, frame) = probe.draw(|ui| render_commander_summary(ui, &view, &mut notes));
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
    let (_, frame) = probe.draw(|ui| render_commander_summary(ui, &view, &mut notes));
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
        backend,
        session: None,
        mission_time: gungnir_model::MissionTime(0.0),
        clock_source: ClockSource::Wall,
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
        render_status_strip(ui, &base(node(true, LinkFreshness::Heard { age_s: 0.8 })));
    });
    assert!(frame.says("heard 0.8 s ago"), "{}", frame.joined());

    let (_, frame) = probe.draw(|ui| {
        render_status_strip(ui, &base(node(true, LinkFreshness::Overdue { age_s: 5.2 })));
    });
    assert!(frame.says("nothing heard for 5.2 s"), "{}", frame.joined());

    // Detached already says so; a second number would compete with it.
    let (_, frame) = probe.draw(|ui| {
        render_status_strip(
            ui,
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
        render_sensor_management(ui, &base(Ok(&[])));
    });
    assert!(
        frame.says("No mode change improves coverage"),
        "{}",
        frame.joined()
    );

    let (_, frame) = probe.draw(|ui| {
        render_sensor_management(ui, &base(Err("no approaches are declared")));
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
        render_sensor_management(ui, &base(Ok(std::slice::from_ref(&line))));
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
    };
    let probe = RenderProbe::new();
    let mut notes = String::new();
    let (_, frame) = probe.draw(|ui| render_commander_summary(ui, &view, &mut notes));
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
    let (_, frame) = probe.draw(|ui| render_commander_summary(ui, &none, &mut notes));
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
    let (_, frame) = probe.draw(|ui| render_reports(ui, &view, &mut draft));
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
    let (_, frame) = probe.draw(|ui| render_reports(ui, &closed, &mut draft));
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
    };
    let probe = RenderProbe::new();
    let mut notes = String::new();
    let (_, frame) = probe.draw(|ui| render_commander_summary(ui, &view, &mut notes));
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
    let (_, frame) = probe.draw(|ui| render_commander_summary(ui, &unscored, &mut notes));
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
    use crate::panels::intercept_panel::{render_intercept_panel, Alternatives, FiresCheckLine};
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
        render_intercept_panel(ui, &plan, &[], &checks, &[], &Alternatives::default());
    });
    assert!(
        frame.says("Target track 7, location error 40 m, firing unit 3"),
        "{}",
        frame.joined()
    );
    assert!(frame.says("PASSED location accuracy"), "{}", frame.joined());
    assert!(frame.says("FAILED no-fire areas"), "{}", frame.joined());
}

/// **A refused alternative is drawn with its denial rather than filtered out** (GAP-032).
///
/// The failure this guards against is the quiet one: a panel that showed only the options
/// still open would leave an operator asking on the radio for the option policy had
/// already refused, and the answer would arrive later than this line does.
#[test]
fn the_intercept_panel_draws_a_refused_alternative_with_its_denial() {
    use crate::panels::intercept_panel::{render_intercept_panel, AlternativeLine, Alternatives};
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
            &PlanView::default(),
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
        };
        let (_, frame) = probe.draw(|ui| render_audit(ui, &view, &mut draft));
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
            decision: 11,
            plan: 511,
            endpoint: Some("battery-2"),
            operator: "operator 7",
            role: "Supervisor",
            issued: MissionTime(100.0),
            delivery: &delivered,
            reports: &reports,
        },
        HandoffRow {
            decision: 12,
            plan: 512,
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
    };
    let probe = RenderProbe::new();
    let mut draft = SignInDraft::default();
    let (_, frame) = probe.draw(|ui| render_audit(ui, &view, &mut draft));

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
        decision: 11,
        plan: 511,
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
    };
    let probe = RenderProbe::new();
    let mut draft = SignInDraft::default();
    let (_, frame) = probe.draw(|ui| render_audit(ui, &view, &mut draft));
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
        backend: BackendStatus::Embedded,
        session: None,
        mission_time: gungnir_model::MissionTime(0.0),
        clock_source: ClockSource::Wall,
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
    };
    let probe = RenderProbe::new();

    // The ordinary case draws nothing: almost no deployment configures a window, and an
    // element that said "baseline valid" on every frame would be read past.
    let (_, frame) = probe.draw(|ui| render_status_strip(ui, &base()));
    assert!(!frame.says("baseline"), "{}", frame.joined());

    let expired = StatusStripView {
        validity: BaselineValidity::Expired {
            since: gungnir_model::MissionTime(3600.0),
        },
        ..base()
    };
    let (_, frame) = probe.draw(|ui| render_status_strip(ui, &expired));
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
    let (_, frame) = probe.draw(|ui| render_status_strip(ui, &not_yet));
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
    let (_, frame) = probe.draw(|ui| render_status_strip(ui, &in_force));
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
        },
        hazards: HazardCurrency {
            declared: 0,
            baseline_version: 1,
        },
        coverage: NothingToDraw::Available,
        comparison: LAYDOWN_COMPARISON,
        geofences_visible: true,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_coverage_layers(ui, &view));

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
    let (_, frame) = probe.draw(|ui| render_coverage_layers(ui, &view));
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
        },
        hazards: HazardCurrency {
            declared: 2,
            baseline_version: 7,
        },
        coverage: NothingToDraw::Available,
        comparison: LAYDOWN_COMPARISON,
        geofences_visible: true,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_coverage_layers(ui, &view));
    assert!(frame.says("Hazards and barriers (2)"), "{}", frame.joined());
    assert!(frame.says("baseline revision 7"), "{}", frame.joined());
    assert!(frame.says("static layer"), "{}", frame.joined());

    let unplaced = CoverageLayersView {
        counts: LayerCounts {
            rings: 0,
            gaps: 0,
            hazards: 0,
            geofences: 0,
        },
        ..view
    };
    let (_, frame) = probe.draw(|ui| render_coverage_layers(ui, &unplaced));
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
        },
        hazards: HazardCurrency {
            declared: 0,
            baseline_version: 1,
        },
        coverage: NothingToDraw::Because {
            reason: "this deployment has declared no local frame origin",
        },
        comparison: LAYDOWN_COMPARISON,
        geofences_visible: true,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_coverage_layers(ui, &view));

    assert!(frame.says("nothing to draw"), "{}", frame.joined());
    assert!(frame.says("no local frame origin"), "{}", frame.joined());
    // Nothing is hidden here, so the hidden-layer warning must not appear: it would
    // send an operator looking for a toggle that is already on.
    assert!(!frame.says("A hidden layer is not"), "{}", frame.joined());
}

/// Comparing two laydowns needs the planning surface, and the panel names the gap
/// rather than offering a control that would do nothing.
#[test]
fn the_comparison_control_names_the_gap_that_would_build_it() {
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
        },
        hazards: HazardCurrency {
            declared: 0,
            baseline_version: 1,
        },
        coverage: NothingToDraw::Available,
        comparison: LAYDOWN_COMPARISON,
        geofences_visible: true,
    };
    let probe = RenderProbe::new();
    let (_, frame) = probe.draw(|ui| render_coverage_layers(ui, &view));
    assert!(frame.says("GAP-055"), "{}", frame.joined());
}

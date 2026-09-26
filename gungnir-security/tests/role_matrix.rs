// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Every role against every action, against the authority matrix (GAP-111, the
//! `gungnir-security` row of `docs/verification-capability-table.md` §2).
//!
//! **Three things are held in step here**, and a change to any one of them without the
//! others fails:
//!
//! 1. `docs/mission/roles-and-stakeholders.md` §4, which this file reads and compares with
//! 2. [`SECTION_4`], the table transcribed literally as data, whose cells
//!    [`READINGS`] turn into grants of the actions [`ROW_ACTIONS`] says each row stands for;
//! 3. `gungnir_security::authz::role_permits`, asked about every role in `Role::ALL` and
//!    every constant in `gungnir_security::actions`, `REQUIREMENT` and `ASSIGN_ROLE`
//!    included.
//!
//! What the test found the first time it ran, and how each was settled, is D-88 and
//! `docs/record/2026-09-26/the-security-row-tested-and-the-node-audited.md`.

use gungnir_security::actions;
use gungnir_security::authz::role_permits;
use gungnir_security::Role;
use std::collections::BTreeSet;

/// §4's six columns, in the order the table gives them.
const COLUMNS: [Role; 6] = [
    Role::Operator,
    Role::Supervisor,
    Role::Commander,
    Role::SensorManager,
    Role::Analyst,
    Role::IntelligenceAnalyst,
];

/// `docs/mission/roles-and-stakeholders.md` §4, transcribed literally: the decision, then
/// its cells under Operator, Supervisor, Commander, Sensor manager, Analyst and
/// Intelligence analyst. The escrow row's eighth cell, a note, is not a column and is not
/// transcribed; the security officer it names is [`SECURITY_OFFICER`].
const SECTION_4: &[(&str, [&str; 6])] = &[
    (
        "Identity declaration (per class policy)",
        [
            "some classes",
            "all classes",
            "all classes",
            "",
            "",
            "intelligence declarations",
        ],
    ),
    (
        "Engagement acceptance, point layer",
        ["delegated", "yes", "yes", "", "", ""],
    ),
    (
        "Engagement acceptance, area layer",
        ["", "pre-delegated cases", "yes", "", "", ""],
    ),
    ("Weapons control status", ["", "yes", "yes", "", "", ""]),
    ("Hold or cease", ["", "yes", "yes", "", "", ""]),
    (
        "Override a recommendation (GAP-111)",
        ["", "yes", "yes", "", "", ""],
    ),
    (
        "Sensor tasking",
        ["camera cue", "concur", "", "yes", "", "request"],
    ),
    ("Plan apply", ["", "yes", "yes", "", "", ""]),
    (
        "Apply a sensor or calibration baseline (GAP-111)",
        ["", "", "", "yes", "", ""],
    ),
    ("Model promotion", ["", "concur", "", "", "yes", ""]),
    ("Product release", ["", "yes", "yes", "", "", "yes"]),
    (
        "Publish to coalition exchange (GAP-065; amended 2026-09-08 to add Supervisor alongside the Product release row above)",
        ["", "yes", "yes", "", "", "yes"],
    ),
    ("Accept coverage gap", ["", "", "yes", "", "", ""]),
    (
        "Recover an escrowed journal key (D-30)",
        ["", "", "", "", "", ""],
    ),
    (
        "Reconciliation conflict resolution (Operator amended 2026-09-25: D-53)",
        ["yes", "yes", "yes", "", "", ""],
    ),
    (
        "View the picture (GAP-111)",
        ["yes", "yes", "yes", "yes", "yes", "yes"],
    ),
    (
        "Submit a detection by hand (GAP-111)",
        ["yes", "yes", "", "", "", ""],
    ),
    (
        "Export a report (GAP-111)",
        ["", "yes", "yes", "", "yes", "yes"],
    ),
    (
        "State, decline or satisfy a collection requirement (GAP-111; DN-11 §5)",
        ["", "", "", "decline", "", "state, satisfy"],
    ),
    (
        "Conduct an after-action review (GAP-111; DN-20)",
        ["", "", "", "", "yes", ""],
    ),
    (
        "Acknowledge a watch handover (GAP-111; DN-21)",
        ["yes", "yes", "yes", "yes", "", ""],
    ),
    (
        "Key in an effector's report or a warned party's acknowledgement (GAP-040, GAP-042)",
        ["", "", "", "", "", ""],
    ),
    (
        "Assign a role to an account (GAP-057)",
        ["", "", "", "", "", ""],
    ),
];

/// Which coarse actions each §4 row stands for. A row with none has no coarse action yet
/// (GAP-058's per-class refinement; the coverage-gap control, GAP-087).
const ROW_ACTIONS: &[(&str, &[&str])] = &[
    ("Identity declaration (per class policy)", &[]),
    ("Engagement acceptance, point layer", &[actions::DECIDE_PLAN]),
    ("Engagement acceptance, area layer", &[actions::DECIDE_PLAN]),
    ("Weapons control status", &[actions::SET_CONTROL_STATUS]),
    // Holding or ceasing a layer is setting it to `Hold` (DN-09; the usability round's
    // US-07 names `SET_CONTROL_STATUS` for it).
    ("Hold or cease", &[actions::SET_CONTROL_STATUS]),
    (
        "Override a recommendation (GAP-111)",
        &[actions::OVERRIDE_PLAN],
    ),
    ("Sensor tasking", &[actions::TASK_SENSOR]),
    ("Plan apply", &[actions::APPLY_CONFIG]),
    (
        "Apply a sensor or calibration baseline (GAP-111)",
        &[actions::APPLY_CONFIG],
    ),
    ("Model promotion", &[actions::PROMOTE_MODEL]),
    ("Product release", &[actions::RELEASE_PRODUCT]),
    (
        "Publish to coalition exchange (GAP-065; amended 2026-09-08 to add Supervisor alongside the Product release row above)",
        &[actions::PUBLISH_EXCHANGE],
    ),
    ("Accept coverage gap", &[]),
    (
        "Recover an escrowed journal key (D-30)",
        &[actions::KEY_ESCROW_RECOVER],
    ),
    // D-53: a conflict the rule cannot rank is left "for a person permitted
    // `plan.decide`".
    (
        "Reconciliation conflict resolution (Operator amended 2026-09-25: D-53)",
        &[actions::DECIDE_PLAN],
    ),
    ("View the picture (GAP-111)", &[actions::VIEW_PICTURE]),
    (
        "Submit a detection by hand (GAP-111)",
        &[actions::SUBMIT_DETECTION],
    ),
    ("Export a report (GAP-111)", &[actions::EXPORT_REPORT]),
    (
        "State, decline or satisfy a collection requirement (GAP-111; DN-11 §5)",
        &[actions::REQUIREMENT],
    ),
    (
        "Conduct an after-action review (GAP-111; DN-20)",
        &[actions::CONDUCT_REVIEW],
    ),
    (
        "Acknowledge a watch handover (GAP-111; DN-21)",
        &[actions::ACKNOWLEDGE_HANDOVER],
    ),
    (
        "Key in an effector's report or a warned party's acknowledgement (GAP-040, GAP-042)",
        &[actions::EFFECTOR_REPORT, actions::ACKNOWLEDGE_WARNING],
    ),
    (
        "Assign a role to an account (GAP-057)",
        &[actions::ASSIGN_ROLE],
    ),
];

/// What a cell means for the coarse action, as §4's own "How the code reads this table"
/// paragraph states it. The second field limits a reading to one row; `None` is every row.
const READINGS: &[(&str, Option<&str>, bool)] = &[
    ("", None, false),
    ("yes", None, true),
    // The policy authority rules narrow these per layer and class (DN-09 §5).
    ("delegated", None, true),
    ("pre-delegated cases", None, true),
    ("decline", None, true),
    ("state, satisfy", None, true),
    // Concurring in a tasking is `sensor.task` (DN-11 §5).
    ("concur", Some("Sensor tasking"), true),
    // Concurring in a promotion is not promoting, and nothing promotes (DN-24 §9).
    ("concur", Some("Model promotion"), false),
    // The coarse action would be wider than the cell (GAP-058).
    ("camera cue", None, false),
    ("request", None, false),
];

/// The planner's actions (the owner, 2026-09-05; §4's "Roles without a column").
const PLANNER: &[&str] = &[actions::VIEW_PICTURE];

/// The security officer's actions (D-30; §4's escrow row).
const SECURITY_OFFICER: &[&str] = &[actions::KEY_ESCROW_RECOVER];

/// The rows the administrator does not hold (D-88; §4's "Roles without a column"): the
/// engagement chain, and escrow recovery.
const ADMINISTRATOR_WITHHELD: &[&str] = &[
    "Engagement acceptance, point layer",
    "Engagement acceptance, area layer",
    "Weapons control status",
    "Hold or cease",
    "Override a recommendation (GAP-111)",
    "Reconciliation conflict resolution (Operator amended 2026-09-25: D-53)",
    "Recover an escrowed journal key (D-30)",
];

/// Every constant in `gungnir_security::actions`, by name, `REQUIREMENT` and
/// `ASSIGN_ROLE` included. [`the_action_list_is_every_constant_in_the_module`] reads the
/// module's source so a constant added there and not here fails.
const EVERY_ACTION: &[(&str, &str)] = &[
    ("VIEW_PICTURE", actions::VIEW_PICTURE),
    ("SUBMIT_DETECTION", actions::SUBMIT_DETECTION),
    ("DECIDE_PLAN", actions::DECIDE_PLAN),
    ("OVERRIDE_PLAN", actions::OVERRIDE_PLAN),
    ("APPLY_CONFIG", actions::APPLY_CONFIG),
    ("TASK_SENSOR", actions::TASK_SENSOR),
    ("PROMOTE_MODEL", actions::PROMOTE_MODEL),
    ("EXPORT_REPORT", actions::EXPORT_REPORT),
    ("SET_CONTROL_STATUS", actions::SET_CONTROL_STATUS),
    ("EFFECTOR_REPORT", actions::EFFECTOR_REPORT),
    ("ACKNOWLEDGE_WARNING", actions::ACKNOWLEDGE_WARNING),
    ("RELEASE_PRODUCT", actions::RELEASE_PRODUCT),
    ("PUBLISH_EXCHANGE", actions::PUBLISH_EXCHANGE),
    ("REVIEW_CONDUCT", actions::REVIEW_CONDUCT),
    ("REQUIREMENT", actions::REQUIREMENT),
    ("CONDUCT_REVIEW", actions::CONDUCT_REVIEW),
    ("ACKNOWLEDGE_HANDOVER", actions::ACKNOWLEDGE_HANDOVER),
    ("KEY_ESCROW_RECOVER", actions::KEY_ESCROW_RECOVER),
    ("ASSIGN_ROLE", actions::ASSIGN_ROLE),
];

fn row_actions(decision: &str) -> &'static [&'static str] {
    ROW_ACTIONS
        .iter()
        .find(|(d, _)| *d == decision)
        .map_or_else(
            || panic!("§4 row {decision:?} names no actions in ROW_ACTIONS"),
            |(_, a)| *a,
        )
}

fn reading(decision: &str, cell: &str) -> bool {
    READINGS
        .iter()
        .find(|(c, only, _)| *c == cell && only.is_none_or(|row| row == decision))
        .map_or_else(
            || {
                panic!(
                    "no reading for the cell {cell:?} in §4 row {decision:?}: add one to READINGS"
                )
            },
            |(_, _, grants)| *grants,
        )
}

/// What §4 says `role` may do with `action`.
fn section_4_permits(role: Role, action: &str) -> bool {
    match role {
        Role::Planner => PLANNER.contains(&action),
        Role::SecurityOfficer => SECURITY_OFFICER.contains(&action),
        Role::Administrator => !SECTION_4.iter().any(|(decision, _)| {
            ADMINISTRATOR_WITHHELD.contains(decision) && row_actions(decision).contains(&action)
        }),
        _ => {
            let column = COLUMNS
                .iter()
                .position(|c| *c == role)
                .unwrap_or_else(|| panic!("{role:?} has neither a column nor a rule"));
            SECTION_4.iter().any(|(decision, cells)| {
                let actions = row_actions(decision);
                actions.contains(&action) && reading(decision, cells[column])
            })
        }
    }
}

/// The table under `## 4.` in the document, as (decision, first six cells) rows.
fn section_4_in_the_document() -> Vec<(String, Vec<String>)> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/mission/roles-and-stakeholders.md");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let section = text.split("\n## 4.").nth(1).expect("the document has a §4");
    section
        .lines()
        .skip_while(|l| !l.starts_with('|'))
        .take_while(|l| l.starts_with('|'))
        .skip(2) // the header and the separator
        .map(|line| {
            let cells: Vec<String> = line
                .trim()
                .trim_matches('|')
                .split('|')
                .map(|c| c.trim().to_owned())
                .collect();
            (cells[0].clone(), cells[1..7].to_vec())
        })
        .collect()
}

#[test]
fn the_transcription_is_what_section_4_says() {
    let document = section_4_in_the_document();
    let transcribed: Vec<(String, Vec<String>)> = SECTION_4
        .iter()
        .map(|(d, cells)| {
            (
                (*d).to_owned(),
                cells.iter().map(|c| (*c).to_owned()).collect(),
            )
        })
        .collect();
    assert_eq!(
        document, transcribed,
        "docs/mission/roles-and-stakeholders.md §4 and SECTION_4 differ: change the \
         document first, then the transcription, then role_permits"
    );
}

#[test]
fn every_role_has_a_column_or_a_rule() {
    let covered: BTreeSet<String> = COLUMNS
        .iter()
        .chain([Role::Administrator, Role::Planner, Role::SecurityOfficer].iter())
        .map(|r| format!("{r:?}"))
        .collect();
    let all: BTreeSet<String> = Role::ALL.iter().map(|r| format!("{r:?}")).collect();
    assert_eq!(covered, all);
}

#[test]
fn every_action_stands_for_a_row() {
    for (name, action) in EVERY_ACTION {
        assert!(
            ROW_ACTIONS.iter().any(|(_, a)| a.contains(action)),
            "{name} ({action}) stands for no §4 row, so no role could be expected to hold it"
        );
    }
    for (decision, _) in SECTION_4 {
        let _ = row_actions(decision);
    }
}

/// The comparison the verification row asks for: every role, every action.
#[test]
fn role_permits_is_section_4_for_every_role_and_every_action() {
    let mut differ = Vec::new();
    for role in Role::ALL {
        for (name, action) in EVERY_ACTION {
            let expected = section_4_permits(*role, action);
            let actual = role_permits(*role, action);
            if expected != actual {
                differ.push(format!(
                    "{role:?} {name}: §4 says {expected}, role_permits says {actual}"
                ));
            }
        }
    }
    assert!(
        differ.is_empty(),
        "role_permits and §4 disagree in {} places:\n{}",
        differ.len(),
        differ.join("\n")
    );
}

/// The list above, `actions::ALL` and the module's source name the same actions.
#[test]
fn the_action_list_is_every_constant_in_the_module() {
    let source = include_str!("../src/lib.rs");
    let module = source
        .split("pub mod actions {")
        .nth(1)
        .expect("lib.rs has the actions module");
    let declared: BTreeSet<(String, String)> = module
        .lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("pub const "))
        .filter_map(|l| {
            let (name, rest) = l.split_once(": &str = \"")?;
            let value = rest.strip_suffix("\";")?;
            Some((name.to_owned(), value.to_owned()))
        })
        .collect();
    let listed: BTreeSet<(String, String)> = EVERY_ACTION
        .iter()
        .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
        .collect();
    assert_eq!(declared, listed, "EVERY_ACTION is not the actions module");

    let distinct: BTreeSet<&str> = EVERY_ACTION.iter().map(|(_, v)| *v).collect();
    let all: BTreeSet<&str> = actions::ALL.iter().copied().collect();
    assert_eq!(
        all, distinct,
        "actions::ALL is not every action the module declares"
    );
    assert_eq!(actions::ALL.len(), all.len(), "actions::ALL repeats a name");
}

/// The four corrections the comparison forced, named so a reader sees them without
/// running the matrix (D-88).
#[test]
fn the_corrections_hold() {
    assert!(role_permits(Role::Supervisor, actions::SET_CONTROL_STATUS));
    for withheld in [
        actions::DECIDE_PLAN,
        actions::OVERRIDE_PLAN,
        actions::SET_CONTROL_STATUS,
        actions::KEY_ESCROW_RECOVER,
    ] {
        assert!(!role_permits(Role::Administrator, withheld), "{withheld}");
    }
    assert!(role_permits(
        Role::IntelligenceAnalyst,
        actions::REQUIREMENT
    ));
    assert!(role_permits(Role::SensorManager, actions::REQUIREMENT));
    assert!(role_permits(Role::Analyst, actions::CONDUCT_REVIEW));
    assert!(actions::is_known(actions::REQUIREMENT));
    assert!(actions::is_known(actions::ASSIGN_ROLE));
}

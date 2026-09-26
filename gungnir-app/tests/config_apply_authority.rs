// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! GAP-162 (D-91): applying a baseline is authorized inside `ConfigEditorState::apply`,
//! section by section against the baseline in force, as
//! `docs/mission/roles-and-stakeholders.md` §4 ("Applying a baseline, section by section")
//! says.
//!
//! Until 2026-09-26 PN-14 hid the apply control from a role without `config.apply` and
//! nothing else checked: a sensor manager, who held `config.apply` for its calibration
//! row, could apply a baseline that changed weapons control status and the authority rules
//! all the same, and anything that called `apply` wrote whatever it was given.

use gungnir_app::state::AppState;
use gungnir_app::sustainment::{self, ConfigEditorState};
use gungnir_config::{ConfigBaseline, FileConfigStore};
use gungnir_model::{EffectorLayer, WeaponsControlStatus};
use gungnir_security::{actions, AuditLog, Role};
use gungnir_ui::panels::config_editor::{ApplyState, GovernedProfiles};

/// A desktop running revision 1 of a baseline file, with an editor holding a validated
/// candidate at revision 2 that `edit` has changed.
struct Desk {
    state: AppState,
    editor: ConfigEditorState,
    path: std::path::PathBuf,
    dir: std::path::PathBuf,
}

impl Drop for Desk {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn desk(name: &str, role: Role, edit: impl FnOnce(&mut ConfigBaseline)) -> Desk {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-config-authority-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("data dir");
    let path = dir.join("baseline.json");
    let running = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        revision: 1,
        ..ConfigBaseline::default()
    };
    let write = |b: &ConfigBaseline| {
        std::fs::write(&path, serde_json::to_string_pretty(b).expect("encodes")).expect("written");
    };
    write(&running);
    let mut state = AppState::with_config_and_store(
        running.clone(),
        Some(FileConfigStore::new(
            &path,
            gungnir_app::state::known_vocabulary(),
        )),
    )
    .expect("desktop state");
    state.set_role(role);
    let mut candidate = ConfigBaseline {
        revision: 2,
        ..running
    };
    edit(&mut candidate);
    write(&candidate);
    let mut editor = ConfigEditorState::default();
    editor.reload(&state);
    editor.validate(&state);
    Desk {
        state,
        editor,
        path,
        dir,
    }
}

fn control_status_free(b: &mut ConfigBaseline) {
    b.policy
        .control_status
        .by_layer
        .insert(EffectorLayer::Point, WeaponsControlStatus::Free);
}

fn sensing_change(b: &mut ConfigBaseline) {
    b.sensor_task_ack_window_s += 5.0;
}

fn security_change(b: &mut ConfigBaseline) {
    b.retention = Some(gungnir_model::RetentionPolicy::default());
}

fn applies(desk: &Desk) -> usize {
    desk.state
        .audit
        .entries()
        .iter()
        .filter(|e| e.action == actions::APPLY_CONFIG)
        .count()
}

fn on_disk(desk: &Desk) -> ConfigBaseline {
    serde_json::from_str(&std::fs::read_to_string(&desk.path).expect("read")).expect("decodes")
}

/// The finding itself: a sensor manager's baseline that sets weapons control status is
/// refused whole -- together with the sensing change beside it -- nothing is written past
/// the candidate already on disk, nothing is applied, nothing is audited, and the refusal
/// names the section and the action it needs.
#[test]
fn a_sensor_manager_may_not_change_control_status_through_a_baseline() {
    let mut desk = desk("sm-control", Role::SensorManager, |b| {
        control_status_free(b);
        sensing_change(b);
    });
    let err = desk
        .editor
        .apply(&mut desk.state)
        .expect_err("a sensor manager set weapons control status through a file");
    assert!(err.contains("policy.control_status"), "{err}");
    assert!(err.contains(actions::APPLY_CONFIG), "{err}");
    assert!(
        !err.contains("sensor_task_ack_window_s"),
        "the sensing change is the sensor manager's and was named as refused: {err}"
    );
    assert_eq!(applies(&desk), 0, "a refusal was audited as an apply");
    assert!(
        desk.state
            .config_store
            .as_ref()
            .and_then(FileConfigStore::applied)
            .is_none(),
        "the store applied a refused baseline"
    );
}

/// PN-14 says so before anyone clicks: the apply state is `Refused`, listing the one
/// section and the action it needs, not an apply button.
#[test]
fn pn14_names_the_refused_section_before_apply_is_pressed() {
    let desk = desk("sm-view", Role::SensorManager, control_status_free);
    let refused = desk.editor.refused_sections(&desk.state);
    let sections = sustainment::config_sections(&desk.state);
    let audit = sustainment::audit_lines(&desk.state);
    let view = sustainment::config_editor_view(
        &desk.state,
        &desk.editor,
        &sections,
        &audit,
        "SensorManager",
        None,
        GovernedProfiles::NothingDeclared,
        &refused,
    );
    match view.apply {
        ApplyState::Refused { role, sections } => {
            assert_eq!(role, "SensorManager");
            assert_eq!(sections.len(), 1, "{sections:?}");
            assert_eq!(sections[0].section, "policy.control_status");
            assert_eq!(sections[0].needs, actions::APPLY_CONFIG);
        }
        other => panic!("PN-14 offered apply for a refused candidate: {other:?}"),
    }
}

/// What the sensor manager's row gives it: a baseline that changes only sensing
/// sections applies, and is audited naming what it changed.
#[test]
fn a_sensor_manager_applies_a_sensing_baseline() {
    let mut desk = desk("sm-sensing", Role::SensorManager, sensing_change);
    desk.editor
        .apply(&mut desk.state)
        .expect("a sensing change is the sensor manager's");
    assert_eq!(applies(&desk), 1);
    let entry = desk
        .state
        .audit
        .entries()
        .iter()
        .rev()
        .find(|e| e.action == actions::APPLY_CONFIG)
        .expect("audited")
        .clone();
    assert!(
        entry.detail.contains("sensor_task_ack_window_s"),
        "the audit entry does not say what changed: {}",
        entry.detail
    );
    assert_eq!(on_disk(&desk).revision, 2);
}

/// D-88 at the console, D-91 through a file: the administrator holds `config.apply` but
/// not weapons control status, so a baseline that sets it is refused and names the
/// action it needs.
#[test]
fn an_administrator_may_not_change_the_engagement_chain_through_a_baseline() {
    let mut desk = desk("admin-control", Role::Administrator, control_status_free);
    let err = desk
        .editor
        .apply(&mut desk.state)
        .expect_err("an administrator set weapons control status through a file");
    assert!(err.contains("policy.control_status"), "{err}");
    assert!(err.contains(actions::SET_CONTROL_STATUS), "{err}");
    assert_eq!(applies(&desk), 0);
}

/// The supervisor holds the engagement chain and applies it; it does not hold account
/// administration, so a security change is the administrator's, and the administrator's
/// security change applies.
#[test]
fn the_engagement_chain_is_the_supervisor_s_and_security_is_the_administrator_s() {
    let mut supervisor = desk("sup-control", Role::Supervisor, control_status_free);
    supervisor
        .editor
        .apply(&mut supervisor.state)
        .expect("the supervisor sets weapons control status");
    assert_eq!(
        on_disk(&supervisor)
            .policy
            .control_status
            .for_layer(EffectorLayer::Point),
        WeaponsControlStatus::Free
    );

    let mut supervisor = desk("sup-security", Role::Supervisor, security_change);
    let err = supervisor
        .editor
        .apply(&mut supervisor.state)
        .expect_err("a supervisor changed the security section");
    assert!(err.contains("retention"), "{err}");
    assert!(err.contains(actions::ASSIGN_ROLE), "{err}");

    let mut administrator = desk("admin-security", Role::Administrator, security_change);
    administrator
        .editor
        .apply(&mut administrator.state)
        .expect("the security section is the administrator's");
    assert!(on_disk(&administrator).retention.is_some());
}

/// The check is inside `apply`, not only on the panel: a role with neither apply action
/// is refused by name before anything is read, whatever the candidate changes.
#[test]
fn a_role_that_may_not_apply_is_refused_inside_apply() {
    let mut desk = desk("operator", Role::Operator, sensing_change);
    let err = desk
        .editor
        .apply(&mut desk.state)
        .expect_err("an operator applied a baseline");
    assert!(err.contains(actions::APPLY_CONFIG), "{err}");
    assert!(err.contains(actions::APPLY_SENSING_CONFIG), "{err}");
    assert_eq!(applies(&desk), 0);
}

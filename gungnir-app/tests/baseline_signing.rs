// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A baseline applied on PN-14 is judged by its signature (GAP-210, DN-22 §15).
//!
//! The key is set on the state rather than through `GUNGNIR_BASELINE_KEY`, so these tests
//! cannot race each other over one process's environment.

use gungnir_app::state::AppState;
use gungnir_app::sustainment::ConfigEditorState;
use gungnir_config::{BackendConfig, BaselineSignature, ConfigBaseline, FileConfigStore};
use gungnir_security::baseline_signing::{BaselineSigner, BaselineVerifyingKey};
use gungnir_security::AuditLog;

const PASSPHRASE: &str = "a baseline passphrase";

struct Desk {
    state: AppState,
    path: std::path::PathBuf,
    signer: BaselineSigner,
    dir: std::path::PathBuf,
}

impl Drop for Desk {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A desktop running revision 1 from a file, an administrator at it, the deployment's
/// key pinned, and that file edited to revision 2 and not yet signed.
fn desk(name: &str, connected: bool) -> Desk {
    let dir = std::env::temp_dir().join(format!("gungnir-baseline-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("data dir");
    let keys = dir.join("keys");
    let pem = BaselineSigner::provision(&keys, PASSPHRASE).expect("provisioned");
    let signer = BaselineSigner::unlock(&keys, PASSPHRASE).expect("unlocked");
    let path = dir.join("baseline.json");
    let running = ConfigBaseline {
        data_dir: dir.join("data").to_string_lossy().into_owned(),
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
    state.set_role(gungnir_security::Role::Administrator);
    state.baseline_key = Ok(Some(BaselineVerifyingKey::from_pem(&pem).expect("key")));
    if connected {
        // The deployment the baseline declares, which is what makes this desktop a
        // node's console; nothing here needs the link itself to come up.
        state.config.backend = BackendConfig::Remote {
            endpoint: "https://localhost:1".to_owned(),
        };
    }
    write(&ConfigBaseline {
        revision: 2,
        ..running
    });
    Desk {
        state,
        path,
        signer,
        dir,
    }
}

fn sign(desk: &Desk) -> String {
    let store = FileConfigStore::new(&desk.path, gungnir_app::state::known_vocabulary());
    let baseline = gungnir_config::ConfigStore::load(&store).expect("loads");
    let made = desk
        .signer
        .sign(&baseline.signing_bytes().expect("bytes"))
        .expect("signs");
    BaselineSignature::from_der(made.key_version, made.key_fingerprint.clone(), &made.der)
        .write_beside(&desk.path)
        .expect("signature written");
    made.key_fingerprint
}

fn apply(desk: &mut Desk) -> Result<(), String> {
    let mut editor = ConfigEditorState::default();
    editor.reload(&desk.state);
    editor.validate(&desk.state);
    editor.apply(&mut desk.state)
}

/// A node's console refuses an unsigned candidate and writes nothing; signed, the same
/// candidate applies and the audit entry names the key that signed it.
#[test]
fn a_console_applies_only_a_signed_baseline_and_names_its_signer() {
    let mut desk = desk("console", true);
    let err = apply(&mut desk).expect_err("an unsigned candidate on a console");
    assert!(
        err.contains("UNSIGNED"),
        "the refusal did not say why: {err}"
    );
    assert!(
        desk.state.audit.entries().is_empty(),
        "a refused apply left an audit entry"
    );

    let fingerprint = sign(&desk);
    apply(&mut desk).expect("the signed candidate applies");
    let entry = desk.state.audit.entries().last().cloned().expect("audited");
    assert_eq!(entry.action, gungnir_security::actions::APPLY_CONFIG);
    assert!(
        entry
            .detail
            .contains(&format!("signed by baseline key {fingerprint}")),
        "the entry did not name the signer: {}",
        entry.detail
    );

    // An edit after signing breaks the signature: the console refuses it again.
    let path = desk.path.clone();
    let mut edited: ConfigBaseline =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("parses");
    edited.revision = 3;
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&edited).expect("encodes"),
    )
    .expect("written");
    let err = apply(&mut desk).expect_err("a changed candidate on a console");
    assert!(err.contains("does NOT verify"), "{err}");
}

/// A desktop deployed on its own applies the unsigned candidate, as DN-22 §15 says, and
/// both the audit entry and an alert say it was unsigned.
#[test]
fn a_disconnected_desktop_applies_an_unsigned_baseline_and_says_so() {
    let mut desk = desk("alone", false);
    apply(&mut desk).expect("a disconnected desktop applies it");
    let entry = desk.state.audit.entries().last().cloned().expect("audited");
    assert!(entry.detail.contains("UNSIGNED"), "{}", entry.detail);
    assert!(
        desk.state.alerts.iter().any(|a| a.contains("UNSIGNED")),
        "{:?}",
        desk.state.alerts
    );
}

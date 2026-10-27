// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! GAP-208, D-142: the desktop's audit anchors signed under a key the operator's session
//! does not hold, and its audit file checked against the heads the node holds for it
//! (plan 12 tranche B).
//!
//! The case the review found: the operator who can edit the audit file can also reseal the
//! journal that holds its head, because the journal's key is unlocked from that operator's
//! own keystore. Two answers, both tested here. The anchors are signed by a key whose
//! passphrase the administrator holds, so a head written by anybody else does not verify.
//! And the heads go to the node when linked, so a file cut or rewritten after its head
//! reached the node is found on reconnect, whatever the desktop's journal now says.

use gungnir_app::audit_record;
use gungnir_app::state::AppState;
use gungnir_config::{AuditSigningConfig, ConfigBaseline};
use gungnir_eventing::Event;
use gungnir_model::events::AuditEvent;
use gungnir_security::audit::events;
use gungnir_security::audit::signing::{AuditSigner, AuditVerifyingKey, SignatureCheck};
use gungnir_security::{AuditLog, ANCHOR_INTERVAL};
use std::path::{Path, PathBuf};
use std::time::Instant;

const ADMIN: &str = "the administrator's own passphrase";

fn data_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gungnir-app-audit-signing-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// A desktop whose baseline names the audit-signing key provisioned in `dir`.
fn desktop(dir: &Path, pem: Option<String>) -> AppState {
    let mut config = ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    };
    config.security.audit_signing = pem.map(|public_key_pem| AuditSigningConfig { public_key_pem });
    gungnir_config::validate(&config).expect("valid");
    let mut state = AppState::with_config(config).expect("the desktop starts");
    gungnir_app::update::tick(&mut state);
    state
}

fn acts(state: &mut AppState, n: usize) {
    for i in 0..n {
        gungnir_app::audit::record(state, "plan.decide", format!("act {i}"));
    }
}

/// The audit events the bus carried since `rx` subscribed.
fn audit_events(rx: &gungnir_eventing::Receiver<gungnir_eventing::Envelope>) -> Vec<AuditEvent> {
    rx.try_iter()
        .filter_map(|e| match e.event {
            Event::Audit(a) => Some(a),
            _ => None,
        })
        .collect()
}

/// Locked at start and said so; refused for a wrong passphrase, on the record; unlocked by
/// the administrator's; and from then on every anchor verifies under the baseline's key.
#[test]
fn anchors_are_signed_once_an_administrator_unlocks_the_key() {
    let dir = data_dir("unlock");
    let pem = AuditSigner::provision(&dir, ADMIN).expect("provisioned");
    let key = AuditVerifyingKey::from_pem(&pem).expect("a key");
    let mut state = desktop(&dir, Some(pem));
    assert!(state.audit_key.is_some() && state.audit_signer.is_none());
    assert!(
        state
            .alerts
            .iter()
            .any(|a| a.contains("Audit anchors are unsigned")),
        "{:?}",
        state.alerts
    );
    let text = audit_record::record_text(&state);
    assert!(
        text.signing_locked && text.signing.contains("UNSIGNED"),
        "{}",
        text.signing
    );

    audit_record::unlock_signing(&mut state, "the operator's guess at it");
    assert!(state.audit_signer.is_none());
    assert!(state
        .alerts
        .iter()
        .any(|a| a.contains("Audit signing was not unlocked")));

    audit_record::unlock_signing(&mut state, ADMIN);
    assert!(state.audit_signer.is_some(), "{:?}", state.alerts);
    assert!(!audit_record::record_text(&state).signing_locked);

    let rx = state.events.subscribe();
    acts(&mut state, 3);
    audit_record::tick_at(&mut state, Instant::now() + ANCHOR_INTERVAL);
    let anchored: Vec<_> = audit_events(&rx)
        .into_iter()
        .filter_map(|e| match e {
            AuditEvent::Anchored { head, .. } => Some(head),
            _ => None,
        })
        .collect();
    assert_eq!(anchored.len(), 1, "{anchored:?}");
    let head = &anchored[0];
    let signature = head.signature.as_ref().expect("signed");
    assert_eq!(
        key.check(
            &audit_record::from_model(head),
            &signature.der().expect("hex")
        ),
        SignatureCheck::Valid
    );

    // Both unlock attempts are on the record, the refusal among them.
    state.audit.flush().expect("flushed");
    let segment = state.audit.segment().expect("a segment").to_path_buf();
    let text = std::fs::read_to_string(segment).expect("read");
    let attempts: Vec<&str> = text
        .lines()
        .filter(|l| l.contains(events::SIGNING_UNLOCK))
        .collect();
    assert_eq!(attempts.len(), 2, "{attempts:?}");
    assert!(attempts[0].contains("refused"));
    let _ = std::fs::remove_dir_all(dir);
}

/// A keystore that opens but holds another deployment's key is refused: its anchors would
/// verify nowhere.
#[test]
fn a_key_that_is_not_the_baselines_is_refused() {
    let dir = data_dir("mismatch");
    let other = data_dir("mismatch-other");
    let _ = AuditSigner::provision(&dir, ADMIN).expect("ours");
    let theirs = AuditSigner::provision(&other, ADMIN).expect("theirs");
    let mut state = desktop(&dir, Some(theirs));
    audit_record::unlock_signing(&mut state, ADMIN);
    assert!(state.audit_signer.is_none());
    assert!(
        state
            .alerts
            .iter()
            .any(|a| a.contains("not the one the baseline names")),
        "{:?}",
        state.alerts
    );
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(other);
}

/// **The tranche B criterion.** A head that reached the node is held there; the file is
/// cut after it, mid-run, past every check the desktop itself would make before its next
/// start; and on reconnect the file is checked against the node's head, the cut is found
/// and said, and the finding goes to the node with the heads.
#[test]
fn a_cut_audit_file_is_found_against_the_nodes_heads_on_reconnect() {
    let dir = data_dir("reconnect");
    let mut state = desktop(&dir, None);
    let link = gungnir_remote::link::NodeLink::scripted();
    state.link = Some(link.clone());
    acts(&mut state, 6);
    audit_record::tick_at(&mut state, Instant::now() + ANCHOR_INTERVAL);
    audit_record::forward(&mut state);
    // What the node was sent, and so holds.
    let sent: Vec<_> = link
        .read()
        .expect("projection")
        .audit_anchor_outbox
        .drain(..)
        .flat_map(|b| b.heads)
        .collect();
    let held = sent
        .iter()
        .filter(|h| h.entries == 6)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(held.len(), 1, "the anchor went to the node: {sent:?}");

    // Somebody able to write the file removes the last two entries.
    state.audit.flush().expect("flushed");
    let path = state.audit.segment().expect("a segment").to_path_buf();
    let text = std::fs::read_to_string(&path).expect("read");
    let kept: Vec<&str> = text.lines().take(4).collect();
    std::fs::write(&path, kept.join("\n") + "\n").expect("cut");

    // The link reconnects and reads the node's heads for this desktop.
    link.read().expect("projection").node_audit_heads = Some(held);
    audit_record::forward(&mut state);
    assert!(
        state
            .alerts
            .iter()
            .any(|a| a.contains("does not reach the node's head") && a.contains("cut")),
        "{:?}",
        state.alerts
    );
    let reported: Vec<_> = link
        .read()
        .expect("projection")
        .audit_anchor_outbox
        .iter()
        .filter_map(|b| b.findings.clone())
        .collect();
    assert_eq!(reported.len(), 1, "the finding goes to the node");
    assert!(reported[0]
        .iter()
        .any(|f| f.contains("2 entries are missing")));
    assert!(audit_record::record_text(&state)
        .signing
        .contains("Checked against the node's heads on reconnect: 1 problem(s)"));
    drop(state);
    let _ = std::fs::remove_dir_all(dir);
}

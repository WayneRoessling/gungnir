// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The desktop's half of signed baselines (GAP-210; DN-22 §15).
//!
//! The rule is `gungnir_security::baseline_signing`'s and human-owned; what is here is what
//! only this binary can say: where its baseline came from, whether it runs connected, and
//! `baseline sign`, which needs the baseline's schema. The node has the same few lines in
//! `gungnir_node::baseline`, because a binary may not depend on a binary.
//!
//! **Connected** is a desktop on the Remote backend: a node's console, whose baseline a
//! deployment controls. A desktop deployed on its own warns and runs, as the gap says,
//! whatever its baseline's signature says.

use std::path::Path;

use gungnir_config::{BackendConfig, BaselineSignature, ConfigBaseline, ConfigStore};
use gungnir_security::baseline_signing::{BaselineTrust, BaselineVerifyingKey, PresentedSignature};

/// Whether this baseline runs the desktop as a node's console.
#[must_use]
pub fn connected(config: &ConfigBaseline) -> bool {
    matches!(config.backend, BackendConfig::Remote { .. })
}

/// What the signature beside the baseline at `path` says about `baseline`, against `key`.
///
/// A `.sig` that does not read is a signature that does not verify, never an absent one.
#[must_use]
pub fn trust_of(
    path: &Path,
    baseline: &ConfigBaseline,
    key: &Result<Option<BaselineVerifyingKey>, String>,
) -> BaselineTrust {
    let bytes = match baseline.signing_bytes() {
        Ok(bytes) => bytes,
        Err(e) => return BaselineTrust::BadSignature(e.to_string()),
    };
    match BaselineSignature::read_beside(path) {
        Ok(signature) => {
            let presented = signature.map(|s| PresentedSignature {
                key_version: s.key_version,
                key_fingerprint: s.key_fingerprint.clone(),
                der: s.der().unwrap_or_default(),
            });
            BaselineTrust::judge(key, &bytes, presented.as_ref())
        }
        Err(e) => match key {
            Err(why) => BaselineTrust::KeyUnreadable(why.clone()),
            Ok(None) => BaselineTrust::NoKeyPinned,
            Ok(Some(_)) => {
                BaselineTrust::BadSignature(format!("the signature beside it does not read: {e}"))
            }
        },
    }
}

/// `baseline sign <baseline.json> <keystore-dir>`: check the baseline as this build would
/// load it, sign it, and write the `.sig` beside it.
///
/// # Errors
///
/// The usage, a baseline that does not load or validate, or a key that does not sign.
pub fn sign(binary: &str, args: &[String]) -> Result<String, String> {
    let usage = || gungnir_security::provisioning::baseline_usage(binary);
    let (Some("sign"), Some(path), Some(keys)) =
        (args.first().map(String::as_str), args.get(1), args.get(2))
    else {
        return Err(usage());
    };
    let store = gungnir_config::FileConfigStore::new(path, crate::state::known_vocabulary());
    let baseline = store.load().map_err(|e| format!("{path}: {e}"))?;
    store
        .validate(&baseline)
        .map_err(|e| format!("{path} does not validate, so no signature was made: {e}"))?;
    let bytes = baseline.signing_bytes().map_err(|e| e.to_string())?;
    let made = gungnir_security::provisioning::sign_baseline_bytes(binary, Path::new(keys), &bytes)
        .map_err(|e| e.to_string())?;
    let signature =
        BaselineSignature::from_der(made.key_version, made.key_fingerprint.clone(), &made.der);
    signature
        .write_beside(Path::new(path))
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "signed {path} (revision {}) with baseline key {} version {}; wrote {}",
        baseline.revision,
        made.key_fingerprint,
        made.key_version,
        BaselineSignature::path_for(Path::new(path)).display()
    ))
}

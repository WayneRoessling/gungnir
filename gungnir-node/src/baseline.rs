// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The node's half of signed baselines (GAP-210; DN-22 §15).
//!
//! The rule is `gungnir_security::baseline_signing`'s and human-owned. A node is always a
//! connected profile, so with a key pinned it **refuses to start** on a baseline that key
//! did not sign. `gungnir_app::baseline` holds the desktop's copy of these few lines,
//! because a binary may not depend on a binary.

use std::path::Path;

use gungnir_config::{
    BaselineSignature, ConfigBaseline, ConfigError, ConfigStore, FileConfigStore, KnownVocabulary,
};
use gungnir_security::baseline_signing::{BaselineTrust, BaselineVerifyingKey, PresentedSignature};

/// The canonical action and role names, which `gungnir-config` may not read itself
/// (GAP-052, DN-08 §6 rule 3).
#[must_use]
pub fn known_vocabulary() -> KnownVocabulary {
    KnownVocabulary::new(
        gungnir_security::actions::ALL.iter().copied(),
        gungnir_security::Role::ALL.iter().map(|r| format!("{r:?}")),
    )
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
                der: s.der().unwrap_or_default(),
                key_fingerprint: s.key_fingerprint,
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

/// Load, validate and admit the baseline at `path` as a node runs it.
///
/// # Errors
///
/// A baseline that does not load or validate, and one [`BaselineTrust::admit`] refuses
/// in a connected profile, as [`ConfigError::Invalid`] naming why.
pub fn load(
    path: &str,
    key: &Result<Option<BaselineVerifyingKey>, String>,
) -> Result<(ConfigBaseline, BaselineTrust), ConfigError> {
    let store = FileConfigStore::new(path, known_vocabulary());
    let baseline = store.load()?;
    store.validate(&baseline)?;
    let trust = trust_of(Path::new(path), &baseline, key);
    match trust.admit(true) {
        Ok(None) => {
            tracing::info!(path, trust = %trust.describe(), "loaded config baseline");
        }
        Ok(Some(warning)) => tracing::warn!(path, "loaded config baseline: {warning}"),
        Err(refusal) => {
            return Err(ConfigError::Invalid(format!(
                "{path} is not run: {refusal}"
            )))
        }
    }
    Ok((baseline, trust))
}

/// `baseline sign <baseline.json> <keystore-dir>`: check the baseline as this build would
/// load it, sign it, and write the `.sig` beside it.
///
/// # Errors
///
/// The usage, a baseline that does not load or validate, or a key that does not sign.
pub fn sign(binary: &str, args: &[String]) -> Result<String, String> {
    let (Some("sign"), Some(path), Some(keys)) =
        (args.first().map(String::as_str), args.get(1), args.get(2))
    else {
        return Err(gungnir_security::provisioning::baseline_usage(binary));
    };
    let store = FileConfigStore::new(path, known_vocabulary());
    let baseline = store.load().map_err(|e| format!("{path}: {e}"))?;
    store
        .validate(&baseline)
        .map_err(|e| format!("{path} does not validate, so no signature was made: {e}"))?;
    let bytes = baseline.signing_bytes().map_err(|e| e.to_string())?;
    let made = gungnir_security::provisioning::sign_baseline_bytes(binary, Path::new(keys), &bytes)
        .map_err(|e| e.to_string())?;
    BaselineSignature::from_der(made.key_version, made.key_fingerprint.clone(), &made.der)
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

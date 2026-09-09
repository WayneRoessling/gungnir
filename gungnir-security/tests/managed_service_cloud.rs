// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The two real cloud key services (DN-22 amendment 5, §14; D-42; GAP-084).
//!
//! **These tests are `#[ignore]`d, and that is a statement about this workspace rather
//! than about the code.** Nothing in this repository can reach an AWS KMS or an Azure Key
//! Vault: there is no account, no credential and no network path to one, and a test that
//! pretended otherwise -- by mocking an HTTP endpoint and calling the result a cloud
//! round trip -- would be the exact failure `CLAUDE.md` forbids. So the honest split is
//! the one below.
//!
//! **What is covered without credentials**, and runs in the normal suite:
//! `gungnir-security/src/managed_service.rs`'s own tests against a fake implementing
//! `CloudKeyService`. That is where the design's claims actually live -- that `seal`
//! makes no call, that a restart reopens the same keystore, that an unreachable service
//! yields a stated unencrypted condition, that sealing survives an outage and signing
//! does not, that rotation is local, that escrow still works. None of those depend on
//! which cloud is underneath.
//!
//! **What is covered here without credentials**: that both services construct against
//! the real SDKs, that their `describe` says what a health line needs and leaks nothing,
//! and that a client which cannot be built at all refuses every queued call instead of
//! leaving its caller blocked -- the one path the in-crate fake cannot reach, because the
//! fake is not behind the worker thread. Both run in the normal suite and are together
//! the compile-and-construct half of amendment 5 (i)'s "compile-verified".
//!
//! **What needs real credentials**, and is `#[ignore]`d until a deployment has them:
//! whether a real service's wrapped-blob format, error text, latency and credential
//! chain behave as the fake models. Run with, for AWS:
//!
//! ```text
//! GUNGNIR_TEST_AWS_REGION=eu-west-2 \
//! GUNGNIR_TEST_AWS_KEY_ID=arn:aws:kms:eu-west-2:123456789012:key/... \
//!   cargo test -p gungnir-security --test managed_service_cloud -- --ignored
//! ```
//!
//! and for Azure:
//!
//! ```text
//! GUNGNIR_TEST_AZURE_VAULT=https://a-vault.vault.azure.net \
//! GUNGNIR_TEST_AZURE_KEY=gungnir-journal \
//!   cargo test -p gungnir-security --test managed_service_cloud -- --ignored
//! ```
//!
//! Neither variable is a credential and neither may become one: the credential comes from
//! the environment the process runs in (an IAM role, a managed identity), which is DN-22
//! §6's rule and the whole reason `aws-config` and `azure_identity` are in the stack.

use gungnir_security::{
    AwsKmsKeyService, AzureKeyVaultKeyService, CloudKeyService, KeyProvider, KeyPurpose,
    ManagedServiceKeyProvider, SignatureScheme,
};
use std::sync::Arc;

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("gungnir-cloud-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("dir");
    d
}

/// Both services build, describe themselves without leaking anything, and report a
/// failure as a value rather than panicking or hanging when they cannot be built at all.
///
/// **Not `#[ignore]`d**, because none of it needs an account, and it is the
/// compile-and-construct half of amendment 5 (i)'s "compile-verified".
///
/// **No network call is made here, deliberately.** An earlier draft pointed the Azure
/// client at an unresolvable `.invalid` host and called `wrap`; it passed, and took 82
/// seconds, because the SDK retried with backoff before giving up. A suite that slow at
/// one assertion is a suite people stop running. The unreachable-*service* path is
/// covered instead where it belongs -- `managed_service.rs`'s own tests, against a fake
/// that can be switched offline in a nanosecond -- and what is covered here is the one
/// thing that fake cannot reach: the worker thread's own construction-failure path.
#[test]
fn construction_does_not_require_a_reachable_service() {
    let aws = AwsKmsKeyService::new("eu-west-2", "alias/gungnir-journal-test");
    assert!(aws.describe().contains("eu-west-2"), "{}", aws.describe());
    assert!(
        aws.describe().contains("alias/gungnir-journal-test"),
        "the health line names the key the baseline pointed at: {}",
        aws.describe()
    );

    let azure = AzureKeyVaultKeyService::new("https://gungnir-test.vault.invalid", "journal");
    assert!(azure.describe().contains("journal"), "{}", azure.describe());
    assert!(
        !format!("{azure:?}").contains("credential"),
        "the service's Debug must not reach for anything secret: {azure:?}"
    );
}

/// The worker thread's construction-failure path: a client that cannot be built must
/// answer every queued call with the reason, not leave the caller blocked on a channel
/// nobody will reply to.
///
/// A vault URL whose scheme is not `http` is refused by `KeyClient::new` **before any
/// network call**, which makes this deterministic and instant -- exactly the property the
/// `.invalid` host did not have.
#[test]
fn a_client_that_cannot_be_built_refuses_every_call_rather_than_hanging() {
    let service = AzureKeyVaultKeyService::new("ftp://not-a-vault.example", "journal");
    let err = service
        .wrap(b"a data key")
        .expect_err("a client that was never built must not appear to succeed");
    let reason = err.to_string();
    assert!(
        reason.contains("not-a-vault.example"),
        "the operator must be told which service failed: {reason}"
    );
    // And a second call gets the same treatment rather than blocking for ever, which is
    // the property `refuse_every_call` exists for: it keeps draining the queue.
    assert!(
        service.unwrap_key(b"anything").is_err(),
        "the second call must be refused too"
    );
}

/// A real AWS KMS round trip. **Never run in this workspace; no such call has been made.**
#[test]
#[ignore = "needs real AWS credentials and a KMS key; none exist in this workspace"]
fn aws_kms_wraps_unwraps_and_signs_for_real() {
    let (Ok(region), Ok(key_id)) = (
        std::env::var("GUNGNIR_TEST_AWS_REGION"),
        std::env::var("GUNGNIR_TEST_AWS_KEY_ID"),
    ) else {
        panic!("set GUNGNIR_TEST_AWS_REGION and GUNGNIR_TEST_AWS_KEY_ID")
    };
    let service = AwsKmsKeyService::new(&region, &key_id);

    let secret = b"a thirty-two byte data key......";
    let wrapped = service.wrap(secret).expect("KMS wrapped the data key");
    assert_ne!(wrapped, secret, "the data key was returned unwrapped");
    assert_eq!(
        service.unwrap_key(&wrapped).expect("KMS unwrapped it"),
        secret,
        "the round trip did not return what went in"
    );

    // A signing key is a different KMS key from a wrapping one, so this half only runs
    // where the configured key is asymmetric; a symmetric key refusing is the correct
    // answer and is reported rather than failed.
    match service.sign(b"a handshake transcript", SignatureScheme::EcdsaP256Sha256) {
        Ok(signature) => assert!(!signature.is_empty(), "an empty signature"),
        Err(err) => eprintln!("the configured key does not sign, which is legitimate: {err}"),
    }
}

/// A real Azure Key Vault round trip. **Never run in this workspace; no such call has
/// been made.**
#[test]
#[ignore = "needs a real Azure managed identity and Key Vault; none exist in this workspace"]
fn azure_key_vault_wraps_unwraps_and_signs_for_real() {
    let (Ok(vault), Ok(key)) = (
        std::env::var("GUNGNIR_TEST_AZURE_VAULT"),
        std::env::var("GUNGNIR_TEST_AZURE_KEY"),
    ) else {
        panic!("set GUNGNIR_TEST_AZURE_VAULT and GUNGNIR_TEST_AZURE_KEY")
    };
    let service = AzureKeyVaultKeyService::new(&vault, &key);

    let secret = b"a thirty-two byte data key......";
    let wrapped = service
        .wrap(secret)
        .expect("the vault wrapped the data key");
    assert_ne!(wrapped, secret, "the data key was returned unwrapped");
    assert_eq!(
        service
            .unwrap_key(&wrapped)
            .expect("the vault unwrapped it"),
        secret,
        "the round trip did not return what went in"
    );

    match service.sign(b"a handshake transcript", SignatureScheme::EcdsaP256Sha256) {
        Ok(signature) => assert!(!signature.is_empty(), "an empty signature"),
        Err(err) => eprintln!("the configured key does not sign, which is legitimate: {err}"),
    }
}

/// The whole profile against a real service: open a keystore, seal, restart, and read
/// back what the first run wrote. **Never run in this workspace.**
#[test]
#[ignore = "needs real cloud credentials; none exist in this workspace"]
fn the_profile_survives_a_restart_against_a_real_service() {
    let service: Arc<dyn CloudKeyService> = if let (Ok(region), Ok(key_id)) = (
        std::env::var("GUNGNIR_TEST_AWS_REGION"),
        std::env::var("GUNGNIR_TEST_AWS_KEY_ID"),
    ) {
        Arc::new(AwsKmsKeyService::new(&region, &key_id))
    } else if let (Ok(vault), Ok(key)) = (
        std::env::var("GUNGNIR_TEST_AZURE_VAULT"),
        std::env::var("GUNGNIR_TEST_AZURE_KEY"),
    ) {
        Arc::new(AzureKeyVaultKeyService::new(&vault, &key))
    } else {
        panic!("set either the AWS or the Azure pair of variables")
    };

    let d = dir("restart");
    let sealed = {
        let provider = ManagedServiceKeyProvider::open_or_create(&d, Arc::clone(&service), None)
            .expect("open");
        let key = provider
            .active_or_generate(KeyPurpose::JournalAtRest)
            .expect("key");
        provider
            .seal(&key, b"written before the restart")
            .expect("sealed")
    };
    let provider = ManagedServiceKeyProvider::open_or_create(&d, service, None).expect("reopened");
    let key = provider
        .active(KeyPurpose::JournalAtRest)
        .expect("same key");
    assert_eq!(
        provider.unseal(&key, &sealed).expect("still opens"),
        b"written before the restart"
    );
    let _ = std::fs::remove_dir_all(d);
}

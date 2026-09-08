// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A host's TLS identity, issued from its own key provider (GAP-060, D-29; DN-22
//! amendment 1).
//!
//! # The invariant, which is the point of the whole module
//!
//! **The private half never leaves custody.** `rcgen` builds and signs the certificate
//! through its `SigningKey` trait, which calls the provider's `sign`; rustls then serves
//! or presents it through a `SigningKey` of its own over the same call. What leaves the
//! provider is the public half and the finished certificate. No code path here can
//! produce a certificate over key material that has left a `KeyProvider`, and that -- not
//! the absence of a certificate generator -- is the property worth protecting.
//!
//! # Why this lives here and not in either binary
//!
//! It was in `gungnir-node` until 2026-09-06, and §2.9 recorded that `rcgen` shipped in
//! that binary and nowhere else. The desktop needs the same thing (GAP-060): its client
//! certificate is read from the environment today, which both the code and the register
//! call a development fallback. Two binaries cannot depend on each other, so leaving the
//! code where it was meant duplicating about 150 lines and letting the copies drift.
//!
//! `gungnir-remote` is where it went because it is the only crate **both binaries already
//! depend on at runtime** that already carries `rustls` (PEM reading included) and
//! `tokio-rustls`, and because it already owns [`crate::LinkTls`] -- the type that
//! answers "who is this host". `gungnir-api` would have been a better fit still, but the
//! desktop holds it as a dev-dependency only and `ARCHITECTURE.md` refuses that as a
//! production edge.
//!
//! # Persistent identities (2026-09-08, GAP-060's remaining slice; human-owned per
//! `docs/agentic-workflow.md` -- written and gated, not signed)
//!
//! [`issue_for_client`] and [`issue`] build a fresh ephemeral `P256KeyProvider` (the
//! node's serving identity in `gungnir-node/src/main.rs::spawn_tls_from_provider`) or
//! take one already built (`issue`, called with an ephemeral one by
//! `issue_for_client`): either way, a new identity every process start. Now that D-39
//! admits an OS-keystore crate and `gungnir_security::PersistentKeyProvider::
//! open_or_create_via_os_keystore` takes `service` as a parameter rather than baking in
//! the desktop's own name, [`issue_node_serving_identity`] and
//! [`issue_desktop_outbound_identity`] issue the same two identities from a persistent
//! provider backed by it instead, so each survives a restart.
//!
//! **Two identities, two service names, never one decision away from colliding.**
//! `NODE_TLS_IDENTITY_SERVICE` and `DESKTOP_TLS_IDENTITY_SERVICE` are as distinct from
//! each other as they are from `gungnir-node-accounts` (GAP-057) and
//! `gungnir-desktop-keystore` (GAP-084/D-39) -- four purposes, four names, one
//! mechanism. **What this deliberately does not do**: the node's own outbound
//! (peer-link) identity, `host_tls`'s call to [`issue_for_client`], is untouched and
//! stays ephemeral. Whether that identity should ever persist too, and whether it
//! should then be the *same* identity as the node's serving one or a third, separately
//! named entry, is real design surface the register leaves open; this module answers
//! neither question, because ARCHITECTURE.md item 105 already named it as deliberately
//! not taken and it is not this change's to decide either.
//!
//! **Honest either way, never a silent downgrade.** Each persistent function falls
//! back to the same ephemeral issuance [`issue_for_client`] already used, logged as a
//! fallback rather than left to look like the persistent path succeeded -- DN-22 §5's
//! disconnected-fallback rule ("an unavailable keystore yields an honest unencrypted
//! state ... never a claimed-but-absent encryption") applied to a TLS identity instead
//! of to journal encryption. A caller only ever sees an error when *both* the
//! persistent and the ephemeral path fail, which for the ephemeral path means `rcgen`
//! itself refused the names -- the same condition that already made [`issue_for_client`]
//! fail before this module could try a keystore at all.
//!
//! **Whether to attempt the persistent path at all is the caller's decision, not this
//! module's.** [`issue_node_serving_identity`] and [`issue_desktop_outbound_identity`]
//! always try; `gungnir-app/src/session.rs::link_tls_for` only calls the desktop one
//! when `security.key_provider` is already `OperatingSystemKeystore`, and calls
//! [`issue_for_client`] directly otherwise. That gate exists because attempting the
//! persistent path does real disk and operating-system-keystore I/O where the
//! ephemeral path is purely in memory, and `link_tls_for` runs on every `AppState`
//! built (every peer link, every reconnect) -- unconditionally attempting it would
//! touch the real keystore, and leave an entry in it, for every desktop and every test
//! that never asked for persistence, `KeyProviderConfig::None` (the default) among
//! them. `spawn_tls_from_provider` (the node's serving identity) has no equivalent
//! deployment-wide switch to read and is reached only along a narrow, already
//! deliberately-configured path, so it carries no such gate.

use std::path::Path;
use std::sync::Arc;

use gungnir_security::{KeyId, KeyProvider, KeyPurpose, SignatureScheme};

/// The provider seen as a signing key by rcgen and by rustls.
pub struct ProviderKey<P: KeyProvider> {
    provider: Arc<P>,
    key: KeyId,
    /// The public half as `SubjectPublicKeyInfo` DER.
    spki: Vec<u8>,
}

impl<P: KeyProvider> std::fmt::Debug for ProviderKey<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderKey")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

impl<P: KeyProvider> ProviderKey<P> {
    pub fn new(provider: Arc<P>, key: KeyId, spki_der: Vec<u8>) -> Self {
        Self {
            provider,
            key,
            spki: spki_der,
        }
    }
}

impl<P: KeyProvider> rcgen::PublicKeyData for ProviderKey<P> {
    fn der_bytes(&self) -> &[u8] {
        // rcgen wants the raw public key bytes and wraps them in the SPKI itself; for
        // P-256 that is the uncompressed point inside the SPKI's BIT STRING, which is the
        // last 65 bytes of the DER.
        let n = self.spki.len();
        if n >= 65 {
            &self.spki[n - 65..]
        } else {
            &self.spki
        }
    }

    fn algorithm(&self) -> &'static rcgen::SignatureAlgorithm {
        &rcgen::PKCS_ECDSA_P256_SHA256
    }
}

impl<P: KeyProvider> rcgen::SigningKey for ProviderKey<P> {
    fn sign(&self, msg: &[u8]) -> Result<Vec<u8>, rcgen::Error> {
        self.provider
            .sign(&self.key, msg, SignatureScheme::EcdsaP256Sha256)
            .map_err(|e| {
                tracing::error!(%e, "the key provider refused to sign the certificate");
                rcgen::Error::RemoteKeyError
            })
    }
}

/// rustls's view: one scheme, ECDSA P-256 with SHA-256.
impl<P: KeyProvider + 'static> rustls::sign::SigningKey for ProviderKey<P> {
    fn choose_scheme(
        &self,
        offered: &[rustls::SignatureScheme],
    ) -> Option<Box<dyn rustls::sign::Signer>> {
        offered
            .contains(&rustls::SignatureScheme::ECDSA_NISTP256_SHA256)
            .then(|| {
                Box::new(ProviderSigner {
                    provider: Arc::clone(&self.provider),
                    key: self.key,
                }) as Box<dyn rustls::sign::Signer>
            })
    }

    fn public_key(&self) -> Option<rustls::pki_types::SubjectPublicKeyInfoDer<'_>> {
        Some(rustls::pki_types::SubjectPublicKeyInfoDer::from(
            self.spki.as_slice(),
        ))
    }

    fn algorithm(&self) -> rustls::SignatureAlgorithm {
        rustls::SignatureAlgorithm::ECDSA
    }
}

struct ProviderSigner<P: KeyProvider> {
    provider: Arc<P>,
    key: KeyId,
}

impl<P: KeyProvider> std::fmt::Debug for ProviderSigner<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderSigner")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

impl<P: KeyProvider> rustls::sign::Signer for ProviderSigner<P> {
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, rustls::Error> {
        self.provider
            .sign(&self.key, message, SignatureScheme::EcdsaP256Sha256)
            .map_err(|e| rustls::Error::General(format!("the key provider refused to sign: {e}")))
    }

    fn scheme(&self) -> rustls::SignatureScheme {
        rustls::SignatureScheme::ECDSA_NISTP256_SHA256
    }
}

/// A self-signed certificate over the provider's transport key, and the rustls key
/// that presents it. Used by a node to serve and by a desktop to authenticate itself.
pub struct HostIdentity {
    pub certificate_der: Vec<u8>,
    pub certificate_pem: String,
    pub key: Arc<dyn rustls::sign::SigningKey>,
}

/// Issue the node's identity for `names` (DNS names or IP addresses the node is reached
/// as).
///
/// # Errors
///
/// When the provider holds no transport key, cannot report its public half, or refuses
/// to sign; or when rcgen rejects the names.
pub fn issue<P: KeyProvider + 'static>(
    provider: Arc<P>,
    key: KeyId,
    spki_der: Vec<u8>,
    names: Vec<String>,
    common_name: &str,
) -> Result<HostIdentity, String> {
    if key.purpose != KeyPurpose::TransportIdentity {
        return Err(format!("{key:?} is not a transport identity key"));
    }
    let signing = ProviderKey::new(provider, key, spki_der);
    let mut params = rcgen::CertificateParams::new(names).map_err(|e| e.to_string())?;
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, common_name);
    let certificate = params.self_signed(&signing).map_err(|e| e.to_string())?;
    Ok(HostIdentity {
        certificate_der: certificate.der().as_ref().to_vec(),
        certificate_pem: certificate.pem(),
        key: Arc::new(signing),
    })
}

/// Issue a client identity for this host from an ephemeral key provider, ready to put in
/// [`crate::LinkTls::issued`].
///
/// **The private half never exists outside the provider**, which is the whole reason this
/// returns a `CertifiedKey` and not a PEM: a PEM carrying a usable identity must contain
/// the private key, and that is the one thing custody exists to prevent. It is also why
/// `gungnir-app` calls this rather than building the type itself -- it would need `rustls`
/// to name it, and there is no reason for the desktop to grow a TLS dependency to hold a
/// value it only passes along.
///
/// **Ephemeral, and that is a stated limit rather than an oversight.** The key lives in
/// process memory, so this is a new identity every start until a persistent keystore
/// exists (GAP-084) -- exactly the position the node's serving identity is in, recorded in
/// the same words. A deployment pinning desktop certificates must pin again after a
/// restart. The alternative today is key material on a path in a baseline, which the
/// workspace rule forbids outright.
///
/// # Errors
///
/// When the provider cannot report its public half, or `rcgen` refuses the name.
pub fn issue_for_client(common_name: &str) -> Result<Arc<rustls::sign::CertifiedKey>, String> {
    Ok(as_certified_key(issue_ephemeral(
        vec!["localhost".to_owned()],
        common_name,
    )?))
}

/// A fresh identity from a new ephemeral in-process provider, over `names` -- the
/// logic [`issue_for_client`] always ran, factored out so
/// [`issue_persistent_or_ephemeral`] can fall back to exactly the same path rather
/// than a second copy of it.
fn issue_ephemeral(names: Vec<String>, common_name: &str) -> Result<HostIdentity, String> {
    use gungnir_security::{KeyPurpose, P256KeyProvider};
    let mut provider = P256KeyProvider::new();
    let key = provider.generate(KeyPurpose::TransportIdentity);
    let spki = provider
        .public_key_der(&key)
        .map_err(|e| format!("the key provider gave no public half: {e}"))?;
    issue(Arc::new(provider), key, spki, names, common_name)
}

/// [`HostIdentity`] wrapped as a `CertifiedKey`, ready for [`crate::LinkTls::issued`].
fn as_certified_key(identity: HostIdentity) -> Arc<rustls::sign::CertifiedKey> {
    Arc::new(rustls::sign::CertifiedKey::new(
        vec![rustls::pki_types::CertificateDer::from(
            identity.certificate_der,
        )],
        identity.key,
    ))
}

/// The name a node's TLS-identity keystore entries live under in the operating
/// system's keystore (D-39; GAP-060's remaining slice) -- distinct from
/// `gungnir-node-accounts` (GAP-057's node account store) and from
/// `DESKTOP_TLS_IDENTITY_SERVICE` below, so none of the three collide on one machine.
const NODE_TLS_IDENTITY_SERVICE: &str = "gungnir-node-tls-identity";

/// As [`NODE_TLS_IDENTITY_SERVICE`], for the desktop's own outbound identity --
/// distinct from `gungnir-desktop-keystore` (the desktop's journal-key custody,
/// GAP-084/D-39), which is a different key for a different purpose reached through the
/// identical mechanism.
const DESKTOP_TLS_IDENTITY_SERVICE: &str = "gungnir-desktop-tls-identity";

/// The subdirectory a persistent TLS-identity keystore lives in, under whatever data
/// directory the caller passes. `PersistentKeyProvider::open_or_create`'s file name
/// (`keystore.sealed`) is fixed, so two unrelated keystores cannot share one directory
/// without overwriting each other's file under two different wrapping keys; a
/// dedicated subdirectory keeps this one out of the way of
/// `KeyProviderConfig::OperatingSystemKeystore`'s own use of the same mechanism
/// directly against the data directory for a completely different key (the desktop's
/// journal key).
const TLS_IDENTITY_SUBDIR: &str = "tls-identity";

/// A stable operating-system-keystore account for `data_dir`'s deployment: its own
/// canonical path where one is obtainable, or the path exactly as given otherwise (for
/// instance, before the directory exists). Two deployments configured with different
/// data directories on one machine therefore never collide, the same property
/// `KeyProviderConfig::OperatingSystemKeystore`'s config-supplied `account` gives the
/// desktop's journal key -- derived here instead of asked for again, since nothing
/// about a TLS identity's account needs an operator's own choice the way an escrow
/// officer or a passphrase does.
///
/// **A stated limit, not a hidden one**: every deployment on one machine sharing
/// literally the same configured data directory still shares one persisted TLS
/// identity for that role, the same way they would already share one journal and one
/// `keystore.sealed`. Nothing in this workspace configures two deployments that way
/// today, and this module invents no new configuration to guard against it.
fn account_for(data_dir: &Path) -> String {
    std::fs::canonicalize(data_dir).map_or_else(
        |_| data_dir.display().to_string(),
        |p| p.display().to_string(),
    )
}

/// Open (or create) the persistent keystore backing a TLS identity: its own
/// subdirectory of `data_dir`, under `service`'s entry in the operating system's
/// keystore. No escrow -- escrow recovers sealed data for a party who lacks the key
/// that sealed it (DN-22 §11), and a signing key has nothing sealed to recover.
fn persistent_provider(
    data_dir: &Path,
    service: &str,
) -> Result<Arc<gungnir_security::PersistentKeyProvider>, String> {
    let keystore_dir = data_dir.join(TLS_IDENTITY_SUBDIR);
    std::fs::create_dir_all(&keystore_dir)
        .map_err(|e| format!("creating {}: {e}", keystore_dir.display()))?;
    let account = account_for(data_dir);
    let provider = gungnir_security::PersistentKeyProvider::open_or_create_via_os_keystore(
        &keystore_dir,
        service,
        &account,
        None,
    )
    .map_err(|e| e.to_string())?;
    Ok(Arc::new(provider))
}

/// Issue `names`' identity from a persistent, OS-keystore-backed provider under
/// `service` when one is reachable; fall back to a fresh ephemeral one, honestly
/// logged as a fallback, when it is not. See the module's own doc section for why this
/// never silently claims persistence it does not have.
fn issue_persistent_or_ephemeral(
    data_dir: &Path,
    service: &str,
    names: Vec<String>,
    common_name: &str,
) -> Result<HostIdentity, String> {
    let attempt = persistent_provider(data_dir, service).and_then(|provider| {
        let key = provider
            .active_or_generate(KeyPurpose::TransportIdentity)
            .map_err(|e| format!("no transport key: {e}"))?;
        let spki = provider
            .public_key_der(&key)
            .map_err(|e| format!("no public half: {e}"))?;
        issue(provider, key, spki, names.clone(), common_name)
    });
    match attempt {
        Ok(identity) => {
            tracing::info!(
                service,
                "this identity is persisted in the operating-system keystore and will \
                 survive a restart"
            );
            Ok(identity)
        }
        Err(err) => {
            tracing::warn!(
                service,
                %err,
                "could not issue this identity from the operating-system keystore; \
                 issuing an ephemeral one instead, which will not survive a restart"
            );
            issue_ephemeral(names, common_name)
        }
    }
}

/// The node's serving identity (`gungnir-node/src/main.rs::spawn_tls_from_provider`),
/// issued from a persistent, OS-keystore-backed provider so the certificate written
/// beside the journal for operators to pin (`node-identity.pem`) is the same
/// certificate after a restart -- falling back to a fresh ephemeral one, honestly, when
/// the keystore is unreachable (GAP-060's remaining slice; D-39).
///
/// **Not the node's peer-link (outbound) identity**, which [`issue_for_client`] still
/// issues ephemerally at every call in `gungnir-node/src/main.rs::host_tls`: see the
/// module doc section above for why this deliberately does not change that.
///
/// `data_dir` is the node's own data directory (`NodeConfig::data_dir`); `names` are
/// the DNS names or IP addresses this node is reached as.
///
/// # Errors
///
/// Only when the ephemeral fallback also fails -- `rcgen` refusing `names` -- since a
/// failure to reach the keystore itself is handled by falling back rather than
/// propagated.
pub fn issue_node_serving_identity(
    data_dir: &Path,
    names: Vec<String>,
    common_name: &str,
) -> Result<HostIdentity, String> {
    issue_persistent_or_ephemeral(data_dir, NODE_TLS_IDENTITY_SERVICE, names, common_name)
}

/// The desktop's outbound identity (`gungnir-app/src/session.rs::link_tls_for`),
/// issued from a persistent, OS-keystore-backed provider so the identity a node sees
/// from this desktop survives a restart -- falling back to a fresh ephemeral one,
/// honestly, when the keystore is unreachable (GAP-060's remaining slice; D-39).
///
/// `data_dir` is the desktop's own data directory (`ConfigBaseline::data_dir`).
///
/// # Errors
///
/// As [`issue_node_serving_identity`].
pub fn issue_desktop_outbound_identity(
    data_dir: &Path,
    common_name: &str,
) -> Result<Arc<rustls::sign::CertifiedKey>, String> {
    Ok(as_certified_key(issue_persistent_or_ephemeral(
        data_dir,
        DESKTOP_TLS_IDENTITY_SERVICE,
        vec!["localhost".to_owned()],
        common_name,
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_security::P256KeyProvider;
    use p256::ecdsa::signature::Verifier;

    #[test]
    fn the_identity_is_signed_by_the_provider_and_verifies_under_its_public_half() {
        let mut provider = P256KeyProvider::new();
        let key = provider.generate(KeyPurpose::TransportIdentity);
        let spki = provider.public_key_der(&key).expect("spki");
        let sec1 = provider.public_key_sec1(&key).expect("sec1");
        let provider = Arc::new(provider);
        let identity = issue(
            Arc::clone(&provider),
            key,
            spki,
            vec!["localhost".into()],
            "gungnir-node",
        )
        .expect("issued");
        assert!(identity.certificate_pem.contains("BEGIN CERTIFICATE"));
        assert!(!identity.certificate_der.is_empty());

        // rustls's signer signs through the provider, and the provider's public half
        // verifies it.
        let signer = rustls::sign::SigningKey::choose_scheme(
            identity.key.as_ref(),
            &[rustls::SignatureScheme::ECDSA_NISTP256_SHA256],
        )
        .expect("its one scheme");
        let signature = signer.sign(b"client hello").expect("signed");
        let verifier = p256::ecdsa::VerifyingKey::from_sec1_bytes(&sec1).expect("sec1");
        let parsed = p256::ecdsa::Signature::from_der(&signature).expect("der");
        verifier.verify(b"client hello", &parsed).expect("verifies");
        assert!(rustls::sign::SigningKey::choose_scheme(
            identity.key.as_ref(),
            &[rustls::SignatureScheme::ED25519]
        )
        .is_none());
    }

    #[test]
    fn a_journal_key_is_not_an_identity() {
        let mut provider = P256KeyProvider::new();
        let key = provider.generate(KeyPurpose::JournalAtRest);
        assert!(issue(
            Arc::new(provider),
            key,
            vec![0; 91],
            vec!["localhost".into()],
            "gungnir-node",
        )
        .is_err());
    }

    fn scratch_dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "gungnir-remote-identity-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("dir");
        d
    }

    /// `PersistentKeyProvider` is `issue()`'s second instantiation of `P: KeyProvider`
    /// (the first, `P256KeyProvider`, is what every test above already exercises) --
    /// this proves the generic bound holds for it too, and that the key it hands back
    /// is the *same* one across a reopen, which is the entire point of using it here
    /// rather than an ephemeral provider. Uses `open_or_create`'s plain-passphrase
    /// path, not the OS keystore: what this test checks is downstream of
    /// `wrapping_secret` producing a stable string, not that mechanism itself, which
    /// `gungnir-security`'s own tests already cover.
    #[test]
    fn a_persistent_provider_issues_the_same_identity_across_a_reopen() {
        use gungnir_security::PersistentKeyProvider;
        let dir = scratch_dir("persistent-reopen");

        let first =
            PersistentKeyProvider::open_or_create(&dir, "correct horse", None).expect("created");
        let key = first
            .active_or_generate(KeyPurpose::TransportIdentity)
            .expect("key");
        let spki = first.public_key_der(&key).expect("spki");
        let identity_one = issue(
            Arc::new(first),
            key,
            spki.clone(),
            vec!["localhost".into()],
            "gungnir-node",
        )
        .expect("issued once");
        assert!(identity_one.certificate_pem.contains("BEGIN CERTIFICATE"));

        // Reopened under the same passphrase: the same key comes back rather than a
        // fresh one, so a second issuance is over the same public half.
        let again =
            PersistentKeyProvider::open_or_create(&dir, "correct horse", None).expect("reopened");
        let key_again = again
            .active_or_generate(KeyPurpose::TransportIdentity)
            .expect("key");
        assert_eq!(key_again, key, "the same key, not a freshly generated one");
        let spki_again = again.public_key_der(&key_again).expect("spki");
        assert_eq!(spki_again, spki, "and so the same public half");
        let identity_two = issue(
            Arc::new(again),
            key_again,
            spki_again,
            vec!["localhost".into()],
            "gungnir-node",
        )
        .expect("issued again");
        assert!(identity_two.certificate_pem.contains("BEGIN CERTIFICATE"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The fallback DN-22 §5 requires: when the persistent path cannot even be opened,
    /// the node's serving identity is still issued -- ephemerally, honestly logged as
    /// such -- rather than the call failing outright. A file standing where a
    /// directory belongs forces `persistent_provider`'s `create_dir_all` to fail
    /// deterministically, on every platform, without depending on whether this
    /// machine happens to have a reachable operating-system keystore.
    #[test]
    fn issue_node_serving_identity_falls_back_to_an_ephemeral_identity_when_the_keystore_directory_cannot_be_created(
    ) {
        let path = std::env::temp_dir().join(format!(
            "gungnir-remote-identity-blocked-node-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, b"not a directory").expect("file standing in for a directory");

        let identity = issue_node_serving_identity(
            &path,
            vec!["127.0.0.1".into(), "localhost".into()],
            "gungnir-node",
        )
        .expect("the ephemeral fallback still issues");
        assert!(identity.certificate_pem.contains("BEGIN CERTIFICATE"));

        let _ = std::fs::remove_file(&path);
    }

    /// As the node's serving identity above, for the desktop's outbound one.
    #[test]
    fn issue_desktop_outbound_identity_falls_back_to_an_ephemeral_identity_when_the_keystore_directory_cannot_be_created(
    ) {
        let path = std::env::temp_dir().join(format!(
            "gungnir-remote-identity-blocked-desktop-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, b"not a directory").expect("file standing in for a directory");

        let certified = issue_desktop_outbound_identity(&path, "gungnir-app")
            .expect("the ephemeral fallback still issues");
        assert!(!certified.cert.is_empty());

        let _ = std::fs::remove_file(&path);
    }

    /// The two persisted identities never share a service name, so the same account
    /// (the same `data_dir`) addresses two independent operating-system-keystore
    /// entries rather than one.
    #[test]
    fn the_node_and_desktop_tls_identity_services_are_distinct_from_each_other_and_from_the_account_store_and_desktop_keystore(
    ) {
        let names = [
            NODE_TLS_IDENTITY_SERVICE,
            DESKTOP_TLS_IDENTITY_SERVICE,
            "gungnir-node-accounts",
            "gungnir-desktop-keystore",
        ];
        for (i, a) in names.iter().enumerate() {
            for b in &names[i + 1..] {
                assert_ne!(a, b, "two OS-keystore purposes must never share a name");
            }
        }
    }

    /// `persistent_provider` against whatever operating-system keystore this machine
    /// actually has (DN-22 §5; D-39) -- honest either way, the rule
    /// `gungnir-security/tests/os_keystore.rs` also follows. Unlike that crate's own
    /// tests, this one needs no separate binary: nothing in `gungnir-remote`'s test
    /// suite installs `keyring_core`'s mock store as the process default, so there is
    /// no race over which backend `keyring::v1::Entry` latches for this process.
    ///
    /// **No cleanup of the operating-system-keystore entry itself, and that is a
    /// stated trade-off.** Deleting it needs `keyring::v1::Entry` directly, the way
    /// `gungnir-security`'s own OS-keystore tests do; `gungnir-remote` depends on
    /// `gungnir-security`'s `PersistentKeyProvider`, which exposes no delete, and
    /// gaining a direct `keyring` dependency only for a test's teardown is not this
    /// change's to decide. A fixed directory name (not process-id-derived) is used
    /// instead, so repeated runs reuse and overwrite one entry rather than minting a
    /// new one to leave behind every time -- the directory itself is still removed.
    #[test]
    fn the_real_backend_round_trips_the_same_key_or_the_documented_fallback_fires() {
        let dir = std::env::temp_dir().join("gungnir-remote-identity-persistence-fixture-c176c2");
        let _ = std::fs::create_dir_all(&dir);

        match persistent_provider(&dir, NODE_TLS_IDENTITY_SERVICE) {
            Ok(first) => {
                let key = first
                    .active_or_generate(KeyPurpose::TransportIdentity)
                    .expect("key");
                let spki = first.public_key_der(&key).expect("spki");
                drop(first);

                // Reopened: the OS keystore handed back the same secret it stored the
                // first time, so the same key comes back rather than a fresh one.
                let again = persistent_provider(&dir, NODE_TLS_IDENTITY_SERVICE)
                    .expect("reopened under the same OS-held secret");
                let key_again = again
                    .active_or_generate(KeyPurpose::TransportIdentity)
                    .expect("key");
                assert_eq!(key_again, key, "the same key, not a freshly generated one");
                assert_eq!(
                    again.public_key_der(&key_again).expect("spki"),
                    spki,
                    "and so the same public half"
                );
            }
            Err(err) => {
                // DN-22 §5's fallback: refused honestly rather than treated as a
                // reason to invent a key. `issue_node_serving_identity` and
                // `issue_desktop_outbound_identity` recover from exactly this by
                // issuing ephemerally, covered deterministically by this module's own
                // fallback tests above without depending on this machine's backend.
                assert!(
                    err.contains("keystore") || err.contains("credential"),
                    "an unrelated failure, not the documented no-keystore fallback: {err}"
                );
            }
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}

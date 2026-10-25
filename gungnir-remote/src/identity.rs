// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A host's TLS identity, issued from its own key provider (GAP-060, D-29; DN-22
//! amendment 1).
//!
//! **Human-owned (the certificate and key-custody path, added to
//! `docs/agentic-workflow.md`'s low-trust list by name on 2026-09-06): the issuance code
//! below -- `ProviderKey`, `issue`, `issue_for_client`, the persistent issuers, and the
//! invariant they exist for** (signatures: docs/signatures.md).
//!
//! **What the review checked.** The invariant this module's own heading states --
//! `der_bytes()`'s slice of `spki_der` is what actually ends up embedded in the issued
//! certificate, not merely a value that signs correctly against a separately-obtained
//! key -- was not what the existing unit test proved, so a new one
//! (`the_certificates_own_embedded_key_is_the_true_one`) checks it directly. Every other
//! claim in this file's documentation was checked against the code it describes:
//! `ProviderKey::sign` and `ProviderSigner::sign` both go through `KeyProvider::sign`
//! alone, with no path that reads or returns the private scalar, which is what
//! `gungnir-app/tests/architecture_compliance.rs` also pins at the crate-surface level;
//! the ephemeral-vs-persistent fallback in the later section falls back honestly rather
//! than silently, per its own review.
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
//! `docs/agentic-workflow.md`; `ARCHITECTURE.md` §10 item 111)
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
//! (peer-link) identity stayed ephemeral, and whether it should persist, and as which
//! identity, was left open (record items 105 and 111).
//!
//! # Every identity its own, named by its key, limited to its role (D-135, 2026-10-25)
//!
//! The owner settled it. **The node's peer-link identity is its own persisted key**
//! ([`issue_node_peer_identity`], under `NODE_PEER_TLS_IDENTITY_SERVICE` in its own
//! subdirectory), so a peer pins it once and neither of the node's keys stands in for
//! the other. **Every node identity is named from its key**, as a desktop's has been
//! since GAP-141 ([`node_name`]: `node-<fingerprint>` serving, `node-peer-<fingerprint>`
//! outbound, `-ephemeral-` when the keystore is unreachable), where every node used to
//! call both of its certificates `gungnir-node`. **Every certificate states its role**
//! as its extended key usage ([`Usage`]): `serverAuth` on a serving one, `clientAuth` on
//! a desktop's or a node's outbound one. The node presents an operator's configured
//! `GUNGNIR_TLS_CERT`/`GUNGNIR_TLS_KEY` on its peer links in place of the issued one;
//! that precedence is the node's (`gungnir-node/src/main.rs::host_tls`), so a desktop's
//! link is unchanged.
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
//! module's.** [`issue_node_serving_identity`], [`issue_node_peer_identity`] and
//! [`issue_desktop_outbound_identity`] always try; `gungnir-app/src/session.rs::
//! link_tls_for` only calls the desktop one when `security.key_provider` is already
//! `OperatingSystemKeystore`, and `gungnir-node/src/main.rs::host_tls` applies the same
//! gate to the node's peer identity, issuing [`issue_node_peer_identity_ephemeral`]
//! otherwise. That gate exists because attempting the
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

/// How many hex digits of a desktop's key fingerprint its name carries.
///
/// Sixty-four bits of a SHA-256 over the public half. Two desktops collide by accident
/// at about one chance in four billion across a hundred thousand of them, which is a
/// different order of thing from the certainty of collision this replaces: every desktop
/// called itself `gungnir-app` (GAP-141). A node's names carry the same (D-135).
const DESKTOP_NAME_HEX: usize = 16;

/// What a certificate may be used for, stated on it as its extended key usage (D-135,
/// RFC 5280 §4.2.1.12).
///
/// **One role per certificate.** A serving certificate cannot authenticate a client and
/// an outbound one cannot serve, so a key taken from one role is no use in the other; a
/// verifier that checks the extension refuses the wrong one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Usage {
    /// A node serving the transport: `serverAuth`.
    Server,
    /// A desktop, or a node on its peer links, authenticating as a client: `clientAuth`.
    Client,
}

impl Usage {
    fn purpose(self) -> rcgen::ExtendedKeyUsagePurpose {
        match self {
            Usage::Server => rcgen::ExtendedKeyUsagePurpose::ServerAuth,
            Usage::Client => rcgen::ExtendedKeyUsagePurpose::ClientAuth,
        }
    }
}

/// A name derived from the public half of a key: `<prefix>-<fingerprint>`, or
/// `<prefix>-ephemeral-<fingerprint>` when the key does not survive a restart.
fn key_name(prefix: &str, spki_der: &[u8], persisted: bool) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(spki_der);
    let mut hex = String::with_capacity(DESKTOP_NAME_HEX);
    for byte in digest.iter().take(DESKTOP_NAME_HEX / 2) {
        use std::fmt::Write as _;
        // Writing to a String cannot fail; the result is discarded rather than unwrapped.
        let _ = write!(hex, "{byte:02x}");
    }
    if persisted {
        format!("{prefix}-{hex}")
    } else {
        format!("{prefix}-ephemeral-{hex}")
    }
}

/// Which of a node's two identities a name is for (D-135).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeRole {
    /// The identity the node serves the transport under: `node-<fingerprint>`.
    Serving,
    /// The identity the node presents on its peer links: `node-peer-<fingerprint>`.
    Peer,
}

/// What a node's identity is called: a name derived from its key, as a desktop's is
/// (D-135). Until D-135 every node's serving and outbound certificates were both named
/// `gungnir-node`, so a peer's `machine_identities` could not tell one node from another.
#[must_use]
pub fn node_name(spki_der: &[u8], role: NodeRole, persisted: bool) -> String {
    let prefix = match role {
        NodeRole::Serving => "node",
        NodeRole::Peer => "node-peer",
    };
    key_name(prefix, spki_der, persisted)
}

/// What a desktop calls itself: a name derived from the public half of the key its
/// certificate carries (GAP-141).
///
/// **Derived rather than stated**, which is the property the node needs. A desktop's
/// certificate is self-signed, so a name it simply asserted would be a claim nothing
/// checked, and two desktops could assert the same one. A fingerprint of the key is a
/// function of what the handshake actually proved possession of, so a node comparing a
/// forwarded batch's `origin` with the name on the certificate it verified is comparing
/// two views of one key.
///
/// **`persisted` is part of the name, not a footnote.** A desktop whose key provider is
/// unreachable gets a fresh key each start, and a name that changes with it; saying so in
/// the name means a record that carries it says what the origin is worth, rather than
/// implying a stability the key does not have.
#[must_use]
pub fn desktop_name(spki_der: &[u8], persisted: bool) -> String {
    key_name("desktop", spki_der, persisted)
}

/// A desktop's identity: the key it presents, the name that key gives it, and whether
/// that name survives a restart (GAP-141).
pub struct DesktopIdentity {
    /// Ready for [`crate::LinkTls::issued`].
    pub key: Arc<rustls::sign::CertifiedKey>,
    /// The certificate's subject common name, which is what a node reads as the party.
    pub name: String,
    /// True when the key came from the operating system's keystore, so the name is the
    /// same after a restart.
    pub persisted: bool,
}

/// A self-signed certificate over the provider's transport key, and the rustls key
/// that presents it. Used by a node to serve and by a desktop to authenticate itself.
pub struct HostIdentity {
    pub certificate_der: Vec<u8>,
    pub certificate_pem: String,
    pub key: Arc<dyn rustls::sign::SigningKey>,
    /// The certificate's subject common name.
    pub common_name: String,
    /// True when the key came from the operating system's keystore and survives a
    /// restart.
    pub persisted: bool,
}

/// Issue an identity for `names` (DNS names or IP addresses the host is reached as),
/// limited to `usage` (D-135).
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
    usage: Usage,
) -> Result<HostIdentity, String> {
    if key.purpose != KeyPurpose::TransportIdentity {
        return Err(format!("{key:?} is not a transport identity key"));
    }
    let signing = ProviderKey::new(provider, key, spki_der);
    let mut params = rcgen::CertificateParams::new(names).map_err(|e| e.to_string())?;
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, common_name);
    params.extended_key_usages = vec![usage.purpose()];
    let certificate = params.self_signed(&signing).map_err(|e| e.to_string())?;
    Ok(HostIdentity {
        certificate_der: certificate.der().as_ref().to_vec(),
        certificate_pem: certificate.pem(),
        key: Arc::new(signing),
        common_name: common_name.to_owned(),
        persisted: false,
    })
}

/// Issue a client identity for this host from an ephemeral key provider, ready to put in
/// [`crate::LinkTls::issued`], limited to client authentication (D-135).
///
/// **The private half never exists outside the provider**, which is the whole reason this
/// returns a `CertifiedKey` and not a PEM: a PEM carrying a usable identity must contain
/// the private key, and that is the one thing custody exists to prevent. It is also why
/// `gungnir-app` calls this rather than building the type itself -- it would need `rustls`
/// to name it, and there is no reason for the desktop to grow a TLS dependency to hold a
/// value it only passes along.
///
/// **Ephemeral, and that is a stated limit rather than an oversight.** The key lives in
/// process memory, so this is a new identity every start. A host that needs one a peer
/// can pin uses a persistent issuer: [`issue_node_peer_identity`] or
/// [`issue_desktop_outbound_identity`].
///
/// # Errors
///
/// When the provider cannot report its public half, or `rcgen` refuses the name.
pub fn issue_for_client(common_name: &str) -> Result<Arc<rustls::sign::CertifiedKey>, String> {
    Ok(as_certified_key(issue_ephemeral_named(
        vec!["localhost".to_owned()],
        Usage::Client,
        &|_, _| common_name.to_owned(),
    )?))
}

/// As [`issue_for_client`], limited to serving (D-135): an ephemeral serving identity
/// for a host reached as `localhost`, for a harness that serves the transport.
///
/// # Errors
///
/// As [`issue_for_client`].
pub fn issue_for_server(common_name: &str) -> Result<Arc<rustls::sign::CertifiedKey>, String> {
    Ok(as_certified_key(issue_ephemeral_named(
        vec!["localhost".to_owned()],
        Usage::Server,
        &|_, _| common_name.to_owned(),
    )?))
}

/// A desktop's ephemeral identity, named after the key it just generated (GAP-141).
///
/// A desktop's name has to tell two desktops apart, and only a fingerprint of the key
/// does that without trusting a string; a node's names are fingerprints too since D-135.
///
/// # Errors
///
/// As [`issue_for_client`].
pub fn issue_desktop_identity() -> Result<DesktopIdentity, String> {
    let identity =
        issue_ephemeral_named(vec!["localhost".to_owned()], Usage::Client, &|spki, _| {
            desktop_name(spki, false)
        })?;
    Ok(DesktopIdentity {
        name: identity.common_name.clone(),
        key: as_certified_key(identity),
        persisted: false,
    })
}

/// How an issued identity is named: from the public half of its key, and whether that
/// key survives a restart.
type NameOf<'a> = &'a dyn Fn(&[u8], bool) -> String;

/// A fresh identity from a new ephemeral in-process provider, over `names`, limited to
/// `usage`, named by `name_of` from the public half of the key it just generated.
fn issue_ephemeral_named(
    names: Vec<String>,
    usage: Usage,
    name_of: NameOf<'_>,
) -> Result<HostIdentity, String> {
    use gungnir_security::{KeyPurpose, P256KeyProvider};
    let mut provider = P256KeyProvider::new();
    let key = provider.generate(KeyPurpose::TransportIdentity);
    let spki = provider
        .public_key_der(&key)
        .map_err(|e| format!("the key provider gave no public half: {e}"))?;
    let common_name = name_of(&spki, false);
    issue(Arc::new(provider), key, spki, names, &common_name, usage)
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

/// The name a node's serving-identity keystore entries live under in the operating
/// system's keystore (D-39; GAP-060's remaining slice) -- distinct from
/// `gungnir-node-accounts` (GAP-057's node account store), from
/// [`NODE_PEER_TLS_IDENTITY_SERVICE`] and from `DESKTOP_TLS_IDENTITY_SERVICE` below, so
/// none of them collide on one machine.
const NODE_TLS_IDENTITY_SERVICE: &str = "gungnir-node-tls-identity";

/// As [`NODE_TLS_IDENTITY_SERVICE`], for the node's outbound, peer-link identity (D-135):
/// its own key, so a compromise of one role's key is not a compromise of the other.
const NODE_PEER_TLS_IDENTITY_SERVICE: &str = "gungnir-node-peer-tls-identity";

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

/// As [`TLS_IDENTITY_SUBDIR`], for the node's peer-link identity, which lives beside the
/// node's serving one in the same data directory and so needs a file of its own (D-135).
const PEER_TLS_IDENTITY_SUBDIR: &str = "peer-tls-identity";

/// Where the node's serving identity is kept.
const SERVING_CUSTODY: Custody = Custody {
    service: NODE_TLS_IDENTITY_SERVICE,
    subdir: TLS_IDENTITY_SUBDIR,
};

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

/// Where one persisted identity lives: its keystore service and its subdirectory.
#[derive(Debug, Clone, Copy)]
struct Custody {
    service: &'static str,
    subdir: &'static str,
}

/// Open (or create) the persistent keystore backing a TLS identity: its own
/// subdirectory of `data_dir`, under its service's entry in the operating system's
/// keystore. No escrow -- escrow recovers sealed data for a party who lacks the key
/// that sealed it (DN-22 §11), and a signing key has nothing sealed to recover.
fn persistent_provider(
    data_dir: &Path,
    custody: Custody,
) -> Result<Arc<gungnir_security::PersistentKeyProvider>, String> {
    let keystore_dir = data_dir.join(custody.subdir);
    std::fs::create_dir_all(&keystore_dir)
        .map_err(|e| format!("creating {}: {e}", keystore_dir.display()))?;
    let account = account_for(data_dir);
    let provider = gungnir_security::PersistentKeyProvider::open_or_create_via_os_keystore(
        &keystore_dir,
        custody.service,
        &account,
        None,
    )
    .map_err(|e| e.to_string())?;
    Ok(Arc::new(provider))
}

/// Issue `names`' identity, limited to `usage`, from a persistent, OS-keystore-backed
/// provider under `custody` when one is reachable; fall back to a fresh ephemeral one,
/// honestly logged as a fallback, when it is not. Named by `name_of` from the key
/// actually used, so the name says which it was. See the module's own doc section for
/// why this never silently claims persistence it does not have.
fn issue_persistent_or_ephemeral(
    data_dir: &Path,
    custody: Custody,
    names: Vec<String>,
    usage: Usage,
    name_of: NameOf<'_>,
) -> Result<HostIdentity, String> {
    let attempt = persistent_provider(data_dir, custody).and_then(|provider| {
        let key = provider
            .active_or_generate(KeyPurpose::TransportIdentity)
            .map_err(|e| format!("no transport key: {e}"))?;
        let spki = provider
            .public_key_der(&key)
            .map_err(|e| format!("no public half: {e}"))?;
        let common_name = name_of(&spki, true);
        let mut identity = issue(provider, key, spki, names.clone(), &common_name, usage)?;
        identity.persisted = true;
        Ok(identity)
    });
    match attempt {
        Ok(identity) => {
            tracing::info!(
                service = custody.service,
                name = %identity.common_name,
                "this identity is persisted in the operating-system keystore and will \
                 survive a restart"
            );
            Ok(identity)
        }
        Err(err) => {
            tracing::warn!(
                service = custody.service,
                %err,
                "could not issue this identity from the operating-system keystore; \
                 issuing an ephemeral one instead, which will not survive a restart and \
                 whose name says so"
            );
            issue_ephemeral_named(names, usage, name_of)
        }
    }
}

/// The node's serving identity (`gungnir-node/src/main.rs::spawn_tls_from_provider`),
/// issued from a persistent, OS-keystore-backed provider so the certificate written
/// beside the journal for operators to pin (`node-identity.pem`) is the same
/// certificate after a restart -- falling back to a fresh ephemeral one, honestly, when
/// the keystore is unreachable (GAP-060's remaining slice; D-39).
///
/// **D-135**: named `node-<fingerprint>` from its own key, and limited to serving.
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
) -> Result<HostIdentity, String> {
    issue_persistent_or_ephemeral(
        data_dir,
        SERVING_CUSTODY,
        names,
        Usage::Server,
        &|spki, persisted| node_name(spki, NodeRole::Serving, persisted),
    )
}

/// What a node presents on its peer links (D-135): the key, the name that key gives it,
/// and whether both survive a restart.
pub struct NodePeerIdentity {
    /// Ready for [`crate::LinkTls::issued`].
    pub key: Arc<rustls::sign::CertifiedKey>,
    /// The certificate's subject common name, which is what a peer reads as the party
    /// and names in its `machine_identities`.
    pub name: String,
    /// True when the key came from the operating system's keystore.
    pub persisted: bool,
    /// The public certificate, for operators to pin at each peer.
    pub certificate_pem: String,
}

impl From<HostIdentity> for NodePeerIdentity {
    fn from(identity: HostIdentity) -> Self {
        NodePeerIdentity {
            name: identity.common_name.clone(),
            persisted: identity.persisted,
            certificate_pem: identity.certificate_pem.clone(),
            key: as_certified_key(identity),
        }
    }
}

/// The node's outbound, peer-link identity (`gungnir-node/src/main.rs::host_tls`), D-135:
/// its own key, persisted in the operating system's keystore beside the serving one under
/// its own service and file, so a peer pins it once; named `node-peer-<fingerprint>`;
/// limited to client authentication. Falls back to a fresh ephemeral one, honestly logged
/// and named `node-peer-ephemeral-...`, when the keystore is unreachable.
///
/// # Errors
///
/// As [`issue_node_serving_identity`].
pub fn issue_node_peer_identity(data_dir: &Path) -> Result<NodePeerIdentity, String> {
    issue_persistent_or_ephemeral(
        data_dir,
        Custody {
            service: NODE_PEER_TLS_IDENTITY_SERVICE,
            subdir: PEER_TLS_IDENTITY_SUBDIR,
        },
        vec!["localhost".to_owned()],
        Usage::Client,
        &|spki, persisted| node_name(spki, NodeRole::Peer, persisted),
    )
    .map(NodePeerIdentity::from)
}

/// As [`issue_node_peer_identity`], from an ephemeral provider only: for a node whose
/// baseline does not use the operating system's keystore, which is not made to leave an
/// entry in it (the gate `gungnir-app/src/session.rs::issue_identity` applies to a
/// desktop's).
///
/// # Errors
///
/// As [`issue_for_client`].
pub fn issue_node_peer_identity_ephemeral() -> Result<NodePeerIdentity, String> {
    issue_ephemeral_named(vec!["localhost".to_owned()], Usage::Client, &|spki, _| {
        node_name(spki, NodeRole::Peer, false)
    })
    .map(NodePeerIdentity::from)
}

/// The desktop's outbound identity (`gungnir-app/src/session.rs::link_tls_for`),
/// issued from a persistent, OS-keystore-backed provider so the identity a node sees
/// from this desktop survives a restart -- falling back to a fresh ephemeral one,
/// honestly, when the keystore is unreachable (GAP-060's remaining slice; D-39).
/// Limited to client authentication (D-135).
///
/// `data_dir` is the desktop's own data directory (`ConfigBaseline::data_dir`).
///
/// # Errors
///
/// As [`issue_node_serving_identity`].
pub fn issue_desktop_outbound_identity(data_dir: &Path) -> Result<DesktopIdentity, String> {
    issue_persistent_or_ephemeral(
        data_dir,
        Custody {
            service: DESKTOP_TLS_IDENTITY_SERVICE,
            subdir: TLS_IDENTITY_SUBDIR,
        },
        vec!["localhost".to_owned()],
        Usage::Client,
        &desktop_name,
    )
    .map(|identity| DesktopIdentity {
        name: identity.common_name.clone(),
        persisted: identity.persisted,
        key: as_certified_key(identity),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_security::P256KeyProvider;
    use p256::ecdsa::signature::Verifier;

    /// **Found reviewing this file.** The existing
    /// round-trip test above proves the provider's true key produces a valid signature,
    /// but never inspects `identity.certificate_der` itself -- so a bug in
    /// `ProviderKey::der_bytes()`'s slice of `spki_der` (the module's own doc comment
    /// names it: "the last 65 bytes of the DER") could embed the wrong public key in the
    /// certificate while every other assertion here still passed, since the signature is
    /// produced independently of `der_bytes()`. This checks the one thing that would
    /// actually catch that: the true EC point, obtained the same way `der_bytes()` is
    /// documented to obtain it, must appear in the certificate `rcgen` actually built.
    /// It passes as the code stands -- no defect found here -- and is kept as a
    /// regression test against `der_bytes()`'s slicing changing silently.
    #[test]
    fn the_certificates_own_embedded_key_is_the_true_one() {
        let mut provider = P256KeyProvider::new();
        let key = provider.generate(KeyPurpose::TransportIdentity);
        let spki = provider.public_key_der(&key).expect("spki");
        let point = spki[spki.len() - 65..].to_vec();
        let provider = Arc::new(provider);
        let identity = issue(
            Arc::clone(&provider),
            key,
            spki.clone(),
            vec!["localhost".into()],
            "gungnir-node",
            Usage::Server,
        )
        .expect("issued");
        let found = identity
            .certificate_der
            .windows(65)
            .any(|w| w == point.as_slice());
        assert!(
            found,
            "the true EC point does not appear in the certificate DER at all"
        );
    }

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
            Usage::Server,
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
            Usage::Server,
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
            Usage::Server,
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
            Usage::Server,
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

        let identity =
            issue_node_serving_identity(&path, vec!["127.0.0.1".into(), "localhost".into()])
                .expect("the ephemeral fallback still issues");
        assert!(identity.certificate_pem.contains("BEGIN CERTIFICATE"));
        // D-135: the fallback's name is the key's, and says it does not survive a restart.
        assert!(!identity.persisted);
        assert!(
            identity.common_name.starts_with("node-ephemeral-"),
            "{}",
            identity.common_name
        );

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

        let identity =
            issue_desktop_outbound_identity(&path).expect("the ephemeral fallback still issues");
        assert!(!identity.key.cert.is_empty());
        // GAP-141: the fallback's name says the key behind it does not survive a restart,
        // so a record carrying it says what the origin is worth.
        assert!(!identity.persisted);
        assert!(
            identity.name.starts_with("desktop-ephemeral-"),
            "{}",
            identity.name
        );

        let _ = std::fs::remove_file(&path);
    }

    /// The DER of an extended-key-usage purpose's OID: `id-kp` (1.3.6.1.5.5.7.3) and the
    /// purpose's last arc.
    fn eku_oid(last: u8) -> [u8; 10] {
        [0x06, 0x08, 0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, last]
    }

    fn states(der: &[u8], last: u8) -> bool {
        der.windows(10).any(|w| w == eku_oid(last))
    }

    /// D-135: every certificate states its one role -- serverAuth (…3.1) on a serving
    /// identity, clientAuth (…3.2) on a desktop's and on a node's outbound one -- and
    /// never the other.
    #[test]
    fn every_certificate_states_its_one_role() {
        const SERVER: u8 = 1;
        const CLIENT: u8 = 2;
        let blocked = std::env::temp_dir().join(format!(
            "gungnir-remote-identity-eku-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&blocked);
        std::fs::write(&blocked, b"not a directory").expect("forces the ephemeral path");
        let serving = issue_node_serving_identity(&blocked, vec!["localhost".into()])
            .expect("issued")
            .certificate_der;
        let peer = issue_node_peer_identity(&blocked).expect("issued");
        let peer = peer.key.cert[0].as_ref().to_vec();
        let desktop = issue_desktop_identity().expect("issued");
        let desktop = desktop.key.cert[0].as_ref().to_vec();
        let server_harness = issue_for_server("harness").expect("issued");
        let client_harness = issue_for_client("harness").expect("issued");
        assert!(states(&serving, SERVER) && !states(&serving, CLIENT));
        assert!(states(server_harness.cert[0].as_ref(), SERVER));
        for der in [
            peer.as_slice(),
            desktop.as_slice(),
            client_harness.cert[0].as_ref(),
        ] {
            assert!(states(der, CLIENT) && !states(der, SERVER));
        }
        let _ = std::fs::remove_file(&blocked);
    }

    /// D-135: a node's two identities are named from their own keys, so two nodes never
    /// share a name and a node's serving and outbound names never collide; the name says
    /// whether the key survives a restart.
    #[test]
    fn a_nodes_names_are_its_keys_so_two_nodes_and_two_roles_differ() {
        let spki = vec![7u8; 91];
        let serving = node_name(&spki, NodeRole::Serving, true);
        let peer = node_name(&spki, NodeRole::Peer, true);
        assert!(serving.starts_with("node-") && !serving.starts_with("node-peer-"));
        assert!(peer.starts_with("node-peer-"));
        assert_ne!(serving, peer);
        assert_ne!(serving, node_name(&[8u8; 91], NodeRole::Serving, true));
        assert!(node_name(&spki, NodeRole::Peer, false).starts_with("node-peer-ephemeral-"));
        assert_eq!(serving, node_name(&spki, NodeRole::Serving, true), "stable");
        let one = issue_node_peer_identity_ephemeral().expect("issued");
        let two = issue_node_peer_identity_ephemeral().expect("issued");
        assert_ne!(one.name, two.name, "two nodes issued one name");
        // The name a peer reads off the certificate is the one the identity reports.
        let on_certificate = gungnir_api::tls::subject_common_name(one.key.cert[0].as_ref());
        assert_eq!(on_certificate.as_deref(), Some(one.name.as_str()));
        assert!(one.certificate_pem.contains("BEGIN CERTIFICATE"));
    }

    /// As the serving identity's fallback, for the node's peer one (D-135).
    #[test]
    fn issue_node_peer_identity_falls_back_to_an_ephemeral_identity_when_the_keystore_directory_cannot_be_created(
    ) {
        let path = std::env::temp_dir().join(format!(
            "gungnir-remote-identity-blocked-peer-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, b"not a directory").expect("file standing in for a directory");
        let identity =
            issue_node_peer_identity(&path).expect("the ephemeral fallback still issues");
        assert!(!identity.persisted);
        assert!(
            identity.name.starts_with("node-peer-ephemeral-"),
            "{}",
            identity.name
        );
        let _ = std::fs::remove_file(&path);
    }

    /// GAP-141: two desktops are told apart by their keys, and one desktop keeps its name
    /// as long as its key.
    #[test]
    fn a_desktops_name_is_its_key_so_two_desktops_differ_and_one_is_stable() {
        let first = issue_desktop_identity().expect("issued");
        let second = issue_desktop_identity().expect("issued");
        assert_ne!(
            first.name, second.name,
            "two desktops issued the same name: {}",
            first.name
        );
        // The same key gives the same name every time it is asked, which is what makes a
        // persisted identity stable across restarts.
        let spki = vec![7u8; 91];
        assert_eq!(desktop_name(&spki, true), desktop_name(&spki, true));
        assert_ne!(
            desktop_name(&spki, true),
            desktop_name(&[8u8; 91], true),
            "two keys gave one name"
        );
        // Whether it survives a restart is part of the name, not a footnote beside it.
        assert!(desktop_name(&spki, true).starts_with("desktop-"));
        assert!(desktop_name(&spki, false).starts_with("desktop-ephemeral-"));
        assert_ne!(desktop_name(&spki, true), desktop_name(&spki, false));
        // Sixteen hex digits of fingerprint, and nothing a certificate would refuse.
        let name = desktop_name(&spki, true);
        assert_eq!(name.len(), "desktop-".len() + DESKTOP_NAME_HEX);
        assert!(name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    }

    /// The name on the certificate is the name the desktop reports: a node reads the
    /// first and a forwarded batch carries the second, and GAP-141 turns on them agreeing.
    #[test]
    fn the_issued_certificate_carries_the_name_the_identity_reports() {
        let identity = issue_desktop_identity().expect("issued");
        let der = identity.key.cert.first().expect("a certificate").as_ref();
        let on_certificate = gungnir_api::tls::subject_common_name(der);
        assert_eq!(on_certificate.as_deref(), Some(identity.name.as_str()));
    }

    /// The two persisted identities never share a service name, so the same account
    /// (the same `data_dir`) addresses two independent operating-system-keystore
    /// entries rather than one.
    #[test]
    fn the_node_and_desktop_tls_identity_services_are_distinct_from_each_other_and_from_the_account_store_and_desktop_keystore(
    ) {
        let names = [
            NODE_TLS_IDENTITY_SERVICE,
            NODE_PEER_TLS_IDENTITY_SERVICE,
            DESKTOP_TLS_IDENTITY_SERVICE,
            "gungnir-node-accounts",
            "gungnir-desktop-keystore",
        ];
        // And the node's two identities never share a keystore file (D-135).
        assert_ne!(TLS_IDENTITY_SUBDIR, PEER_TLS_IDENTITY_SUBDIR);
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

        match persistent_provider(&dir, SERVING_CUSTODY) {
            Ok(first) => {
                let key = first
                    .active_or_generate(KeyPurpose::TransportIdentity)
                    .expect("key");
                let spki = first.public_key_der(&key).expect("spki");
                drop(first);

                // Reopened: the OS keystore handed back the same secret it stored the
                // first time, so the same key comes back rather than a fresh one.
                let again = persistent_provider(&dir, SERVING_CUSTODY)
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

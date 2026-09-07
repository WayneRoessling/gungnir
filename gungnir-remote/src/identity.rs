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
//! depend on at runtime** that already carries `rustls`, `tokio-rustls` and
//! `rustls-pemfile`, and because it already owns [`crate::LinkTls`] -- the type that
//! answers "who is this host". `gungnir-api` would have been a better fit still, but the
//! desktop holds it as a dev-dependency only and `ARCHITECTURE.md` refuses that as a
//! production edge.

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
    use gungnir_security::{KeyPurpose, P256KeyProvider};
    let mut provider = P256KeyProvider::new();
    let key = provider.generate(KeyPurpose::TransportIdentity);
    let spki = provider
        .public_key_der(&key)
        .map_err(|e| format!("the key provider gave no public half: {e}"))?;
    let identity = issue(
        Arc::new(provider),
        key,
        spki,
        vec!["localhost".to_owned()],
        common_name,
    )?;
    Ok(Arc::new(rustls::sign::CertifiedKey::new(
        vec![rustls::pki_types::CertificateDer::from(
            identity.certificate_der,
        )],
        identity.key,
    )))
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
}

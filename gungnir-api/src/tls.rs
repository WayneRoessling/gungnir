// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Mutual TLS for the v2 transport (GAP-060, D-02).
//!
//! Crates: `rustls`, `tokio-rustls` and `rustls-pemfile`, signed off under D-18 on
//! 2026-09-05 and unused until now.
//!
//! # Which custody model this is
//!
//! DN-22 §5 gives three profiles genuinely different answers, and this is the **on-prem**
//! one: the certificate chain and the private key are PEM files the host provides, and
//! custody is the file's permissions. That is what §2.9's `rustls-pemfile` row describes
//! -- "reads them; never holds or logs the key material".
//!
//! It is **not** the cloud answer. There, material never leaves a managed service and
//! `rustls` is given a `sign::SigningKey` that delegates to it -- which is what DN-22
//! amendment 1 (a) added `KeyProvider::sign` for. That path needs an asymmetric provider,
//! which needs the signature scheme D-22 left as its open third row, so it is not built.
//! The two coexist by design rather than one superseding the other.
//!
//! **No path to key material appears in a configuration baseline.** The paths come from
//! the environment, as DN-22 §6 and the workspace's own rule require.
//!
//! # Mutual, not optional
//!
//! A client certificate is **required**, not requested. D-02 fixes machine identity as
//! mutual TLS, and a server that accepted anonymous clients would be authenticating the
//! server to the client and nobody to the server -- which is the half that matters least
//! for a command-and-control surface on a defended network.

use crate::ApiError;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};
use std::path::Path;
use std::sync::Arc;
pub use tokio_rustls::TlsAcceptor;

/// Where a node's TLS material lives.
///
/// Paths, not bytes: this struct is built from the environment and the files are read
/// once at start-up. It deliberately has no `Debug` deriving the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsPaths {
    /// This node's certificate chain, leaf first. Public.
    pub certificate_chain: String,
    /// This node's private key. **The only path here that names key material.**
    pub private_key: String,
    /// The authority that signs the client certificates this node will accept.
    pub client_ca: String,
}

impl TlsPaths {
    /// Read the three paths from the environment.
    ///
    /// `None` when none is set, which is the ordinary case and means the node serves
    /// loopback in the clear. All three or nothing: a node with a certificate and no
    /// client authority would serve one-way TLS, which is not what D-02 chose, and
    /// falling back to it silently would be worse than not starting.
    #[must_use]
    pub fn from_env() -> Option<Result<Self, ApiError>> {
        Self::from_env_parts(
            std::env::var("GUNGNIR_TLS_CERT").ok().as_deref(),
            std::env::var("GUNGNIR_TLS_KEY").ok().as_deref(),
            std::env::var("GUNGNIR_TLS_CLIENT_CA").ok().as_deref(),
        )
    }

    /// The decision, separated from where the values came from so a test can assert it
    /// without setting process-wide environment variables and racing other tests.
    #[must_use]
    pub fn from_env_parts(
        certificate_chain: Option<&str>,
        private_key: Option<&str>,
        client_ca: Option<&str>,
    ) -> Option<Result<Self, ApiError>> {
        let certificate_chain = certificate_chain.map(ToOwned::to_owned);
        let private_key = private_key.map(ToOwned::to_owned);
        let client_ca = client_ca.map(ToOwned::to_owned);
        match (certificate_chain, private_key, client_ca) {
            (None, None, None) => None,
            (Some(certificate_chain), Some(private_key), Some(client_ca)) => Some(Ok(Self {
                certificate_chain,
                private_key,
                client_ca,
            })),
            _ => Some(Err(ApiError::Transport(
                "TLS needs all three of GUNGNIR_TLS_CERT, GUNGNIR_TLS_KEY and \
                 GUNGNIR_TLS_CLIENT_CA; a node with only some of them would serve \
                 one-way TLS, which is not the mutual authentication D-02 chose"
                    .into(),
            ))),
        }
    }
}

/// Build the acceptor a node serves with.
///
/// # Errors
///
/// When a file cannot be read or does not contain what it should. Every failure is fatal
/// to serving: a node that fell back to plaintext because a certificate was missing would
/// be the silent downgrade this whole design refuses.
/// The client authority alone, when a deployment issues the node's own identity from
/// its key provider (D-29) and pins only what it must trust.
#[must_use]
pub fn client_ca_from_env() -> Option<String> {
    std::env::var("GUNGNIR_TLS_CLIENT_CA").ok()
}

/// A mutual-TLS acceptor over a certificate and a signing key that is not a private-key
/// file: the provider's key, signing through custody (D-29, DN-22 amendment 1).
///
/// # Errors
///
/// As [`acceptor`], for the client authority.
pub fn acceptor_with_key(
    certificate_der: Vec<u8>,
    key: Arc<dyn rustls::sign::SigningKey>,
    client_ca: &str,
) -> Result<TlsAcceptor, ApiError> {
    let mut roots = RootCertStore::empty();
    for authority in read_certificates(Path::new(client_ca))? {
        roots.add(authority).map_err(|e| {
            ApiError::Transport(format!("{client_ca} is not a usable authority: {e}"))
        })?;
    }
    if roots.is_empty() {
        return Err(ApiError::Transport(format!(
            "{client_ca} contains no client authority, so no client could ever be verified"
        )));
    }
    let verifier = WebPkiClientVerifier::builder(Arc::new(roots))
        .build()
        .map_err(|e| ApiError::Transport(format!("the client verifier is unusable: {e}")))?;
    let certified =
        rustls::sign::CertifiedKey::new(vec![CertificateDer::from(certificate_der)], key);
    let config = ServerConfig::builder()
        .with_client_cert_verifier(verifier)
        .with_cert_resolver(Arc::new(rustls::sign::SingleCertAndKey::from(certified)));
    Ok(TlsAcceptor::from(Arc::new(config)))
}

pub fn acceptor(paths: &TlsPaths) -> Result<TlsAcceptor, ApiError> {
    let certs = read_certificates(Path::new(&paths.certificate_chain))?;
    if certs.is_empty() {
        return Err(ApiError::Transport(format!(
            "{} contains no certificate",
            paths.certificate_chain
        )));
    }
    let key = read_private_key(Path::new(&paths.private_key))?;

    let mut roots = RootCertStore::empty();
    for authority in read_certificates(Path::new(&paths.client_ca))? {
        roots.add(authority).map_err(|e| {
            ApiError::Transport(format!(
                "{} is not a usable authority: {e}",
                paths.client_ca
            ))
        })?;
    }
    if roots.is_empty() {
        return Err(ApiError::Transport(format!(
            "{} contains no client authority, so no client could ever be verified",
            paths.client_ca
        )));
    }

    // `builder` and not `builder_with_certificate_optional`: a client certificate is
    // required. See the module documentation.
    let verifier = WebPkiClientVerifier::builder(Arc::new(roots))
        .build()
        .map_err(|e| ApiError::Transport(format!("the client verifier is unusable: {e}")))?;

    let config = ServerConfig::builder()
        .with_client_cert_verifier(verifier)
        .with_single_cert(certs, key)
        .map_err(|e| ApiError::Transport(format!("the certificate and key do not match: {e}")))?;

    Ok(TlsAcceptor::from(Arc::new(config)))
}

fn read_certificates(path: &Path) -> Result<Vec<CertificateDer<'static>>, ApiError> {
    let bytes = std::fs::read(path)
        .map_err(|e| ApiError::Transport(format!("could not read {}: {e}", path.display())))?;
    rustls_pemfile::certs(&mut bytes.as_slice())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| ApiError::Transport(format!("{} is not valid PEM: {e}", path.display())))
}

/// Read a private key, and **never log it or include it in an error**.
///
/// The error says which file failed and nothing about its contents, because an error
/// message is the easiest way for key material to reach a log.
fn read_private_key(path: &Path) -> Result<PrivateKeyDer<'static>, ApiError> {
    let bytes = std::fs::read(path)
        .map_err(|e| ApiError::Transport(format!("could not read {}: {e}", path.display())))?;
    rustls_pemfile::private_key(&mut bytes.as_slice())
        .map_err(|_| ApiError::Transport(format!("{} is not a valid private key", path.display())))?
        .ok_or_else(|| ApiError::Transport(format!("{} contains no private key", path.display())))
}

/// Who a connection is, as far as the transport can say (GAP-062, D-02).
///
/// `party` is the subject common name of the client certificate the handshake verified,
/// and is what DN-17's per-party filtering and DN-18's agreements key on. A plaintext
/// connection has none, and every route that would release anything to a party treats
/// that as `Internal` (DN-17 §5, "there is no anonymous peer").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    pub addr: std::net::SocketAddr,
    pub party: Option<String>,
}

impl axum::extract::connect_info::Connected<axum::serve::IncomingStream<'_, TlsListener>> for Peer {
    fn connect_info(stream: axum::serve::IncomingStream<'_, TlsListener>) -> Self {
        stream.remote_addr().clone()
    }
}

impl axum::extract::connect_info::Connected<axum::serve::IncomingStream<'_, PlainListener>>
    for Peer
{
    fn connect_info(stream: axum::serve::IncomingStream<'_, PlainListener>) -> Self {
        stream.remote_addr().clone()
    }
}

/// A plaintext listener that reports the same [`Peer`] shape as [`TlsListener`], with no
/// party, so one router serves both.
pub struct PlainListener(pub tokio::net::TcpListener);

impl axum::serve::Listener for PlainListener {
    type Io = tokio::net::TcpStream;
    type Addr = Peer;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            if let Ok((stream, addr)) = self.0.accept().await {
                return (stream, Peer { addr, party: None });
            }
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.0.local_addr().map(|addr| Peer { addr, party: None })
    }
}

/// The subject common name of an X.509 certificate, read from its DER.
///
/// A deliberately small reader: the certificate has already been verified against the
/// client authority by rustls, so this only has to find one attribute in a structure
/// whose shape the verifier already accepted. It walks `Certificate ::= SEQUENCE {
/// tbsCertificate SEQUENCE { [0] version, serial, signature, issuer, validity, subject
/// ... } }` and returns the first `commonName` (OID 2.5.4.3) in `subject`. `None` when
/// the structure is not that, which the caller treats as no party.
#[must_use]
pub fn subject_common_name(der: &[u8]) -> Option<String> {
    let (_, cert, _) = tlv(der, 0)?;
    let (_, tbs, _) = tlv(cert, 0)?;
    let mut at = 0;
    // [0] EXPLICIT version, present when the tag is 0xA0.
    if tbs.first() == Some(&0xA0) {
        let (_, _, next) = tlv(tbs, at)?;
        at = next;
    }
    for _ in 0..4 {
        // serialNumber, signature, issuer, validity
        let (_, _, next) = tlv(tbs, at)?;
        at = next;
    }
    let (tag, subject, _) = tlv(tbs, at)?;
    if tag != 0x30 {
        return None;
    }
    let mut pos = 0;
    while pos < subject.len() {
        let (_, set, next_set) = tlv(subject, pos)?;
        let mut inner = 0;
        while inner < set.len() {
            let (_, attribute, next_attribute) = tlv(set, inner)?;
            let (oid_tag, oid, after_oid) = tlv(attribute, 0)?;
            if oid_tag == 0x06 && oid == [0x55, 0x04, 0x03] {
                let (value_tag, value, _) = tlv(attribute, after_oid)?;
                if matches!(value_tag, 0x0C | 0x13 | 0x16 | 0x14) {
                    return Some(String::from_utf8_lossy(value).into_owned());
                }
                return None;
            }
            inner = next_attribute;
        }
        pos = next_set;
    }
    None
}

/// One DER tag-length-value at `at`: the tag, the content, and the offset after it.
fn tlv(buf: &[u8], at: usize) -> Option<(u8, &[u8], usize)> {
    let tag = *buf.get(at)?;
    let first = *buf.get(at + 1)?;
    let (len, header) = if first & 0x80 == 0 {
        (usize::from(first), 2)
    } else {
        let n = usize::from(first & 0x7F);
        if n == 0 || n > 4 {
            return None;
        }
        let mut len = 0usize;
        for i in 0..n {
            len = (len << 8) | usize::from(*buf.get(at + 2 + i)?);
        }
        (len, 2 + n)
    };
    let start = at + header;
    let end = start.checked_add(len)?;
    let content = buf.get(start..end)?;
    Some((tag, content, end))
}

/// A TCP listener that completes a TLS handshake before handing a connection on.
///
/// Implements `axum`'s own `Listener` trait, so the rest of the transport is unchanged
/// and there is one router serving both the plaintext loopback case and this one.
pub struct TlsListener {
    inner: tokio::net::TcpListener,
    acceptor: TlsAcceptor,
}

impl TlsListener {
    #[must_use]
    pub fn new(inner: tokio::net::TcpListener, acceptor: TlsAcceptor) -> Self {
        Self { inner, acceptor }
    }
}

impl axum::serve::Listener for TlsListener {
    type Io = tokio_rustls::server::TlsStream<tokio::net::TcpStream>;
    type Addr = Peer;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let Ok((stream, addr)) = self.inner.accept().await else {
                continue;
            };
            match self.acceptor.accept(stream).await {
                Ok(tls) => {
                    // The verified client certificate's subject is the party (GAP-062).
                    let party = tls
                        .get_ref()
                        .1
                        .peer_certificates()
                        .and_then(|chain| chain.first())
                        .and_then(|cert| subject_common_name(cert.as_ref()));
                    return (tls, Peer { addr, party });
                }
                // A failed handshake is one client's problem, not the node's: an
                // unverified peer is refused and the listener keeps serving. Logged at
                // debug because a scanner would otherwise fill the log.
                Err(err) => {
                    tracing::debug!(%addr, %err, "a client failed the TLS handshake");
                }
            }
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.inner
            .local_addr()
            .map(|addr| Peer { addr, party: None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All three or nothing. A node with a certificate and no client authority would
    /// serve one-way TLS, which is not the mutual authentication D-02 chose.
    #[test]
    fn a_partial_configuration_is_refused_rather_than_downgraded() {
        // The environment is process-wide, so this asserts the decision function
        // directly rather than setting variables and racing other tests.
        let partial = TlsPaths::from_env_parts(Some("cert.pem"), None, Some("ca.pem"));
        assert!(partial.is_some_and(|r| r.is_err()));

        let none = TlsPaths::from_env_parts(None, None, None);
        assert!(none.is_none(), "an unconfigured node reported a TLS error");

        let all = TlsPaths::from_env_parts(Some("c"), Some("k"), Some("a"));
        assert!(all.is_some_and(|r| r.is_ok()));
    }

    #[test]
    fn a_missing_file_names_the_file_and_not_its_contents() {
        let paths = TlsPaths {
            certificate_chain: "does-not-exist.pem".into(),
            private_key: "does-not-exist.key".into(),
            client_ca: "does-not-exist.ca".into(),
        };
        let Err(err) = acceptor(&paths) else {
            panic!("a missing certificate produced a usable acceptor");
        };
        let message = err.to_string();
        assert!(message.contains("does-not-exist.pem"), "{message}");
    }
}

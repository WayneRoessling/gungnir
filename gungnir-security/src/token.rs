// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Short-lived session tokens, for a desktop talking to a node (GAP-057, DN-23 §5).
//!
//! D-02: "short-lived signed tokens for operator sessions issued by the node or a local
//! identity provider". Crates: D-20's `hmac`, `sha2` and `subtle`.
//!
//! # Why a MAC and not a signature
//!
//! The node issues these and the node verifies them. There is no third party to check a
//! signature, so a public-key scheme would buy nothing and cost the thing that stopped
//! TLS in GAP-041: a private key living in the process, outside DN-22's `seal`/`unseal`
//! custody boundary. A symmetric MAC keeps the secret somewhere a `KeyProvider` can hold
//! it. If a peer C2 system must one day verify a Gungnir token itself, that is a
//! different decision (§2.9, D-20).
//!
//! # What a token is not
//!
//! **It is not a capability.** It carries who the operator is and when it stops being
//! believed; it does not carry what they may do. Authorization stays with
//! `role_permits`, evaluated per request against the matrix, so widening a role never
//! means reissuing tokens and a stolen token never carries more authority than the
//! operator has right now.
//!
//! # The encoding
//!
//! `<hex payload>.<hex mac>`, where the payload is JSON. Hex rather than base64 so this
//! module needs no encoding crate; the tokens are not typed by hand and their length
//! does not matter.
//!
//! # A token is believed only by the process that issued it (GAP-212)
//!
//! A valid MAC is necessary and no longer sufficient. Every token carries a random
//! identifier, and the issuer holds a registry of the ones it issued, with when each was
//! issued and last used **on a monotonic clock**. So:
//!
//! - a token outlives neither its lifetime nor D-143's idle timeout, measured on a clock
//!   that cannot be set back, where the mission-time expiry it carries could be;
//! - a token can be revoked, one at a time or every one an operator holds, by taking it
//!   out of the registry;
//! - a node that restarts believes no token it issued before, which is what a monotonic
//!   clock that starts again at zero would have required anyway. A desktop signs in again.
//!
//! # The key (GAP-212)
//!
//! [`TokenIssuer::from_secret`] derives the MAC key from the deployment's secret with
//! HKDF-SHA256 under this module's own label, rather than using the secret's bytes as the
//! key. Holding the key in a `KeyProvider` needs a MAC operation no provider has; until
//! one does, this is the bridge DN-23 §17 names.

use crate::session::{
    AuthFailure, MissionTimeSeconds, MonotonicClock, OperatorSession, SystemMonotonic, IDLE_LOCK_S,
    SESSION_MAX_S,
};
use crate::{OperatorId, Role, SecurityError};
use hmac::{Mac, SimpleHmac};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use subtle::ConstantTimeEq;

/// The HKDF label the MAC key is derived under (GAP-212). Changing it ends every session.
const KEY_LABEL: &[u8] = b"gungnir-session-token/1";

type HmacSha256 = SimpleHmac<Sha256>;

/// The claims a token carries. Deliberately small: identity and lifetime, nothing else.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct Claims {
    /// Random, and the key to the issuer's registry (GAP-212).
    id: u64,
    operator: OperatorId,
    role: Role,
    established: MissionTimeSeconds,
    /// Always present. **A node-issued session always expires** (DN-23 §5); only the
    /// disconnected desktop's local-account profile may issue one that does not, and
    /// that profile does not mint tokens.
    expires: MissionTimeSeconds,
}

/// Mints and verifies session tokens for one node.
///
/// Holds the signing secret, which is why it is constructed by the binary and not by a
/// library: custody belongs to the host (DN-22 §4).
pub struct TokenIssuer {
    /// Cleared from memory when the issuer is dropped (GAP-215, D-145).
    key: zeroize::Zeroizing<Vec<u8>>,
    lifetime_s: MissionTimeSeconds,
    idle_s: MissionTimeSeconds,
    clock: Arc<dyn MonotonicClock>,
    /// The tokens this issuer has issued and not yet forgotten, by identifier.
    issued: Mutex<HashMap<u64, Live>>,
}

/// One issued token, on the monotonic clock.
#[derive(Debug, Clone, Copy)]
struct Live {
    operator: OperatorId,
    began: f64,
    used: f64,
}

impl std::fmt::Debug for TokenIssuer {
    /// Never prints the key. A `Debug` that leaked the signing secret into a log would
    /// undo the whole module.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenIssuer")
            .field("lifetime_s", &self.lifetime_s)
            .field("idle_s", &self.idle_s)
            .finish_non_exhaustive()
    }
}

impl TokenIssuer {
    /// # Errors
    ///
    /// When the key is too short to be worth using, or the lifetime is not positive or is
    /// longer than D-143's twelve hours. Each is refused rather than defaulted: a token
    /// signed with an empty key, or one that outlives the maximum, is worse than a node
    /// that will not start.
    pub fn new(key: Vec<u8>, lifetime_s: MissionTimeSeconds) -> Result<Self, SecurityError> {
        if key.len() < 32 {
            return Err(SecurityError::AuthenticationUnavailable(
                "the token signing key must be at least 32 bytes".into(),
            ));
        }
        if !lifetime_s.is_finite() || lifetime_s <= 0.0 {
            return Err(SecurityError::AuthenticationUnavailable(
                "a session token lifetime must be finite and positive".into(),
            ));
        }
        if lifetime_s > SESSION_MAX_S {
            return Err(SecurityError::AuthenticationUnavailable(format!(
                "a session token lifetime of {lifetime_s} s is longer than the {SESSION_MAX_S} s \
                 maximum (D-143)"
            )));
        }
        Ok(Self {
            key: zeroize::Zeroizing::new(key),
            lifetime_s,
            idle_s: IDLE_LOCK_S,
            clock: Arc::new(SystemMonotonic::default()),
            issued: Mutex::new(HashMap::new()),
        })
    }

    /// [`Self::new`], with the MAC key derived from `secret` by HKDF-SHA256 under this
    /// module's label (GAP-212), so the deployment's secret is never itself the key.
    ///
    /// # Errors
    ///
    /// A secret shorter than 32 bytes, and anything [`Self::new`] refuses.
    pub fn from_secret(
        secret: &[u8],
        lifetime_s: MissionTimeSeconds,
    ) -> Result<Self, SecurityError> {
        if secret.len() < 32 {
            return Err(SecurityError::AuthenticationUnavailable(
                "the token signing secret must be at least 32 bytes".into(),
            ));
        }
        let key = hkdf_sha256(KEY_LABEL, secret, KEY_LABEL)
            .map_err(|e| SecurityError::AuthenticationUnavailable(e.to_string()))?;
        Self::new(key, lifetime_s)
    }

    /// A token unused this long is refused, and never longer than D-143's fifteen minutes.
    #[must_use]
    pub fn with_idle_timeout(mut self, idle_s: Option<MissionTimeSeconds>) -> Self {
        self.idle_s = crate::session::at_most(idle_s, IDLE_LOCK_S);
        self
    }

    /// Measure lifetime and idleness on `clock` rather than the system's (a test's hand).
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn MonotonicClock>) -> Self {
        self.clock = clock;
        self
    }

    fn registry(&self) -> std::sync::MutexGuard<'_, HashMap<u64, Live>> {
        // Every entry is written whole, so a poisoned lock holds nothing half-written.
        self.issued
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Forget every token `operator` holds, so each is refused from now on (GAP-212).
    /// Returns how many there were.
    pub fn revoke_operator(&self, operator: OperatorId) -> usize {
        let mut issued = self.registry();
        let before = issued.len();
        issued.retain(|_, live| live.operator != operator);
        before - issued.len()
    }

    /// Forget one token, so it is refused from now on: a sign-out (GAP-212). False when
    /// it was not one this issuer holds.
    pub fn revoke(&self, token: &str) -> bool {
        self.claims(token)
            .is_some_and(|claims| self.registry().remove(&claims.id).is_some())
    }

    #[must_use]
    pub fn lifetime_s(&self) -> MissionTimeSeconds {
        self.lifetime_s
    }

    /// Mint a token for an operator who has already been verified.
    ///
    /// Takes a session rather than an identifier, so there is no path from a bare
    /// `OperatorId` to a token: the only way to get one is to have signed in.
    ///
    /// # Errors
    ///
    /// When the claims cannot be encoded, which should not happen.
    pub fn mint(
        &self,
        session: &OperatorSession,
        now: MissionTimeSeconds,
    ) -> Result<String, SecurityError> {
        let id = random_id();
        let claims = Claims {
            id,
            operator: session.operator,
            role: session.role,
            established: now,
            expires: now + self.lifetime_s,
        };
        let payload = serde_json::to_vec(&claims)
            .map_err(|e| SecurityError::AuthenticationUnavailable(e.to_string()))?;
        let mac = self
            .mac(&payload)
            .map_err(|e| SecurityError::AuthenticationUnavailable(e.to_string()))?;
        let at = self.clock.seconds();
        let mut issued = self.registry();
        // Forget what can no longer verify, so the registry is bounded by the tokens alive.
        let (lifetime, idle) = (self.lifetime_s, self.idle_s);
        issued.retain(|_, live| at - live.began < lifetime && at - live.used < idle);
        issued.insert(
            id,
            Live {
                operator: session.operator,
                began: at,
                used: at,
            },
        );
        Ok(format!("{}.{}", hex(&payload), hex(&mac)))
    }

    /// Verify a token and recover the session it stands for.
    ///
    /// # Errors
    ///
    /// [`AuthFailure::Rejected`] for anything wrong with the token, including an expired
    /// one. **A tampered token and an expired token are the same failure**, for the
    /// reason an unknown operator and a bad passphrase are: telling them apart tells a
    /// prober which half to work on.
    pub fn verify(
        &self,
        token: &str,
        now: MissionTimeSeconds,
    ) -> Result<OperatorSession, AuthFailure> {
        let claims = self.claims(token).ok_or(AuthFailure::Rejected)?;

        // GAP-212: a token this process issued, still inside its lifetime and its idle
        // timeout on the monotonic clock. Using it keeps it alive; an expired or idle one
        // is forgotten, so it is not revived by a later use either.
        {
            let at = self.clock.seconds();
            let mut issued = self.registry();
            let live = issued.get_mut(&claims.id).ok_or(AuthFailure::Rejected)?;
            if live.operator != claims.operator
                || at - live.began >= self.lifetime_s
                || at - live.used >= self.idle_s
            {
                issued.remove(&claims.id);
                return Err(AuthFailure::Rejected);
            }
            live.used = at;
        }
        let session = OperatorSession {
            operator: claims.operator,
            role: claims.role,
            established: claims.established,
            expires: Some(claims.expires),
        };
        if !session.is_valid_at(now) {
            return Err(AuthFailure::Rejected);
        }
        Ok(session)
    }

    /// The claims of a token whose MAC verifies under this issuer's key, or `None`.
    fn claims(&self, token: &str) -> Option<Claims> {
        let (payload_hex, mac_hex) = token.split_once('.')?;
        let payload = unhex(payload_hex)?;
        let presented = unhex(mac_hex)?;

        // Constant-time, so the comparison does not leak how much of a forged MAC was
        // right. `ct_eq` on differing lengths is false without short-circuiting.
        // A key the MAC will not take is an authenticator that cannot verify anything,
        // which is a rejection of every token rather than a panic in the request path.
        let expected = self.mac(&payload).ok()?;
        if expected.ct_eq(&presented).unwrap_u8() != 1 {
            return None;
        }

        // Only now is the payload worth reading: an unauthenticated payload is attacker
        // input, and deserializing it before the MAC check would be parsing what an
        // attacker chose.
        serde_json::from_slice(&payload).ok()
    }

    /// HMAC accepts a key of any length, so the error is unreachable in practice; it is
    /// still returned rather than unwrapped, because the unwrap policy (CONTRIBUTING,
    /// checked by `gungnir-app/tests/architecture_compliance.rs`) allows no exception
    /// on a request path, and a panic here would take the transport down with it
    /// (GAP-081).
    fn mac(&self, payload: &[u8]) -> Result<Vec<u8>, hmac::digest::InvalidLength> {
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&self.key[..])?;
        mac.update(payload);
        Ok(mac.finalize().into_bytes().to_vec())
    }
}

/// HKDF-SHA256 (RFC 5869) with one block of output: extract under `salt`, expand with
/// `info`. Written over `hmac` rather than adding the `hkdf` crate for two HMAC calls.
fn hkdf_sha256(
    salt: &[u8],
    ikm: &[u8],
    info: &[u8],
) -> Result<Vec<u8>, hmac::digest::InvalidLength> {
    let mut extract = <HmacSha256 as Mac>::new_from_slice(salt)?;
    extract.update(ikm);
    let prk = extract.finalize().into_bytes();
    let mut expand = <HmacSha256 as Mac>::new_from_slice(&prk)?;
    expand.update(info);
    expand.update(&[1]);
    Ok(expand.finalize().into_bytes().to_vec())
}

/// A token identifier from the operating system's generator.
fn random_id() -> u64 {
    let mut bytes = [0u8; 8];
    aes_gcm::aead::rand_core::RngCore::fill_bytes(&mut aes_gcm::aead::OsRng, &mut bytes);
    u64::from_le_bytes(bytes)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issuer() -> TokenIssuer {
        TokenIssuer::new(vec![7u8; 32], 60.0).expect("a valid issuer")
    }

    fn session() -> OperatorSession {
        OperatorSession {
            operator: OperatorId(7),
            role: Role::SensorManager,
            established: 0.0,
            expires: None,
        }
    }

    #[test]
    fn a_minted_token_verifies_and_carries_the_operator() {
        let issuer = issuer();
        let token = issuer.mint(&session(), 10.0).expect("minted");
        let recovered = issuer.verify(&token, 20.0).expect("verified");
        assert_eq!(recovered.operator, OperatorId(7));
        assert_eq!(recovered.role, Role::SensorManager);
        assert_eq!(recovered.expires, Some(70.0));
    }

    /// The property the MAC exists for. Changing any byte of the payload invalidates
    /// the token, so a caller cannot promote themselves by editing their own claims.
    #[test]
    fn a_tampered_payload_is_rejected() {
        let issuer = issuer();
        let token = issuer.mint(&session(), 10.0).expect("minted");
        let (payload, mac) = token.split_once('.').expect("well formed");

        // Flip one hex digit of the payload, keeping the length valid.
        let mut bytes: Vec<char> = payload.chars().collect();
        bytes[0] = if bytes[0] == 'a' { 'b' } else { 'a' };
        let forged: String = bytes.into_iter().collect();
        assert_eq!(
            issuer.verify(&format!("{forged}.{mac}"), 20.0),
            Err(AuthFailure::Rejected)
        );
    }

    /// A token from another node's key does not verify here. Without this a second
    /// deployment's operator would be believed by this one.
    #[test]
    fn a_token_from_another_key_is_rejected() {
        let theirs = TokenIssuer::new(vec![9u8; 32], 60.0).expect("issuer");
        let token = theirs.mint(&session(), 10.0).expect("minted");
        assert_eq!(issuer().verify(&token, 20.0), Err(AuthFailure::Rejected));
    }

    /// A node-issued session always expires, and an expired one is refused rather than
    /// renewed on use -- renewal on use would make "short-lived" meaningless
    /// (DN-23 §5 rule 2).
    #[test]
    fn an_expired_token_is_refused_and_never_renewed() {
        let issuer = issuer();
        let token = issuer.mint(&session(), 0.0).expect("minted");
        assert!(issuer.verify(&token, 59.0).is_ok());
        assert_eq!(issuer.verify(&token, 60.0), Err(AuthFailure::Rejected));
        // Using it again later does not revive it.
        assert_eq!(issuer.verify(&token, 61.0), Err(AuthFailure::Rejected));
    }

    /// Every node-issued token expires. A session that never expired would be a
    /// permanent credential handed out over the network.
    #[test]
    fn a_minted_token_always_expires() {
        let issuer = issuer();
        let recovered = issuer
            .verify(&issuer.mint(&session(), 5.0).expect("minted"), 5.0)
            .expect("verified");
        assert!(
            recovered.expires.is_some(),
            "a node issued a session that never expires"
        );
    }

    /// Malformed input is rejected without panicking and without saying what was wrong.
    #[test]
    fn malformed_tokens_are_rejected() {
        let issuer = issuer();
        for bad in ["", ".", "no-dot", "zz.zz", "aa.", ".aa", "abc.def"] {
            assert_eq!(
                issuer.verify(bad, 1.0),
                Err(AuthFailure::Rejected),
                "{bad:?} was not rejected"
            );
        }
    }

    /// A weak key and a non-positive lifetime are refused at construction. A node that
    /// started with either would be issuing credentials it should not.
    #[test]
    fn a_weak_key_or_a_bad_lifetime_is_refused() {
        assert!(TokenIssuer::new(vec![1u8; 16], 60.0).is_err());
        assert!(TokenIssuer::new(vec![1u8; 32], 0.0).is_err());
        assert!(TokenIssuer::new(vec![1u8; 32], f64::NAN).is_err());
        assert!(TokenIssuer::new(vec![1u8; 32], 1.0).is_ok());
    }

    /// The signing key never reaches a log.
    #[test]
    fn debug_does_not_print_the_key() {
        let printed = format!(
            "{:?}",
            TokenIssuer::new(vec![0xAB; 32], 30.0).expect("issuer")
        );
        assert!(!printed.contains("ab"), "{printed}");
        assert!(!printed.contains("171"), "{printed}");
        assert!(printed.contains("lifetime_s"), "{printed}");
    }

    /// GAP-212: a token unused for D-143's idle timeout is refused, on the monotonic
    /// clock, however far mission time has or has not moved; one used is kept alive.
    #[test]
    fn an_idle_token_is_refused_and_a_used_one_lives() {
        let clock = crate::session::ManualMonotonic::default();
        let issuer = TokenIssuer::new(vec![7u8; 32], 3600.0)
            .expect("issuer")
            .with_idle_timeout(Some(300.0))
            .with_clock(Arc::new(clock.clone()));
        let token = issuer.mint(&session(), 0.0).expect("minted");
        clock.advance(299.0);
        assert!(issuer.verify(&token, 0.0).is_ok());
        clock.advance(299.0);
        assert!(issuer.verify(&token, 0.0).is_ok(), "used, so still alive");
        clock.advance(300.0);
        assert_eq!(issuer.verify(&token, 0.0), Err(AuthFailure::Rejected));
        assert_eq!(issuer.verify(&token, 0.0), Err(AuthFailure::Rejected));
    }

    /// GAP-212: the lifetime holds on the monotonic clock, so a mission clock set back
    /// does not make an expired token good again.
    #[test]
    fn a_clock_set_back_does_not_revive_a_token() {
        let clock = crate::session::ManualMonotonic::default();
        let issuer = TokenIssuer::new(vec![7u8; 32], 600.0)
            .expect("issuer")
            .with_clock(Arc::new(clock.clone()));
        let token = issuer.mint(&session(), 1_000.0).expect("minted");
        for _ in 0..3 {
            clock.advance(200.0);
            let _ = issuer.verify(&token, 1_000.0);
        }
        assert_eq!(issuer.verify(&token, 900.0), Err(AuthFailure::Rejected));
    }

    /// GAP-212: a token can be revoked alone, or with every other its operator holds;
    /// another operator's is untouched.
    #[test]
    fn a_revoked_token_is_refused() {
        let issuer = issuer();
        let one = issuer.mint(&session(), 0.0).expect("minted");
        let two = issuer.mint(&session(), 0.0).expect("minted");
        let other = issuer
            .mint(
                &OperatorSession {
                    operator: OperatorId(8),
                    ..session()
                },
                0.0,
            )
            .expect("minted");
        assert!(issuer.revoke(&one));
        assert!(!issuer.revoke(&one), "already gone");
        assert_eq!(issuer.verify(&one, 1.0), Err(AuthFailure::Rejected));
        assert!(issuer.verify(&two, 1.0).is_ok());
        assert_eq!(issuer.revoke_operator(OperatorId(7)), 1);
        assert_eq!(issuer.verify(&two, 1.0), Err(AuthFailure::Rejected));
        assert!(issuer.verify(&other, 1.0).is_ok());
    }

    /// GAP-212: a token is believed only by the issuer that minted it. The same key in a
    /// restarted node verifies the MAC and still refuses the token.
    #[test]
    fn a_restarted_issuer_believes_no_earlier_token() {
        let token = issuer().mint(&session(), 0.0).expect("minted");
        assert_eq!(issuer().verify(&token, 1.0), Err(AuthFailure::Rejected));
    }

    /// GAP-212: the deployment's secret is not the key, a short secret is refused, and
    /// two issuers from one secret agree on the key.
    #[test]
    fn the_key_is_derived_from_the_secret() {
        let secret = b"a-deployment-secret-of-adequate-length-01";
        assert!(TokenIssuer::from_secret(&secret[..31], 60.0).is_err());
        let derived = TokenIssuer::from_secret(secret, 60.0).expect("issuer");
        assert_ne!(*derived.key, secret.to_vec());
        assert_eq!(derived.key.len(), 32);
        let again = TokenIssuer::from_secret(secret, 60.0).expect("issuer");
        assert_eq!(*derived.key, *again.key);
        assert!(TokenIssuer::new(vec![1u8; 32], SESSION_MAX_S + 1.0).is_err());
    }

    #[test]
    fn hex_round_trips() {
        let bytes = vec![0x00, 0x0f, 0xa5, 0xff];
        assert_eq!(hex(&bytes), "000fa5ff");
        assert_eq!(unhex("000fa5ff"), Some(bytes));
        assert_eq!(unhex("odd"), None);
        assert_eq!(unhex("zz"), None);
    }
}

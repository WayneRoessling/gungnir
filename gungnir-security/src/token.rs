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

use crate::session::{AuthFailure, MissionTimeSeconds, OperatorSession};
use crate::{OperatorId, Role, SecurityError};
use hmac::{Mac, SimpleHmac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = SimpleHmac<Sha256>;

/// The claims a token carries. Deliberately small: identity and lifetime, nothing else.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct Claims {
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
    key: Vec<u8>,
    lifetime_s: MissionTimeSeconds,
}

impl std::fmt::Debug for TokenIssuer {
    /// Never prints the key. A `Debug` that leaked the signing secret into a log would
    /// undo the whole module.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenIssuer")
            .field("lifetime_s", &self.lifetime_s)
            .finish_non_exhaustive()
    }
}

impl TokenIssuer {
    /// # Errors
    ///
    /// When the key is too short to be worth using, or the lifetime is not positive.
    /// Both are refused rather than defaulted: a token signed with an empty key, or one
    /// that never expires, is worse than a node that will not start.
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
        Ok(Self { key, lifetime_s })
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
        let claims = Claims {
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
        let (payload_hex, mac_hex) = token.split_once('.').ok_or(AuthFailure::Rejected)?;
        let payload = unhex(payload_hex).ok_or(AuthFailure::Rejected)?;
        let presented = unhex(mac_hex).ok_or(AuthFailure::Rejected)?;

        // Constant-time, so the comparison does not leak how much of a forged MAC was
        // right. `ct_eq` on differing lengths is false without short-circuiting.
        // A key the MAC will not take is an authenticator that cannot verify anything,
        // which is a rejection of every token rather than a panic in the request path.
        let expected = self.mac(&payload).map_err(|_| AuthFailure::Rejected)?;
        if expected.ct_eq(&presented).unwrap_u8() != 1 {
            return Err(AuthFailure::Rejected);
        }

        // Only now is the payload worth reading: an unauthenticated payload is attacker
        // input, and deserializing it before the MAC check would be parsing what an
        // attacker chose.
        let claims: Claims = serde_json::from_slice(&payload).map_err(|_| AuthFailure::Rejected)?;
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

    /// HMAC accepts a key of any length, so the error is unreachable in practice; it is
    /// still returned rather than unwrapped, because the unwrap policy (CONTRIBUTING,
    /// checked by `gungnir-app/tests/architecture_compliance.rs`) allows no exception
    /// on a request path, and a panic here would take the transport down with it.
    /// **Signed by the owner 2026-09-06** (GAP-081).
    fn mac(&self, payload: &[u8]) -> Result<Vec<u8>, hmac::digest::InvalidLength> {
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&self.key)?;
        mac.update(payload);
        Ok(mac.finalize().into_bytes().to_vec())
    }
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
        let recovered = issuer()
            .verify(&issuer().mint(&session(), 5.0).expect("minted"), 5.0)
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

    #[test]
    fn hex_round_trips() {
        let bytes = vec![0x00, 0x0f, 0xa5, 0xff];
        assert_eq!(hex(&bytes), "000fa5ff");
        assert_eq!(unhex("000fa5ff"), Some(bytes));
        assert_eq!(unhex("odd"), None);
        assert_eq!(unhex("zz"), None);
    }
}

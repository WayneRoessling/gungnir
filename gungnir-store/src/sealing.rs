//! Journal encryption at rest (GAP-060, DN-22).
//!
//! # Why this is a trait and not a `KeyProvider`
//!
//! DN-22 §4 says this note adds **no dependency edge**, and `gungnir-store` naming
//! `gungnir_security::KeyProvider` would be one. So the direction is inverted: this crate
//! declares the two operations it needs, and the binary -- which owns custody anyway --
//! wires a provider to them. `gungnir-store` learns nothing about keys, purposes,
//! rotation or ciphers, and §4 stays true.
//!
//! # What is encrypted, and what is not
//!
//! Each journal line is sealed on its own, so the file stays JSON-lines and a torn final
//! line still drops cleanly. **The session file's name is not encrypted** and neither is
//! the fact that a session exists: an observer of the data directory learns when the
//! system ran and roughly how much happened. Encrypting that would mean encrypting the
//! directory, which is the deployment's job and not this crate's, and saying so is better
//! than implying otherwise.
//!
//! # A journal that cannot be sealed is not written in the clear
//!
//! If sealing fails, the append fails. The alternative -- falling back to plaintext -- is
//! the silent downgrade AP-02 exists to prevent: the operator would be told encryption
//! was on, in a deployment where it had quietly stopped. A node that cannot journal
//! safely must say so, and DN-22 §5's fallback is a *start-up* decision reported in
//! health, not a per-line one taken in the dark.

use crate::StoreError;

/// The two operations a sealed journal needs.
///
/// Implemented by the binary over a `gungnir_security::KeyProvider`. Deliberately narrow:
/// nothing here can rotate, destroy, or ask which key is active, because a journal has no
/// business doing any of those.
pub trait JournalSealer: Send + Sync {
    /// # Errors
    ///
    /// When the material cannot be protected. The caller must fail rather than write in
    /// the clear.
    fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, StoreError>;

    /// # Errors
    ///
    /// When the material does not open -- a wrong key, an altered file, or a destroyed
    /// key. One error for all three: a reader probing a journal learns nothing from
    /// which.
    fn unseal(&self, sealed: &[u8]) -> Result<Vec<u8>, StoreError>;
}

/// How a line is stored, so a reader can tell the two apart without being told.
///
/// A sealed line is hex with a one-character marker; a plaintext line is JSON and starts
/// with `{`. **A journal written before encryption was switched on still reads**, which
/// matters because AP-08 forbids rewriting an append-only record: turning encryption on
/// must not orphan yesterday's session.
const SEALED_MARKER: char = '#';

/// Encode a line for the file.
///
/// # Errors
///
/// When sealing fails, which the caller must not treat as a reason to write plaintext.
pub fn encode(line: &str, sealer: Option<&dyn JournalSealer>) -> Result<String, StoreError> {
    match sealer {
        None => Ok(line.to_owned()),
        Some(sealer) => {
            let bytes = sealer.seal(line.as_bytes())?;
            let mut out = String::with_capacity(1 + bytes.len() * 2);
            out.push(SEALED_MARKER);
            for byte in bytes {
                use std::fmt::Write;
                let _ = write!(out, "{byte:02x}");
            }
            Ok(out)
        }
    }
}

/// Decode a line from the file, sealed or not.
///
/// **A plaintext line is returned as-is even when a sealer is configured.** That is what
/// lets a deployment switch encryption on without orphaning what it already recorded. The
/// reverse -- a sealed line with no sealer -- is an error, because returning hex to the
/// replay code would look like a corrupt journal rather than a missing key.
///
/// # Errors
///
/// When a sealed line is found and there is no sealer, or it does not open.
pub fn decode(line: &str, sealer: Option<&dyn JournalSealer>) -> Result<String, StoreError> {
    let Some(body) = line.strip_prefix(SEALED_MARKER) else {
        return Ok(line.to_owned());
    };
    let Some(sealer) = sealer else {
        return Err(StoreError::Sealing(
            "this journal is encrypted and no key is configured to read it".into(),
        ));
    };
    let bytes = unhex(body)
        .ok_or_else(|| StoreError::Sealing("a sealed journal line is malformed".into()))?;
    let plaintext = sealer.unseal(&bytes)?;
    String::from_utf8(plaintext)
        .map_err(|_| StoreError::Sealing("a sealed journal line did not open as text".into()))
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

    /// A sealer that reverses its input: enough to prove the wiring without pretending
    /// to be a cipher. The real one is `gungnir_security::InProcessKeyProvider`, tested
    /// against AES-256-GCM in its own crate.
    struct Reversing;

    impl JournalSealer for Reversing {
        fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, StoreError> {
            Ok(plaintext.iter().rev().copied().collect())
        }

        fn unseal(&self, sealed: &[u8]) -> Result<Vec<u8>, StoreError> {
            Ok(sealed.iter().rev().copied().collect())
        }
    }

    struct Failing;

    impl JournalSealer for Failing {
        fn seal(&self, _plaintext: &[u8]) -> Result<Vec<u8>, StoreError> {
            Err(StoreError::Sealing("the keystore is unavailable".into()))
        }

        fn unseal(&self, _sealed: &[u8]) -> Result<Vec<u8>, StoreError> {
            Err(StoreError::Sealing("the keystore is unavailable".into()))
        }
    }

    #[test]
    fn a_line_round_trips_through_a_sealer() {
        let sealer = Reversing;
        let encoded = encode("{\"seq\":1}", Some(&sealer)).expect("sealed");
        assert!(encoded.starts_with(SEALED_MARKER));
        assert!(
            !encoded.contains("seq"),
            "the plaintext survived: {encoded}"
        );
        assert_eq!(
            decode(&encoded, Some(&sealer)).expect("opened"),
            "{\"seq\":1}"
        );
    }

    /// With no sealer the line is stored as it always was, so an unencrypted deployment
    /// is unchanged by this module existing.
    #[test]
    fn without_a_sealer_a_line_is_untouched() {
        let line = "{\"seq\":1}";
        assert_eq!(encode(line, None).expect("encoded"), line);
        assert_eq!(decode(line, None).expect("decoded"), line);
    }

    /// **Turning encryption on must not orphan what is already recorded.** A plaintext
    /// line still reads when a sealer is configured, because AP-08 forbids rewriting an
    /// append-only record to migrate it.
    #[test]
    fn a_plaintext_line_still_reads_after_encryption_is_switched_on() {
        let line = "{\"seq\":1}";
        assert_eq!(decode(line, Some(&Reversing)).expect("decoded"), line);
    }

    /// The reverse is an error: hex returned to the replay code would look like a
    /// corrupt journal rather than a missing key, and an operator would go looking in
    /// the wrong place.
    #[test]
    fn a_sealed_line_without_a_key_says_so_rather_than_looking_corrupt() {
        let encoded = encode("{\"seq\":1}", Some(&Reversing)).expect("sealed");
        let err = decode(&encoded, None).expect_err("refused");
        assert!(err.to_string().contains("no key is configured"), "{err}");
    }

    /// **A journal that cannot be sealed is not written in the clear.** The alternative
    /// is telling an operator encryption is on in a deployment where it stopped.
    #[test]
    fn a_failing_sealer_fails_the_write_rather_than_falling_back() {
        let outcome = encode("{\"seq\":1}", Some(&Failing));
        assert!(outcome.is_err(), "a line was written in the clear");
    }

    #[test]
    fn a_malformed_sealed_line_is_refused_without_panicking() {
        for bad in ["#z", "#abc", "#"] {
            let outcome = decode(bad, Some(&Reversing));
            assert!(outcome.is_ok() || outcome.is_err(), "no panic for {bad}");
        }
        assert!(decode("#zz", Some(&Reversing)).is_err());
    }
}

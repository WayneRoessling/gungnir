// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Reading a 128-bit identity that an older journal wrote as a JSON number (GAP-175,
//! decision D-101).
//!
//! # Why this exists
//!
//! From GAP-069 to GAP-175 a `gungnir_model::identity::GlobalEntityId` was written as its
//! `u128`, a JSON number up to 39 digits long. It is written as RFC 9562 text now, and the
//! owner's decision is that **both forms read**: every journal written in between holds
//! numbers, and nothing is rewritten.
//!
//! **The number cannot be read by the identity's own `Deserialize`.** `serde_json` hands a
//! self-describing reader an integer wider than 64 bits as an `f64`, whose low bits are
//! already gone; a reader that asks for a `u128` instead is refused a string, so no one
//! type can accept both and choose after seeing the token. Inside an externally tagged
//! event the old derive asked for a `u128` and got it exactly, which is why the old lines
//! read until now; once the identity accepts text as well, only the raw digits can give
//! the old value back. `serde_json`'s `arbitrary_precision` feature would keep them, but
//! Cargo unifies features across the workspace, so it would change how every crate's
//! numbers are buffered -- it is the feature D-60 already refused for breaking field-tagged
//! enums.
//!
//! # What it does
//!
//! [`identities_as_text`] scans a line's JSON once, outside strings, for a non-negative
//! integer token wider than 64 bits and at most 128, and writes each one as the hyphenated
//! UUID string the identity writes now, read from its digits exactly. Every other byte is
//! left as it was, and a line holding no such token is returned borrowed, untouched.
//!
//! **Why any such token is an identity.** Nothing in this workspace writes any other
//! integer that wide: every `f64` is written with a point or an exponent, every other
//! integer field is 64 bits or fewer, and the record identifiers have been text since
//! D-60, which also reads a hyphenated string. A negative integer, a fraction and an
//! exponent are never touched.
//!
//! [`crate::nonfinite::from_line`] calls it on every journal line and every v3 frame and
//! body it reads, marked or plain, so the journal, the node's history and the desktop's
//! link all read an old identity the same way.

use std::borrow::Cow;

/// The line with every integer token wider than 64 bits and at most 128 written as the
/// hyphenated UUID string it stands for (D-101); borrowed and unchanged when it holds none.
#[must_use]
pub fn identities_as_text(json: &str) -> Cow<'_, str> {
    let bytes = json.as_bytes();
    let mut rewritten: Option<String> = None;
    // How much of `json` has been copied into `rewritten`.
    let mut copied = 0;
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'"' => at = past_string(bytes, at + 1),
            // A negative number is never an identity; step over the whole token so its
            // digits are not read as one.
            b'-' => at = past_number(bytes, at + 1),
            b'0'..=b'9' => {
                let start = at;
                while at < bytes.len() && bytes[at].is_ascii_digit() {
                    at += 1;
                }
                if at < bytes.len() && matches!(bytes[at], b'.' | b'e' | b'E') {
                    // A fraction or an exponent: a float, whatever its length.
                    at = past_number(bytes, at);
                    continue;
                }
                if let Some(value) = wide(&json[start..at]) {
                    let out = rewritten.get_or_insert_with(|| String::with_capacity(json.len()));
                    out.push_str(&json[copied..start]);
                    out.push('"');
                    out.push_str(&uuid_text(value));
                    out.push('"');
                    copied = at;
                }
            }
            _ => at += 1,
        }
    }
    match rewritten {
        None => Cow::Borrowed(json),
        Some(mut out) => {
            out.push_str(&json[copied..]);
            Cow::Owned(out)
        }
    }
}

/// The index just past the string whose content begins at `at`.
fn past_string(bytes: &[u8], mut at: usize) -> usize {
    while at < bytes.len() {
        match bytes[at] {
            // An escape: whatever follows is part of the string, a quote included.
            b'\\' => at += 2,
            b'"' => return at + 1,
            _ => at += 1,
        }
    }
    at
}

/// The index just past the rest of a number token.
fn past_number(bytes: &[u8], mut at: usize) -> usize {
    while at < bytes.len() && matches!(bytes[at], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-') {
        at += 1;
    }
    at
}

/// The digits' value when it is wider than 64 bits and fits in 128.
fn wide(digits: &str) -> Option<u128> {
    // `u64::MAX` has 20 digits, so anything shorter fits in 64 bits.
    if digits.len() < 20 {
        return None;
    }
    let value = digits.parse::<u128>().ok()?;
    (value > u128::from(u64::MAX)).then_some(value)
}

/// The hyphenated lowercase form the identity writes.
fn uuid_text(value: u128) -> String {
    gungnir_model::identity::GlobalEntityId(value).to_string()
}

#[cfg(test)]
mod tests {
    use super::identities_as_text;
    use std::borrow::Cow;

    /// A v7 identity as the desktop wrote it before GAP-175, and its text.
    const OLD: &str = "2164528803482661774963721757477223390";
    const TEXT: &str = "01a0df82-6bb1-75d1-9e86-4109de36bbde";

    #[test]
    fn a_wide_integer_becomes_the_identity_s_text_exactly() {
        let line = format!(r#"{{"Minted":{{"track":1,"entity":{OLD},"at":100.0}}}}"#);
        assert_eq!(
            identities_as_text(&line),
            format!(r#"{{"Minted":{{"track":1,"entity":"{TEXT}","at":100.0}}}}"#)
        );
        // At the very end of the input, and as an array element.
        assert_eq!(identities_as_text(OLD), format!("\"{TEXT}\""));
        assert_eq!(
            identities_as_text(&format!("[{OLD},{OLD}]")),
            format!("[\"{TEXT}\",\"{TEXT}\"]")
        );
    }

    /// Everything that is not a wide non-negative integer is left exactly as it was, and
    /// a line with nothing to change is not copied.
    #[test]
    fn nothing_else_is_touched() {
        for line in [
            r#"{"seq":18446744073709551615,"t":1.0}"#, // u64::MAX: a 64-bit integer
            r#"{"f":1e40,"g":2164528803482661774963721757477223390.5,"h":1E+39}"#,
            r#"{"n":-2164528803482661774963721757477223390}"#, // negative
            r#"{"s":"2164528803482661774963721757477223390"}"#, // already a string
            r#"{"s":"an \"escaped\" 2164528803482661774963721757477223390 \\"}"#,
            r#"{"big":1000000000000000000000000000000000000000}"#, // wider than 128 bits
            "~{\"x\":\"\\u0000f64:7ff8000000000000\"}",
            "",
        ] {
            let out = identities_as_text(line);
            assert!(matches!(out, Cow::Borrowed(_)), "{line} was copied");
            assert_eq!(out, line);
        }
    }

    /// A string ending in an escaped backslash is closed where it ends, so an identity
    /// after it is still found.
    #[test]
    fn a_string_ending_in_an_escaped_backslash_does_not_swallow_what_follows() {
        let line = format!(r#"{{"basis":"a\\","entity":{OLD}}}"#);
        assert_eq!(
            identities_as_text(&line),
            format!(r#"{{"basis":"a\\","entity":"{TEXT}"}}"#)
        );
    }
}

// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Append-only journal framing: one JSON object per line (`.jsonl`), one file per
//! session, named `session-<zero-padded id>.jsonl`. Kept separate from the
//! `EventJournal` trait in lib.rs so the on-disk format can change (checksums,
//! rotation, compression) without touching the public interface.

use crate::SessionId;
use gungnir_eventing::Envelope;

pub const FILE_EXTENSION: &str = "jsonl";
const FILE_PREFIX: &str = "session-";

/// `session-000000000042.jsonl` -- zero-padded so lexical order is numeric order.
pub fn session_file_name(session: SessionId) -> String {
    format!("{FILE_PREFIX}{:012}.{FILE_EXTENSION}", session.0)
}

/// Inverse of [`session_file_name`]; `None` for any file that is not a journal.
pub fn parse_session_file_name(name: &str) -> Option<SessionId> {
    let stem = name
        .strip_prefix(FILE_PREFIX)?
        .strip_suffix(&format!(".{FILE_EXTENSION}"))?;
    stem.parse::<u64>().ok().map(SessionId)
}

/// Suffix a session file carries while retention removes it (D-78).
///
/// The rename to this name is the one step that takes a session out of the journal's
/// listing, so a purge interrupted at any point leaves a session either whole or gone,
/// never half-listed. [`parse_session_file_name`] does not read it.
pub const PURGING_SUFFIX: &str = "purging";
/// Extension of a retention hold (D-78): `session-<id>.hold`, whose text is the reason.
pub const HOLD_EXTENSION: &str = "hold";

/// `session-000000000042.jsonl.purging`.
pub fn purging_file_name(session: SessionId) -> String {
    format!("{}.{PURGING_SUFFIX}", session_file_name(session))
}

/// Inverse of [`purging_file_name`].
pub fn parse_purging_file_name(name: &str) -> Option<SessionId> {
    parse_session_file_name(name.strip_suffix(&format!(".{PURGING_SUFFIX}"))?)
}

/// `session-000000000042.hold`.
pub fn hold_file_name(session: SessionId) -> String {
    format!("{FILE_PREFIX}{:012}.{HOLD_EXTENSION}", session.0)
}

/// Inverse of [`hold_file_name`].
pub fn parse_hold_file_name(name: &str) -> Option<SessionId> {
    let stem = name
        .strip_prefix(FILE_PREFIX)?
        .strip_suffix(&format!(".{HOLD_EXTENSION}"))?;
    stem.parse::<u64>().ok().map(SessionId)
}

/// One envelope as a single line of JSON (no embedded newlines).
///
/// An envelope whose floats are all finite is written exactly as `serde_json` writes it.
/// One that carries a NaN or an infinity is written in the marked, lossless form of
/// [`crate::nonfinite`] (GAP-126, D-77), because plain JSON would write `null` and the
/// line could not be read back.
pub fn encode_line(envelope: &Envelope) -> Result<String, serde_json::Error> {
    crate::nonfinite::to_line(envelope)
}

/// The inverse of [`encode_line`], for both forms.
pub fn decode_line(line: &str) -> Result<Envelope, serde_json::Error> {
    crate::nonfinite::from_line(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_round_trip_and_sort_numerically() {
        let a = session_file_name(SessionId(9));
        let b = session_file_name(SessionId(10));
        assert!(a < b, "zero padding must keep lexical order numeric");
        assert_eq!(parse_session_file_name(&a), Some(SessionId(9)));
        assert_eq!(parse_session_file_name("notes.txt"), None);
        assert_eq!(parse_session_file_name("session-x.jsonl"), None);
    }

    #[test]
    fn purging_and_hold_names_are_never_read_as_sessions() {
        let s = SessionId(42);
        assert_eq!(parse_session_file_name(&purging_file_name(s)), None);
        assert_eq!(parse_session_file_name(&hold_file_name(s)), None);
        assert_eq!(parse_purging_file_name(&purging_file_name(s)), Some(s));
        assert_eq!(parse_hold_file_name(&hold_file_name(s)), Some(s));
        assert_eq!(parse_purging_file_name(&session_file_name(s)), None);
        assert_eq!(parse_hold_file_name(&session_file_name(s)), None);
    }
}

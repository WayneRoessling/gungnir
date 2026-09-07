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

/// One envelope as a single line of JSON (no embedded newlines).
pub fn encode_line(envelope: &Envelope) -> Result<String, serde_json::Error> {
    serde_json::to_string(envelope)
}

pub fn decode_line(line: &str) -> Result<Envelope, serde_json::Error> {
    serde_json::from_str(line)
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
}

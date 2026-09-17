// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! How a minted record identifier is written, read and shown (GAP-130): [`crate::DecisionId`]
//! and [`crate::PlanId`] here, and `gungnir_command::PendingApprovalId` through the same
//! functions, so the three cannot come to disagree about any of it.
//!
//! **Held as a `u128`, minted as UUID v7 where the thing is created** (D-56,
//! `docs/design/DN-31-node-approval-queue.md` §5.1). The counters these replaced restarted
//! at 1 in every process, so two desktops on one node, or one desktop across a restart,
//! minted the same identifiers, and an effector report found every handoff with its
//! number. Minting is not done here: this crate takes `uuid` without the `v7` feature,
//! which D-11 admits only where identities are minted.
//!
//! # Written as a UUID string, read from a string or a number (D-60)
//!
//! [`wire`] writes the hyphenated RFC 9562 form and reads either that form or a JSON
//! integer. **A number is not written**, although it was the first plan: a 128-bit JSON
//! number fails `serde_json::to_value`, loses its low digits when parsed into a
//! `serde_json::Value`, and cannot be read inside a field-tagged enum, whose buffer holds
//! nothing wider than 64 bits. The handoff posted to an effector and the handoff body
//! published for exchange both pass through a `Value`, and both became `null` with a
//! numeric v7 identifier (DN-31 §12). Reading a number stays, because every journal
//! written before GAP-130 holds one.
//!
//! # Shown as a short tag, recorded in full (D-61)
//!
//! [`short`] is what a panel or an alert shows: the last eight hex digits, `…9f3a61c2`.
//! The last digits because a v7 identifier starts with its millisecond timestamp, so
//! identifiers minted in one session share their leading digits and differ at the end.
//! [`fmt_full`] is the whole identifier, for an audit entry, a log field, PN-07 and
//! anything else a person may need to quote or search for.
//!
//! **A value that fits in a `u64` is a number, not a UUID, and is shown as one.** Every
//! UUID with a version carries a non-zero version nibble in its upper 64 bits, so no
//! minted identifier fits in a `u64`, and every value that does is a pre-change counter or
//! an identifier a rehearsal seed gave (`P-1183`), which D-61 shows as it was given.

use std::fmt;

/// Why a text is not an identifier.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "{text:?} is not an identifier: expected the hyphenated UUID form \
     (xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx) or a decimal number"
)]
pub struct IdentifierError {
    pub text: String,
}

/// Read an identifier from text: the hyphenated UUID form, or a decimal number.
///
/// Exactly those two. The hyphenated form is what D-60 writes; a decimal number is what a
/// client written before GAP-130 puts in a path, and reading it gives that client an answer
/// about its identifier rather than a refusal about its syntax. The 32-digit simple UUID
/// form is refused because it can be all decimal digits, and a text that reads as two
/// different identifiers is not an identifier.
///
/// # Errors
///
/// [`IdentifierError`] for any other text.
pub fn parse(text: &str) -> Result<u128, IdentifierError> {
    let refused = || IdentifierError {
        text: text.to_owned(),
    };
    if text.len() == 36 {
        // 36 characters is the hyphenated form and no other: simple is 32, braced 38, URN 45.
        return uuid::Uuid::try_parse(text)
            .map(|id| id.as_u128())
            .map_err(|_| refused());
    }
    if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) {
        return text.parse::<u128>().map_err(|_| refused());
    }
    Err(refused())
}

/// The whole identifier: the hyphenated UUID form, or the number a pre-change counter or a
/// rehearsal seed gave (see the module documentation).
///
/// # Errors
///
/// Only the formatter's own.
pub fn fmt_full(value: u128, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match u64::try_from(value) {
        Ok(number) => write!(f, "{number}"),
        Err(_) => write!(f, "{}", uuid::Uuid::from_u128(value).hyphenated()),
    }
}

/// The on-screen tag: `…` and the last eight hex digits, or the number itself (D-61).
#[must_use]
pub fn short(value: u128) -> String {
    match u64::try_from(value) {
        Ok(number) => number.to_string(),
        Err(_) => format!("\u{2026}{:08x}", value & 0xffff_ffff),
    }
}

/// The written form of an identifier's `u128` (D-60): what each identifier's `Serialize`
/// and `Deserialize` call, written out on the type rather than attached as
/// `#[serde(with)]`, so the declaration stays `pub struct DecisionId(pub u128);` -- the
/// shape DN-31 §5.1 shows and `docs/architecture/uaf/tools/build_uaf.py` reads.
pub mod wire {
    use serde::de::{Error, Unexpected, Visitor};
    use serde::{Deserializer, Serializer};

    /// The hyphenated RFC 9562 form, always, including for a value a counter or a seed gave:
    /// one written form, so a reader never has to guess which kind of writer it has.
    ///
    /// # Errors
    ///
    /// Only the serializer's own.
    #[allow(clippy::trivially_copy_pass_by_ref)] // serde's `serialize_with` shape, by reference
    pub fn serialize<S: Serializer>(value: &u128, serializer: S) -> Result<S::Ok, S::Error> {
        let mut buffer = uuid::Uuid::encode_buffer();
        serializer.serialize_str(
            uuid::Uuid::from_u128(*value)
                .hyphenated()
                .encode_lower(&mut buffer),
        )
    }

    /// Either written form, through `deserialize_any`, which is what lets an identifier be
    /// read out of a `serde_json::Value`, a field-tagged enum's buffer and an axum path.
    ///
    /// # Errors
    ///
    /// Anything but a hyphenated UUID string, a decimal string or a non-negative integer. An
    /// integer too wide for 64 bits reaches this as a float, which no writer of this crate
    /// ever produced, and is refused rather than rounded.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u128, D::Error> {
        deserializer.deserialize_any(IdentifierVisitor)
    }

    struct IdentifierVisitor;

    impl Visitor<'_> for IdentifierVisitor {
        type Value = u128;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(
                "an identifier: a hyphenated UUID string, or the non-negative integer a \
                 journal written before GAP-130 holds",
            )
        }

        fn visit_u64<E: Error>(self, value: u64) -> Result<u128, E> {
            Ok(u128::from(value))
        }

        fn visit_u128<E: Error>(self, value: u128) -> Result<u128, E> {
            Ok(value)
        }

        fn visit_i64<E: Error>(self, value: i64) -> Result<u128, E> {
            u128::try_from(value).map_err(|_| E::invalid_value(Unexpected::Signed(value), &self))
        }

        fn visit_str<E: Error>(self, text: &str) -> Result<u128, E> {
            super::parse(text).map_err(|_| E::invalid_value(Unexpected::Str(text), &self))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse, short};
    use crate::{DecisionId, PlanId};

    /// A v7-shaped value: a 2025 millisecond timestamp, version 7, the RFC variant.
    const V7: u128 = 0x0199_5a3b_7c2d_7e4f_8a1b_2c3d_9f3a_61c2;
    const V7_TEXT: &str = "01995a3b-7c2d-7e4f-8a1b-2c3d9f3a61c2";

    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    #[serde(tag = "state", rename_all = "kebab-case")]
    enum Tagged {
        Decided { decision: DecisionId, plan: PlanId },
    }

    /// D-60: written as the hyphenated string, and read back to the same 128 bits.
    #[test]
    fn an_identifier_is_written_as_the_hyphenated_uuid_and_reads_back() {
        let json = serde_json::to_string(&DecisionId(V7)).expect("encodes");
        assert_eq!(json, format!("\"{V7_TEXT}\""));
        assert_eq!(
            serde_json::from_str::<DecisionId>(&json).expect("decodes"),
            DecisionId(V7)
        );
    }

    /// D-60, the first of the paths a 128-bit number broke: `serde_json::to_value` refused
    /// it with "number out of range", and the effector's payload became `null`.
    #[test]
    fn an_identifier_survives_to_value() {
        let value = serde_json::to_value(PlanId(V7)).expect("to_value accepts it");
        assert_eq!(value, serde_json::Value::String(V7_TEXT.to_owned()));
        assert_eq!(
            serde_json::from_value::<PlanId>(value).expect("from_value"),
            PlanId(V7)
        );
    }

    /// D-60, the second: a document parsed into a `Value` first, as the node parses an
    /// exchange body, turned a 128-bit number into a float and lost the low digits.
    #[test]
    fn an_identifier_survives_being_parsed_into_a_value() {
        let text = serde_json::json!({ "decision": DecisionId(V7) }).to_string();
        let value: serde_json::Value = serde_json::from_str(&text).expect("parses");
        assert_eq!(value.to_string(), text, "the Value changed the document");
        let decision = serde_json::from_value::<DecisionId>(value["decision"].clone())
            .expect("reads out of the Value");
        assert_eq!(decision, DecisionId(V7));
    }

    /// D-60, the third: a field-tagged enum buffers its fields, and the buffer holds no
    /// integer wider than 64 bits, so a numeric v7 identifier inside one could not be read.
    #[test]
    fn an_identifier_reads_inside_a_field_tagged_enum() {
        let event = Tagged::Decided {
            decision: DecisionId(V7),
            plan: PlanId(V7 + 1),
        };
        let text = serde_json::to_string(&event).expect("encodes");
        assert_eq!(
            serde_json::from_str::<Tagged>(&text).expect("decodes inside the tag"),
            event
        );
        // And an old number inside the same enum, as a pre-change line would hold it.
        let old: Tagged =
            serde_json::from_str(r#"{"state":"decided","decision":1,"plan":3}"#).expect("reads");
        assert_eq!(
            old,
            Tagged::Decided {
                decision: DecisionId(1),
                plan: PlanId(3)
            }
        );
    }

    /// **A journal written before GAP-130 still reads**: its identifiers are numbers, and a
    /// number reads as the same value, which is then written in the new form.
    #[test]
    fn a_number_written_before_the_change_reads_as_the_same_identifier() {
        let old = serde_json::from_str::<DecisionId>("7").expect("a number reads");
        assert_eq!(old, DecisionId(7));
        assert_eq!(
            serde_json::to_string(&old).expect("encodes"),
            "\"00000000-0000-0000-0000-000000000007\""
        );
        assert!(serde_json::from_str::<DecisionId>("-7").is_err());
        assert!(
            serde_json::from_str::<DecisionId>("2125479544897800857153712946870708859").is_err(),
            "a 128-bit number was never written, and reading it through a float would round it"
        );
        assert!(serde_json::from_str::<DecisionId>("7.5").is_err());
    }

    /// The two text forms, and nothing that could be read two ways.
    #[test]
    fn the_text_forms_are_the_hyphenated_uuid_and_a_decimal_number() {
        assert_eq!(parse(V7_TEXT), Ok(V7));
        assert_eq!(parse(&V7_TEXT.to_uppercase()), Ok(V7));
        assert_eq!(parse("1183"), Ok(1183));
        assert_eq!(parse("00000000-0000-0000-0000-000000000007"), Ok(7));
        for refused in [
            "",
            "+7",
            "-7",
            "01995a3b7c2d7e4f8a1b2c3d9f3a61c2",
            "{01995a3b-7c2d-7e4f-8a1b-2c3d9f3a61c2}",
            "urn:uuid:01995a3b-7c2d-7e4f-8a1b-2c3d9f3a61c2",
            "not-an-identifier",
        ] {
            let err = parse(refused).expect_err(refused);
            assert!(err.to_string().contains("hyphenated"), "{err}");
        }
    }

    /// D-61: the tag is the random end of a minted identifier; the full form is all of it;
    /// a number is shown as the number it is.
    #[test]
    fn a_minted_identifier_is_tagged_by_its_last_digits_and_a_number_by_itself() {
        assert_eq!(short(V7), "\u{2026}9f3a61c2");
        assert_eq!(DecisionId(V7).to_string(), V7_TEXT);
        assert_eq!(PlanId(1183).short(), "1183");
        assert_eq!(PlanId(1183).to_string(), "1183");
        assert_eq!(short(u128::from(u64::MAX)), u64::MAX.to_string());
        assert_eq!(short(u128::from(u64::MAX) + 1), "\u{2026}00000000");
        assert_eq!(V7_TEXT.parse::<PlanId>(), Ok(PlanId(V7)));
    }
}

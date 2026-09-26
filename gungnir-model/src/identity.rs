// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Global entity identity primitive, extended by gungnir-identity into full
//! lineage/merge-split tracking.
//!
//! **The newtype is a UUID** as of GAP-069 (D-11). It stayed a bare
//! `u128` until then because a new dependency needs a sign-off, and the comment that used
//! to stand here said `uuid` could be adopted "later behind this same newtype". That is
//! what happened: the representation did not change, and everything that stored or
//! serialized a `GlobalEntityId` as 128 bits still does.
//!
//! # Why the textual form lives here
//!
//! An identity crosses a wire. A peer that receives one has to read it, and one it sends
//! has to be written the same way every time -- so **the format is part of what the
//! identity is**, not a detail of whichever crate happens to serialize it. The alternative
//! was a formatting helper in `gungnir-identity`, which would have made every crate that
//! writes an id depend on the crate that mints them.
//!
//! # Written as RFC 9562 text since GAP-175 (D-101)
//!
//! The identity is written as the hyphenated lowercase UUID, the form the schema
//! catalogue's `gungnir.GlobalEntityId` entry has always registered (`gungnir-interop`),
//! through the same [`crate::identifier::wire`] functions D-60 gave the record
//! identifiers, so the two cannot come to disagree. From GAP-069 to GAP-175 the derived
//! `Serialize` wrote the `u128` as a JSON number: `serde_json::to_value` refused any value
//! holding one ("number out of range"), and a reader outside Rust rounded it to a double,
//! which can merge two entities or split one.
//!
//! **Both forms read.** The text, and a number: one that fits in 64 bits (an identity from
//! the counter used before GAP-069) reads here. **A number wider than 64 bits cannot**,
//! because `serde_json` hands one to a self-describing reader as a float, whose low bits
//! are gone before any `Deserialize` sees it; this refuses the float rather than rounding
//! it. A journal line and a v3 frame are read through `gungnir_eventing::nonfinite::
//! from_line`, which reads each such number exactly from its raw digits and gives it to
//! this type as the text it now is (`gungnir_eventing::wide_integers`), so every journal
//! written since GAP-069 still reads, and nothing is rewritten.

/// A globally unique entity identity: a UUID, held as its 128 bits.
///
/// Minted as version 7 by `gungnir-identity`, so the first 48 bits are a millisecond
/// timestamp and identities sort in the order they were created -- which is the order an
/// after-action review reads them in.
/// Ordered, and the order means something: v7 puts a big-endian millisecond timestamp in
/// the most significant 48 bits, so comparing the `u128` compares mint times. An identity
/// from before GAP-069 sorts as the small number it is, which is harmless and visibly odd
/// rather than silently wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GlobalEntityId(pub u128);

impl serde::Serialize for GlobalEntityId {
    /// The hyphenated RFC 9562 string (D-101), as the record identifiers are (D-60).
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        crate::identifier::wire::serialize(&self.0, serializer)
    }
}

impl<'de> serde::Deserialize<'de> for GlobalEntityId {
    /// That string, or a number (D-101): see the module documentation for which numbers,
    /// and where a wider one is read.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        crate::identifier::wire::deserialize(deserializer).map(Self)
    }
}

impl GlobalEntityId {
    /// The identity as a `uuid::Uuid`.
    #[must_use]
    pub fn as_uuid(self) -> uuid::Uuid {
        uuid::Uuid::from_u128(self.0)
    }

    /// The UUID version, when the identity carries one.
    ///
    /// `None` for an identity that is not a well-formed UUID of any version -- which
    /// includes every id minted by the counter this crate used before GAP-069, and which
    /// is why this returns rather than asserting: **a journal recorded before the change
    /// is still a journal**, and it must be readable rather than rejected.
    #[must_use]
    pub fn version(self) -> Option<uuid::Version> {
        self.as_uuid().get_version()
    }

    /// Parse the standard textual form.
    ///
    /// # Errors
    ///
    /// When the text is not a UUID in any of the accepted representations.
    pub fn parse(text: &str) -> Result<Self, uuid::Error> {
        uuid::Uuid::parse_str(text).map(|u| Self(u.as_u128()))
    }
}

impl std::fmt::Display for GlobalEntityId {
    /// The hyphenated lowercase form, which is what goes on a wire and in a report.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_uuid().hyphenated())
    }
}

impl std::str::FromStr for GlobalEntityId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod identity_tests {
    use super::GlobalEntityId;

    /// The textual form round-trips exactly. An identity that changed on the way through a
    /// wire would silently split one entity into two.
    #[test]
    fn the_textual_form_round_trips() {
        let id = GlobalEntityId(0x0192_3f4d_8e2a_7c31_9b5e_1a2c_3d4e_5f60);
        let text = id.to_string();
        assert_eq!(text.len(), 36, "{text}");
        assert_eq!(GlobalEntityId::parse(&text).expect("parses"), id);
        assert_eq!(text.parse::<GlobalEntityId>().expect("FromStr"), id);
    }

    /// **A journal written before GAP-069 is still a journal.** Identities minted by the
    /// old counter are not UUIDs of any version, and they read back rather than being
    /// rejected -- the version accessor says so instead.
    #[test]
    fn an_identity_from_before_the_change_still_reads() {
        let old = GlobalEntityId(7);
        assert_eq!(old.0, 7, "the representation changed");
        assert!(
            old.version().is_none() || old.version() != Some(uuid::Version::SortRand),
            "a counter value was reported as a minted v7 identity"
        );
        // And it still has a textual form, so nothing that writes one has to special-case
        // it.
        assert_eq!(old.to_string().len(), 36);
    }

    /// A v7 identity `gungnir-identity` minted while writing the pre-GAP-175 fixture
    /// (`testdata/journals/pre-gap-175/`). As a double it is
    /// 2164528803482661745094088668257189888, so any path through a float gives a
    /// different entity.
    const MINTED: u128 = 2_164_528_803_482_661_774_963_721_757_477_223_390;

    /// **D-101: written as the text, and it survives a `serde_json::Value`** (GAP-175).
    /// The derived form was a 128-bit number, which `to_value` refused outright.
    #[test]
    fn an_identity_is_written_as_rfc_9562_text_and_survives_a_value() {
        let id = GlobalEntityId(MINTED);
        let json = serde_json::to_string(&id).expect("encodes");
        assert_eq!(json, "\"01a0df82-6bb1-75d1-9e86-4109de36bbde\"");
        assert_eq!(
            serde_json::from_str::<GlobalEntityId>(&json).expect("reads"),
            id
        );
        let value = serde_json::to_value(id).expect("to_value accepts it");
        assert_eq!(value, serde_json::Value::String(id.to_string()));
        assert_eq!(
            serde_json::from_value::<GlobalEntityId>(value).expect("from_value"),
            id
        );
    }

    /// **Both forms read, and a wide number is never rounded** (D-101). A counter's
    /// number reads; a 128-bit number read straight through `serde_json` arrives as a
    /// float and is refused -- `gungnir_eventing::nonfinite::from_line`, which every
    /// journal line and v3 frame goes through, is what reads it exactly.
    #[test]
    // The loss through a double is what this test is about.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    fn a_counter_number_reads_and_a_wide_one_is_refused_rather_than_rounded() {
        assert_eq!(
            serde_json::from_str::<GlobalEntityId>("7").expect("a counter's number"),
            GlobalEntityId(7)
        );
        let rounded = MINTED as f64;
        assert_ne!(
            rounded as u128, MINTED,
            "the value survives a double after all"
        );
        let err = serde_json::from_str::<GlobalEntityId>(&MINTED.to_string())
            .expect_err("a wide number read through a float");
        assert!(err.to_string().contains("identifier"), "{err}");
    }
}

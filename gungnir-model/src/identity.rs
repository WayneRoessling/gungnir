//! Global entity identity primitive, extended by gungnir-identity into full
//! lineage/merge-split tracking.
//!
//! **The newtype is a UUID** as of GAP-069 (D-11, signed 2026-09-04). It stayed a bare
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

/// A globally unique entity identity: a UUID, held as its 128 bits.
///
/// Minted as version 7 by `gungnir-identity`, so the first 48 bits are a millisecond
/// timestamp and identities sort in the order they were created -- which is the order an
/// after-action review reads them in.
/// Ordered, and the order means something: v7 puts a big-endian millisecond timestamp in
/// the most significant 48 bits, so comparing the `u128` compares mint times. An identity
/// from before GAP-069 sorts as the small number it is, which is harmless and visibly odd
/// rather than silently wrong.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct GlobalEntityId(pub u128);

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
}

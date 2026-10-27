// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Releasability: who may receive a piece of data.
//!
//! Design: docs/design/DN-17-releasability.md.
//! Capabilities CAP-6.6 and CAP-7.4; decision D-06 settled that it is modelled now
//! and enforced per caller later.
//!
//! **Releasability is a property of the data, not of the channel.** Channel-based
//! control fails the moment a product is forwarded, which is why the marking travels
//! on the view, the report, and the contract rather than on the connection that
//! carried them.
//!
//! Two rules make the rest safe:
//!
//! 1. [`Releasability::default`] is `Internal`, so anything unmarked stays in.
//! 2. [`Releasability::combine`] takes the **most restrictive** input, so a report
//!    built from several sources cannot launder a restricted track by aggregation.
//!
//! This is **not a classification system** and must not be conflated with one.
//! Everything in this repository is unclassified (AP-04); the marking governs
//! distribution between deployments, which is a different question.

use std::collections::{BTreeMap, BTreeSet};

/// Who may receive this.
///
/// Deliberately small: an expressive marking language nobody configures correctly is
/// worse than a coarse one everybody understands.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Releasability {
    /// Never leaves this deployment.
    #[default]
    Internal,
    /// May go to the named parties and nobody else.
    Parties { parties: BTreeSet<String> },
    /// May go to any authenticated peer.
    AllPeers,
}

impl Releasability {
    /// Convenience for the common case of naming parties.
    pub fn parties<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Releasability::Parties {
            parties: names.into_iter().map(Into::into).collect(),
        }
    }

    /// True when a caller belonging to `party` may receive this.
    ///
    /// A caller whose party could not be established passes an empty name and
    /// receives nothing: there is no anonymous peer.
    pub fn permits(&self, party: &str) -> bool {
        if party.trim().is_empty() {
            return false;
        }
        match self {
            Releasability::Internal => false,
            Releasability::AllPeers => true,
            Releasability::Parties { parties } => parties.contains(party),
        }
    }

    /// How restrictive this marking is, least permissive first.
    fn restriction(&self) -> u8 {
        match self {
            Releasability::AllPeers => 0,
            Releasability::Parties { .. } => 1,
            Releasability::Internal => 2,
        }
    }

    /// The marking of a product derived from several inputs: **the most restrictive
    /// of them, never the least.**
    ///
    /// Two party sets combine to their intersection, because a product built from
    /// both may only go where both may go. An empty intersection is `Internal`
    /// rather than an empty party list, so the result cannot be mistaken for
    /// releasable to nobody in particular.
    ///
    /// Combining nothing yields `Internal`: a product with no inputs has nothing
    /// permitting its release.
    pub fn combine(items: impl IntoIterator<Item = Releasability>) -> Releasability {
        let mut result: Option<Releasability> = None;
        for item in items {
            result = Some(match result {
                None => item,
                Some(current) => Self::pair(current, item),
            });
        }
        result.unwrap_or_default()
    }

    fn pair(a: Releasability, b: Releasability) -> Releasability {
        match (&a, &b) {
            (Releasability::Internal, _) | (_, Releasability::Internal) => Releasability::Internal,
            (Releasability::Parties { parties: x }, Releasability::Parties { parties: y }) => {
                let both: BTreeSet<String> = x.intersection(y).cloned().collect();
                if both.is_empty() {
                    Releasability::Internal
                } else {
                    Releasability::Parties { parties: both }
                }
            }
            _ if a.restriction() >= b.restriction() => a,
            _ => b,
        }
    }
}

/// What each source's contribution may be released as, and so what a product built from
/// some of them is marked (DN-17 §5 and §6; GAP-206).
///
/// DN-17 §5: "a configured sensor carries a default marking that its detections inherit;
/// a peer-sourced track inherits the peer's". A source is the identifier the ingest
/// gateway admits a report under -- a sensor's id, or a peer's `source_id` -- so one map
/// covers both. A source the map does not name takes the deployment's default.
///
/// **A product is marked by [`Releasability::combine`] over the sources that fed it**,
/// so a track one restricted sensor ever touched is restricted for the rest of its life:
/// its estimate carries what that sensor saw. And a product nothing is recorded as
/// feeding is `Internal`, which is `combine`'s own answer for no inputs.
///
/// The default of this type is a deployment that configured nothing: every source
/// `Internal`, which is what every track was before GAP-206.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceMarkings {
    default: Releasability,
    by_source: BTreeMap<u32, Releasability>,
}

impl SourceMarkings {
    /// Every source marked `default` until [`SourceMarkings::with_source`] says otherwise.
    #[must_use]
    pub fn new(default: Releasability) -> Self {
        Self {
            default,
            by_source: BTreeMap::new(),
        }
    }

    /// Mark one source's contribution, replacing the default for it.
    #[must_use]
    pub fn with_source(mut self, source: u32, marking: Releasability) -> Self {
        self.by_source.insert(source, marking);
        self
    }

    /// What `source`'s contribution is marked.
    #[must_use]
    pub fn marking_of(&self, source: u32) -> &Releasability {
        self.by_source.get(&source).unwrap_or(&self.default)
    }

    /// The marking of a product fed by `sources`: the most restrictive of theirs.
    #[must_use]
    pub fn mark(&self, sources: &[u32]) -> Releasability {
        Releasability::combine(sources.iter().map(|s| self.marking_of(*s).clone()))
    }
}

/// Filters a collection for one caller, reporting how many items were withheld.
///
/// **Withholding is reported.** A silently shortened list makes a peer believe they
/// have the whole picture, which is worse than telling them they do not. The
/// single-item case is deliberately different: see [`permits_single`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Filtered<T> {
    pub items: Vec<T>,
    pub withheld: usize,
}

/// Filters `items` for `party`, keeping the count of what was removed.
pub fn filter_for<T>(
    items: Vec<T>,
    party: &str,
    marking: impl Fn(&T) -> &Releasability,
) -> Filtered<T> {
    let total = items.len();
    let kept: Vec<T> = items
        .into_iter()
        .filter(|item| marking(item).permits(party))
        .collect();
    let withheld = total - kept.len();
    Filtered {
        items: kept,
        withheld,
    }
}

/// Whether a caller may receive one named item.
///
/// The caller returns not-found rather than forbidden when this is false, so the
/// existence of a restricted item is not disclosed by the error code. That pulls
/// against the reported withholding above, and both are kept: a collection admits
/// that items were withheld, because the peer needs to know their picture is
/// partial; a single-item request does not, because it would be an oracle for
/// guessing identifiers.
pub fn permits_single(marking: &Releasability, party: &str) -> bool {
    marking.permits(party)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unmarked_item_is_internal_and_goes_nowhere() {
        let default = Releasability::default();
        assert_eq!(default, Releasability::Internal);
        assert!(!default.permits("partner-a"));
    }

    #[test]
    fn a_caller_with_no_party_receives_nothing() {
        // There is no anonymous peer.
        for marking in [
            Releasability::Internal,
            Releasability::AllPeers,
            Releasability::parties(["partner-a"]),
        ] {
            assert!(!marking.permits(""), "{marking:?}");
            assert!(!marking.permits("   "), "{marking:?}");
        }
    }

    #[test]
    fn parties_admit_only_the_named() {
        let m = Releasability::parties(["partner-a", "partner-b"]);
        assert!(m.permits("partner-a"));
        assert!(m.permits("partner-b"));
        assert!(!m.permits("partner-c"));
    }

    #[test]
    fn all_peers_admits_any_named_caller() {
        let m = Releasability::AllPeers;
        assert!(m.permits("partner-a"));
        assert!(m.permits("anyone"));
    }

    #[test]
    fn combine_always_yields_the_most_restrictive_input() {
        // Exhaustive over the small variant set, which is the point of keeping it
        // small: this is the rule an aggregation bug would silently break.
        let internal = Releasability::Internal;
        let parties = Releasability::parties(["partner-a"]);
        let all = Releasability::AllPeers;

        for other in [&internal, &parties, &all] {
            assert_eq!(
                Releasability::combine([internal.clone(), other.clone()]),
                Releasability::Internal,
                "internal wins over {other:?}"
            );
            assert_eq!(
                Releasability::combine([other.clone(), internal.clone()]),
                Releasability::Internal,
                "order does not matter"
            );
        }
        assert_eq!(
            Releasability::combine([all.clone(), parties.clone()]),
            parties,
            "parties are more restrictive than all peers"
        );
        assert_eq!(Releasability::combine([all.clone(), all.clone()]), all);
    }

    #[test]
    fn two_party_sets_combine_to_their_intersection() {
        let a = Releasability::parties(["x", "y"]);
        let b = Releasability::parties(["y", "z"]);
        assert_eq!(
            Releasability::combine([a, b]),
            Releasability::parties(["y"])
        );
    }

    #[test]
    fn party_sets_with_nothing_in_common_combine_to_internal() {
        let a = Releasability::parties(["x"]);
        let b = Releasability::parties(["z"]);
        assert_eq!(
            Releasability::combine([a, b]),
            Releasability::Internal,
            "an empty intersection must not read as releasable"
        );
    }

    #[test]
    fn combining_nothing_is_internal() {
        assert_eq!(
            Releasability::combine(std::iter::empty()),
            Releasability::Internal,
            "a product with no inputs has nothing permitting its release"
        );
    }

    #[test]
    fn a_report_cannot_launder_a_restricted_track_by_aggregation() {
        let inputs = [
            Releasability::AllPeers,
            Releasability::AllPeers,
            Releasability::Internal,
            Releasability::AllPeers,
        ];
        assert_eq!(
            Releasability::combine(inputs),
            Releasability::Internal,
            "one restricted input restricts the whole product"
        );
    }

    #[test]
    fn a_filtered_collection_reports_how_many_were_withheld() {
        struct Item(Releasability);
        let items = vec![
            Item(Releasability::AllPeers),
            Item(Releasability::Internal),
            Item(Releasability::parties(["partner-a"])),
            Item(Releasability::Internal),
        ];
        let out = filter_for(items, "partner-a", |i| &i.0);
        assert_eq!(out.items.len(), 2);
        assert_eq!(
            out.withheld, 2,
            "a silently shortened list makes a peer believe it has everything"
        );
    }

    /// GAP-206: a track is marked by the sources that fed it, the most restrictive
    /// deciding, a source with no row of its own taking the deployment's default, and a
    /// track nothing is recorded as feeding staying in.
    #[test]
    fn a_product_is_marked_by_the_sources_that_fed_it() {
        let markings = SourceMarkings::new(Releasability::AllPeers)
            .with_source(2, Releasability::Internal)
            .with_source(3, Releasability::parties(["partner-a"]));
        assert_eq!(markings.mark(&[1]), Releasability::AllPeers, "the default");
        assert_eq!(
            markings.mark(&[1, 3]),
            Releasability::parties(["partner-a"])
        );
        assert_eq!(
            markings.mark(&[1, 3, 2]),
            Releasability::Internal,
            "one restricted source restricts the track"
        );
        assert_eq!(markings.mark(&[]), Releasability::Internal);
        assert_eq!(
            SourceMarkings::default().mark(&[1, 2]),
            Releasability::Internal,
            "a deployment that configured nothing releases nothing"
        );
    }

    #[test]
    fn a_single_restricted_item_is_simply_not_permitted() {
        assert!(!permits_single(&Releasability::Internal, "partner-a"));
        assert!(permits_single(&Releasability::AllPeers, "partner-a"));
    }
}

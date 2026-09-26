// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The late-data policy: what the tracker does with a detection that arrives after data
//! measured later than it (GAP-114, D-98).
//!
//! **Why it lives in the lowest crate.** Two layers read it and neither may depend on the
//! other. `gungnir-fusion-async`'s reorder buffer is where lateness is known and where the
//! policy is applied; `gungnir-time`'s clock-skew estimate (MOP-09) judges a source out of
//! sync against the same policy, and `gungnir-config` carries it in the baseline. Until
//! GAP-114 it was defined in `gungnir-time`, which the pipeline sits below, so the one
//! place a late detection is decided could not name the policy and ran a separate horizon
//! nothing configured. Owned here and re-exported upward, by `gungnir-model` and from
//! there `gungnir-time`, it is one type with one meaning in every crate
//! (`docs/agentic-coding-standards.md` §1.2).
//!
//! **Lateness is measured in source time**, against the newest source time the pipeline
//! has taken: how far behind the stream's front a detection arrives. Not receipt time
//! minus source time -- that mixes transit delay with a source's clock error, which is
//! what the clock-skew estimate reports separately, and it has no meaning in a replay.

/// What to do with a detection whose source time is earlier than data already taken.
///
/// Serialized with a `kind` tag (`{"kind": "buffer-and-reorder", "max_lateness_s": 1.0}`,
/// `{"kind": "reject"}`, `{"kind": "accept-as-is"}`), the shape the baseline's other
/// tagged sections use.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum LateDataPolicy {
    /// No reordering: every detection is processed as it arrives, and one whose source
    /// time is earlier than any already taken is dropped and counted. The lowest latency
    /// a deployment can choose, paid for with every out-of-order detection.
    Reject,
    /// Hold every detection in the reorder buffer until the stream's front is
    /// `max_lateness_s` past it, then process in source-time order. A detection arriving
    /// more than `max_lateness_s` behind the front, or behind what has already been
    /// processed, is dropped and counted. The picture runs `max_lateness_s` behind the
    /// newest data, which is the price of the ordering.
    BufferAndReorder { max_lateness_s: f64 },
    /// Process a late detection as though it were in order: applied to the estimate as it
    /// stands, at the newest time already taken, never retrodicted to its own time.
    /// **Replay and testing only**: folding a stale measurement into a current estimate at
    /// full weight quietly corrupts the track, so a baseline may not choose it
    /// (`gungnir-config` refuses it; D-99).
    AcceptAsIs,
}

impl LateDataPolicy {
    /// What every deployment ran before the policy was configurable: a one-second reorder
    /// buffer, the pipeline's own horizon since GAP-011 (D-99).
    pub const DEFAULT_MAX_LATENESS_S: f64 = 1.0;

    /// How long the reorder buffer holds a detection before processing it, seconds:
    /// `max_lateness_s` when buffering, zero when not.
    #[must_use]
    pub fn hold_s(&self) -> f64 {
        match self {
            Self::BufferAndReorder { max_lateness_s } => *max_lateness_s,
            Self::Reject | Self::AcceptAsIs => 0.0,
        }
    }
}

impl Default for LateDataPolicy {
    /// [`LateDataPolicy::DEFAULT_MAX_LATENESS_S`] of buffering: what a baseline that does
    /// not name a policy has always run, so reading one written before the field existed
    /// changes nothing the picture does.
    fn default() -> Self {
        Self::BufferAndReorder {
            max_lateness_s: Self::DEFAULT_MAX_LATENESS_S,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_baseline_shape_round_trips_for_every_variant() {
        for (policy, json) in [
            (
                LateDataPolicy::BufferAndReorder {
                    max_lateness_s: 2.5,
                },
                r#"{"kind":"buffer-and-reorder","max_lateness_s":2.5}"#,
            ),
            (LateDataPolicy::Reject, r#"{"kind":"reject"}"#),
            (LateDataPolicy::AcceptAsIs, r#"{"kind":"accept-as-is"}"#),
        ] {
            assert_eq!(serde_json::to_string(&policy).expect("encodes"), json);
            let back: LateDataPolicy = serde_json::from_str(json).expect("decodes");
            assert_eq!(back, policy);
        }
    }

    #[test]
    fn only_buffering_holds_anything() {
        assert!((LateDataPolicy::default().hold_s() - 1.0).abs() < f64::EPSILON);
        assert!(LateDataPolicy::Reject.hold_s().abs() < f64::EPSILON);
        assert!(LateDataPolicy::AcceptAsIs.hold_s().abs() < f64::EPSILON);
    }
}

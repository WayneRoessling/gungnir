// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Peer sources and coalition exchange.
//!
//! Design: docs/design/DN-16-peer-sources.md and
//! docs/design/DN-18-coalition-exchange.md. Capabilities CAP-1.6 and CAP-7.4;
//! decisions D-06, D-08, D-09.
//!
//! **Quality is assigned by us, not claimed by them.** A peer that marks everything
//! high confidence cannot raise its own weight in our fusion. That is the single
//! most important rule here.
//!
//! **Age is always visible.** A thirty-second-old peer track drawn identically to a
//! live local one is a lie the operator cannot detect, so the difference between the
//! peer's stamp and our receipt travels on every peer-sourced track.
//!
//! Coalition exchange needs almost no new mechanism: it is the peer source inbound,
//! the existing stream and reports outbound, the handoff, and the marking. What it
//! needs beyond those is an agreement, which is what [`ExchangeAgreement`] is.
//!
//! [`ReportedPosition`] is DN-25's addition, capability CAP-3.8, GAP-090; see
//! docs/design/DN-25-cursor-on-target.md §3: a position an entity reports about
//! itself, from something that is not a sensor. It reuses [`PeerOrigin`] rather
//! than paralleling it, for the reason §3 gives: the timing and quality rules
//! are the same three, and a second struct beside it would drift.

use crate::{Classification, Geodetic, MissionTime, Releasability};

/// Where a peer-sourced track came from and how far behind it is.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PeerOrigin {
    /// Configured name of the peer node, not its address.
    pub peer: String,
    /// The peer's own track identifier, kept so a correction can be matched.
    pub remote_track: String,
    /// What the peer stamped, and when we received it. The difference is the age
    /// the operator must see.
    pub peer_time: MissionTime,
    pub receipt_time: MissionTime,
    /// Quality this deployment assigns to the peer, from configuration.
    ///
    /// **Not a number the peer sends about itself.**
    pub assigned_quality: f32,
}

impl PeerOrigin {
    /// Seconds between the peer's stamp and our receipt.
    pub fn age_s(&self) -> f64 {
        self.receipt_time.seconds_since(self.peer_time)
    }

    /// True when this track is older than the deployment tolerates.
    ///
    /// Beyond the limit a peer track is marked stale rather than discarded, and the
    /// existing stale rule keeps it out of allocation.
    pub fn is_stale_beyond(&self, max_age_s: f64) -> bool {
        self.age_s() > max_age_s
    }
}

/// A position an entity reports about itself, from something that is not a sensor.
///
/// **Never a track.** DN-16 refused to make a track out of a peer's launch warning,
/// because a track we have not observed is a track we cannot maintain. A self-report
/// is the same case and takes the same answer.
///
/// Nothing in this workspace constructs one yet. GAP-091's wire adapter -- the
/// codec, the inbound feed, and the multicast sink DN-25 §2 assigns to
/// `gungnir-interop`, `gungnir-ingest`, and `gungnir-remote` -- is blocked on an
/// actual TAK client (ATAK, `WinTAK`, or iTAK) to record a corpus from
/// (`docs/design/external-standards.md` §5.6, §5.8): "recorded, never authored" is
/// the rule that keeps a decoder from being tested only against its own author's
/// fixtures, and that client is the one thing this workspace cannot supply itself.
/// The type exists so `gungnir-policy`'s DN-05 rule 1 (GAP-090) can be written and
/// gated against it now, with directly constructed values standing in for the feed,
/// so that a future feed plugs in without a further change to the policy.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReportedPosition {
    /// Timing and assigned quality, exactly as a peer-sourced track carries them.
    pub origin: PeerOrigin,
    /// Callsign or unit as the reporter states it. Shown as theirs, and never
    /// resolved against the order of battle unless a person does it.
    pub reporter: String,
    /// WGS-84 as it arrived. **The caller anchors it to the local frame**, because
    /// the conversion lives in a crate this type's consumers may not depend on --
    /// the correction DN-01 §3a had to make, not repeated here.
    pub position: Geodetic,
    /// The accuracy the reporter claims, in metres. Recorded on the provenance and
    /// weighted by nothing.
    pub claimed_accuracy_m: Option<f64>,
    /// What the reporter says it is, from the type's `friend` predicate and
    /// nothing else. Only `Friendly` is acted on; see DN-25 §5 rule 3.
    pub affiliation: Classification,
}

/// A launch warning as the deployment that raised it publishes it (DN-16 §5, GAP-009).
///
/// **A warning is not a track, and this type is deliberately unable to become one.** It
/// carries no position, no velocity and no covariance, because DN-16 §5's reason for
/// making it a distinct message is that "a warning is a statement about the future with
/// no kinematic state". A field for where the launch was seen would be the first step
/// back towards the track the note refuses: "a track we have not observed is a track we
/// cannot maintain". The absence of kinematic fields is the rule, in the same way that
/// [`PeerOrigin`] having no peer-claimed quality field is the rule that a peer cannot
/// raise its own weight.
///
/// Nothing in this workspace issues one yet: the type exists so a peer's warning can be
/// received, and no producer has been faked to make the path look busier than it is.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LaunchWarningReport {
    /// The issuing deployment's own identifier for this warning.
    ///
    /// Kept for the reason [`PeerOrigin::remote_track`] is kept: without it a repeat and
    /// a correction cannot be told apart from a second launch, and a second launch is the
    /// one of the three an operator must act on.
    pub id: String,
    /// What the issuer says was launched, in the issuer's own words.
    ///
    /// Free text rather than an enumeration, because the vocabulary belongs to whoever
    /// raised the warning: an enumeration would silently map a category we do not have
    /// onto one we do, and an operator reading "unknown" could not tell that from a
    /// peer that really said unknown.
    pub what: String,
    /// Mission time the issuer stamped it. Ours is [`PeerLaunchWarning::receipt_time`],
    /// and the two are kept apart so the age is visible (DN-16 §5).
    pub at: MissionTime,
    /// The marking, which is the second of DN-18 §5's two gates.
    ///
    /// Present so a launch warning crossing an exchange is filtered by the marking as
    /// well as the agreement, with the restrictive one deciding
    /// ([`ExchangeSet::may_send`]). Defaults to `Internal` like every other marking, so
    /// a warning that nobody marked does not leave the deployment.
    #[serde(default)]
    pub releasability: Releasability,
}

/// A launch warning as this deployment received it from a peer (DN-16 §5, GAP-009).
///
/// The two fields this deployment adds are the two the sender cannot know: the name we
/// know the peer by, and when we took the message. Both follow the rule [`PeerOrigin`]
/// follows -- what is ours is stamped by us and never read off the wire, so a peer
/// cannot rename itself into another peer's alerts or claim to have been received
/// earlier than it was.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PeerLaunchWarning {
    /// Configured name of the peer, from our baseline. **Not a name the peer sends.**
    pub peer: String,
    /// What the peer published, unchanged.
    pub report: LaunchWarningReport,
    /// When we took it off the peer's stream.
    pub receipt_time: MissionTime,
}

impl PeerLaunchWarning {
    /// Seconds between the peer's stamp and our receipt.
    ///
    /// DN-16 §5: age is always visible. A warning is about the future, so a late one is
    /// worth less than a fresh one and the operator has to be able to see which it is.
    pub fn age_s(&self) -> f64 {
        self.receipt_time.seconds_since(self.report.at)
    }

    /// True when the warning is older than the deployment tolerates for this peer.
    ///
    /// Reported, never a reason to discard: DN-16 §5 marks a stale peer message rather
    /// than dropping it, and a launch warning that arrived late is still evidence that a
    /// peer saw a launch.
    pub fn is_stale_beyond(&self, max_age_s: f64) -> bool {
        self.age_s() > max_age_s
    }

    /// The one-line alert DN-16 §5 requires: "it raises an alert with the peer named".
    ///
    /// The peer, its own words, and the age, in that order, because the peer is what
    /// tells an operator how much to believe it and the age is what tells them whether
    /// it is still current.
    pub fn alert_summary(&self) -> String {
        format!(
            "launch warning from peer {}: {} (their {}, {:.0} s old on receipt)",
            self.peer,
            self.report.what,
            self.report.id,
            self.age_s()
        )
    }
}

/// What flows in one direction under an agreement.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum ExchangeItem {
    Tracks,
    Warnings,
    Reports,
    Handoffs,
    Health,
}

/// Wire format for one partner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExchangeFormat {
    /// Our own schema, for another instance of this product.
    Canonical,
    Stanag4676,
    Asterix048,
}

impl ExchangeFormat {
    /// True when converting to this format loses information.
    ///
    /// Conversion is lossy in both directions for the industry formats, and the
    /// loss is recorded on the provenance rather than assumed away.
    pub fn is_lossy(self) -> bool {
        !matches!(self, ExchangeFormat::Canonical)
    }
}

/// A configured exchange relationship.
///
/// Expressible asymmetrically: a partner that may send tracks but not receive plans
/// is an agreement with `Tracks` inbound and nothing outbound.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExchangeAgreement {
    /// Matches the peer name in the peer table and the party in a marking.
    pub party: String,
    pub inbound: Vec<ExchangeItem>,
    pub outbound: Vec<ExchangeItem>,
    pub format: ExchangeFormat,
}

impl ExchangeAgreement {
    pub fn accepts(&self, item: ExchangeItem) -> bool {
        self.inbound.contains(&item)
    }

    pub fn sends(&self, item: ExchangeItem) -> bool {
        self.outbound.contains(&item)
    }
}

/// The agreements in force, and the two gates they form with releasability.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExchangeSet {
    pub agreements: Vec<ExchangeAgreement>,
}

impl ExchangeSet {
    pub fn for_party(&self, party: &str) -> Option<&ExchangeAgreement> {
        self.agreements.iter().find(|a| a.party == party)
    }

    /// Whether an inbound item type is accepted from this party at all.
    ///
    /// Refused here, **before** the ingest gateway. Anything on the list still goes
    /// through validation and quarantine like any other source.
    pub fn accepts_inbound(&self, party: &str, item: ExchangeItem) -> bool {
        self.for_party(party).is_some_and(|a| a.accepts(item))
    }

    /// Whether an outbound item may go to this party.
    ///
    /// **Two independent gates, and the restrictive one always decides.** The
    /// agreement says what type of thing may flow; the marking says whether this
    /// particular thing may. An agreement permitting reports does not override a
    /// report marked internal.
    pub fn may_send(&self, party: &str, item: ExchangeItem, marking: &Releasability) -> bool {
        self.for_party(party).is_some_and(|a| a.sends(item)) && marking.permits(party)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agreement(party: &str) -> ExchangeAgreement {
        ExchangeAgreement {
            party: party.into(),
            inbound: vec![ExchangeItem::Tracks, ExchangeItem::Warnings],
            outbound: vec![ExchangeItem::Tracks, ExchangeItem::Reports],
            format: ExchangeFormat::Canonical,
        }
    }

    fn origin(peer_time: f64, receipt_time: f64, assigned_quality: f32) -> PeerOrigin {
        PeerOrigin {
            peer: "partner-a".into(),
            remote_track: "T-9911".into(),
            peer_time: MissionTime(peer_time),
            receipt_time: MissionTime(receipt_time),
            assigned_quality,
        }
    }

    #[test]
    fn peer_age_is_the_difference_between_their_stamp_and_our_receipt() {
        let o = origin(100.0, 130.0, 0.5);
        assert!((o.age_s() - 30.0).abs() < f64::EPSILON);
        assert!(o.is_stale_beyond(20.0));
        assert!(!o.is_stale_beyond(60.0));
    }

    #[test]
    fn the_assigned_quality_comes_from_our_configuration() {
        // A peer that marks everything high confidence cannot raise its own weight.
        // The struct has no field for a peer-claimed quality at all, which is the
        // strongest form of this rule.
        let o = origin(100.0, 101.0, 0.4);
        assert!((o.assigned_quality - 0.4).abs() < f32::EPSILON);
    }

    fn reported(
        peer_time: f64,
        receipt_time: f64,
        affiliation: Classification,
    ) -> ReportedPosition {
        ReportedPosition {
            origin: origin(peer_time, receipt_time, 0.6),
            reporter: "fire-group-2".into(),
            position: crate::Geodetic {
                lat_rad: 0.0,
                lon_rad: 0.0,
                alt_m: 0.0,
            },
            claimed_accuracy_m: Some(5.0),
            affiliation,
        }
    }

    #[test]
    fn a_reported_position_reuses_peer_origin_for_age_and_staleness() {
        // DN-25 §3: "PeerOrigin is reused rather than paralleled". The strongest
        // check of that claim is that the same two methods, called through the
        // embedded field, give the same answer they give PeerOrigin directly.
        let r = reported(100.0, 140.0, Classification::Friendly);
        assert!((r.origin.age_s() - 40.0).abs() < f64::EPSILON);
        assert!(r.origin.is_stale_beyond(30.0));
        assert!(!r.origin.is_stale_beyond(50.0));
    }

    #[test]
    fn a_reported_position_round_trips_and_carries_no_kinematic_state() {
        // The same criterion as a launch warning (DN-16 §5's rule DN-25 §3 reuses):
        // a self-report is never a track, so nothing serialised can be mistaken for
        // one.
        let r = reported(100.0, 101.0, Classification::Friendly);
        let text = serde_json::to_string(&r).expect("serialises");
        assert!(!text.contains("velocity"), "{text}");
        assert!(!text.contains("covariance"), "{text}");
        let back: ReportedPosition = serde_json::from_str(&text).expect("parses");
        assert_eq!(r, back);
    }

    #[test]
    fn a_reported_position_carries_the_reporters_claimed_affiliation_unfiltered() {
        // The type itself records whatever the reporter claims (DN-25 §3: "from the
        // type's `friend` predicate and nothing else"); only `Friendly` is acted on
        // is a rule for the reader (DN-05 rule 1, GAP-090), not a constraint the
        // type enforces on construction.
        let hostile = reported(100.0, 101.0, Classification::Hostile);
        assert_eq!(hostile.affiliation, Classification::Hostile);
        let friendly = reported(100.0, 101.0, Classification::Friendly);
        assert_eq!(friendly.affiliation, Classification::Friendly);
    }

    #[test]
    fn an_item_type_absent_from_the_inbound_list_is_refused() {
        let set = ExchangeSet {
            agreements: vec![agreement("partner-a")],
        };
        assert!(set.accepts_inbound("partner-a", ExchangeItem::Tracks));
        assert!(!set.accepts_inbound("partner-a", ExchangeItem::Handoffs));
    }

    #[test]
    fn a_peer_with_no_agreement_can_do_nothing() {
        let set = ExchangeSet::default();
        assert!(!set.accepts_inbound("partner-a", ExchangeItem::Tracks));
        assert!(!set.may_send("partner-a", ExchangeItem::Tracks, &Releasability::AllPeers));
    }

    #[test]
    fn the_marking_is_checked_independently_of_the_agreement() {
        // The criterion that proves the two gates are independent. An
        // implementation shortcut would check only the agreement.
        let set = ExchangeSet {
            agreements: vec![agreement("partner-a")],
        };
        assert!(
            set.may_send(
                "partner-a",
                ExchangeItem::Reports,
                &Releasability::parties(["partner-a"])
            ),
            "the agreement permits reports and the marking names the party"
        );
        assert!(
            !set.may_send("partner-a", ExchangeItem::Reports, &Releasability::Internal),
            "an agreement permitting reports does not override an internal marking"
        );
        assert!(
            !set.may_send("partner-a", ExchangeItem::Health, &Releasability::AllPeers),
            "and a permissive marking does not override the agreement"
        );
    }

    #[test]
    fn an_agreement_can_be_asymmetric() {
        let listen_only = ExchangeAgreement {
            party: "partner-b".into(),
            inbound: vec![ExchangeItem::Tracks],
            outbound: Vec::new(),
            format: ExchangeFormat::Stanag4676,
        };
        let set = ExchangeSet {
            agreements: vec![listen_only],
        };
        assert!(set.accepts_inbound("partner-b", ExchangeItem::Tracks));
        assert!(!set.may_send("partner-b", ExchangeItem::Tracks, &Releasability::AllPeers));
    }

    #[test]
    fn industry_formats_are_lossy_and_our_own_is_not() {
        assert!(!ExchangeFormat::Canonical.is_lossy());
        assert!(ExchangeFormat::Stanag4676.is_lossy());
        assert!(ExchangeFormat::Asterix048.is_lossy());
    }

    fn warning(at: f64, receipt: f64) -> PeerLaunchWarning {
        PeerLaunchWarning {
            peer: "partner-a".into(),
            report: LaunchWarningReport {
                id: "LW-4".into(),
                what: "ballistic launch, northern sector".into(),
                at: MissionTime(at),
                releasability: Releasability::AllPeers,
            },
            receipt_time: MissionTime(receipt),
        }
    }

    #[test]
    fn a_launch_warnings_age_is_the_difference_between_their_stamp_and_our_receipt() {
        let w = warning(100.0, 118.0);
        assert!((w.age_s() - 18.0).abs() < f64::EPSILON);
        assert!(w.is_stale_beyond(10.0));
        assert!(!w.is_stale_beyond(30.0));
    }

    #[test]
    fn the_alert_names_the_peer_and_says_how_old_the_warning_is() {
        // DN-16 §5: "it raises an alert with the peer named". An alert that did not
        // say which peer warned is one an operator cannot weigh.
        let line = warning(100.0, 118.0).alert_summary();
        assert!(line.contains("partner-a"), "{line}");
        assert!(line.contains("ballistic launch"), "{line}");
        assert!(line.contains("18 s old"), "{line}");
    }

    #[test]
    fn a_launch_warning_round_trips_and_carries_no_kinematic_state() {
        // The criterion the note's own reason turns on: a warning is a statement about
        // the future with no kinematic state, so nothing serialised here can be read as
        // a position and turned into a track.
        let w = warning(100.0, 101.0);
        let text = serde_json::to_string(&w).expect("serialises");
        assert!(!text.contains("state"), "{text}");
        assert!(!text.contains("covariance"), "{text}");
        assert!(!text.contains("measurement"), "{text}");
        let back: PeerLaunchWarning = serde_json::from_str(&text).expect("parses");
        assert_eq!(w, back);
    }

    #[test]
    fn an_unmarked_launch_warning_stays_inside_the_deployment() {
        // The marking defaults to `Internal` like every other, so a peer sending a
        // warning with no marking field cannot have it forwarded onward by default.
        let report: LaunchWarningReport =
            serde_json::from_str(r#"{"id":"LW-1","what":"launch","at":10.0}"#).expect("parses");
        assert_eq!(report.releasability, Releasability::Internal);
        // The agreement sends warnings, so the marking is the gate that decides here.
        let set = ExchangeSet {
            agreements: vec![ExchangeAgreement {
                party: "partner-a".into(),
                inbound: vec![ExchangeItem::Warnings],
                outbound: vec![ExchangeItem::Warnings],
                format: ExchangeFormat::Canonical,
            }],
        };
        assert!(!set.may_send("partner-a", ExchangeItem::Warnings, &report.releasability));
        assert!(set.may_send(
            "partner-a",
            ExchangeItem::Warnings,
            &Releasability::AllPeers
        ));
    }
}

// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Warning assets and authorities.
//!
//! Design: docs/design/DN-03-warning.md. Capability CAP-4.5; measures MOE-01 and
//! MOP-33; mission threads MT-02 (warn an asset and its units) and MT-04 (warn a
//! port authority).
//!
//! For a threat with no engagement option, warning is the only response, and today
//! it is a radio call an operator remembers to make.
//!
//! This crate gained an edge to `gungnir-assessment` for it, accepted by the
//! engineering reviewer on 2026-09-05 and drawn in ARCHITECTURE.md §7.1. The trigger
//! is a **rule about an obligation**, not a detector: it reads the obligation from
//! the asset, the prediction from assessment, and the state machine from here.
//! Putting it in a binary would put a policy rule in code with no unit test and no
//! second consumer.
//!
//! **Failure is loud.** An endpoint that cannot be reached leaves the warning
//! visibly failed and the alert open. A warning function that quietly fails is worse
//! than none, because the operator believes the asset was warned.

use gungnir_assessment::AssetExposure;
use gungnir_model::{AssetId, DefendedAsset, MissionTime, TrackId, WarningObligation};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum WarningState {
    /// The obligation has triggered and nothing has been sent.
    Owed,
    /// Handed to the endpoint; not yet acknowledged.
    Sent { at: MissionTime },
    /// The receiving party acknowledged.
    Acknowledged { at: MissionTime },
    /// Sent or still owed after the due time. Kept distinct so the measure can
    /// count it, and **never closed by the passage of time**.
    Late,
    /// The endpoint refused or was unreachable. Never silently retried away.
    Failed { reason: String },
    /// A person judged it unnecessary and said why. A recorded decision, like any
    /// other.
    Waived { operator: String, reason: String },
}

impl WarningState {
    /// True while the warning still needs attention.
    pub fn is_open(&self) -> bool {
        matches!(
            self,
            WarningState::Owed
                | WarningState::Sent { .. }
                | WarningState::Late
                | WarningState::Failed { .. }
        )
    }

    /// True when the obligation has been discharged one way or another.
    pub fn is_discharged(&self) -> bool {
        matches!(
            self,
            WarningState::Acknowledged { .. } | WarningState::Waived { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WarningTransition {
    pub to: WarningState,
    pub at: MissionTime,
}

/// A warning owed to an asset because of one track.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Warning {
    pub asset: AssetId,
    pub track: TrackId,
    /// Mission time by which the warning is owed, from the obligation's lead time
    /// and the predicted impact.
    pub due_by: MissionTime,
    /// Endpoint name from the asset's obligation, resolved per decision D-08.
    pub channel: String,
    pub state: WarningState,
    pub history: Vec<WarningTransition>,
}

impl Warning {
    fn new(asset: AssetId, track: TrackId, due_by: MissionTime, channel: String) -> Self {
        Self {
            asset,
            track,
            due_by,
            channel,
            state: WarningState::Owed,
            history: Vec::new(),
        }
    }

    fn move_to(&mut self, to: WarningState, at: MissionTime) {
        self.history.push(WarningTransition { to: to.clone(), at });
        self.state = to;
    }

    /// The endpoint accepted it for delivery.
    pub fn sent(&mut self, at: MissionTime) {
        self.move_to(WarningState::Sent { at }, at);
    }

    /// The receiving party acknowledged.
    pub fn acknowledged(&mut self, at: MissionTime) {
        self.move_to(WarningState::Acknowledged { at }, at);
    }

    /// Delivery failed. The alert stays open and the reason travels with it.
    pub fn failed(&mut self, reason: impl Into<String>, at: MissionTime) {
        self.move_to(
            WarningState::Failed {
                reason: reason.into(),
            },
            at,
        );
    }

    /// A person judged it unnecessary. Recorded with who and why.
    pub fn waive(
        &mut self,
        operator: impl Into<String>,
        reason: impl Into<String>,
        at: MissionTime,
    ) {
        self.move_to(
            WarningState::Waived {
                operator: operator.into(),
                reason: reason.into(),
            },
            at,
        );
    }

    /// True when the due time has passed with the obligation undischarged.
    pub fn is_overdue(&self, now: MissionTime) -> bool {
        !self.state.is_discharged() && now > self.due_by && self.state != WarningState::Late
    }
}

/// Marks every overdue warning late and raises its severity.
///
/// Returns the assets whose warnings went late, so the caller can alert. A warning
/// still owed past its due time is **never closed by the passage of time**.
pub fn mark_overdue(warnings: &mut [Warning], now: MissionTime) -> Vec<AssetId> {
    let mut late = Vec::new();
    for w in warnings.iter_mut().filter(|w| w.is_overdue(now)) {
        w.move_to(WarningState::Late, now);
        late.push(w.asset);
    }
    late
}

/// Why an obligation triggered for a track (DN-03 §5 rule 1 and amendment 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Trigger {
    /// Predicted to arrive in this many seconds, inside the lead time.
    ImpactIn { seconds: f64 },
    /// Predicted to pass this close, inside the obligation's distance, with no impact
    /// predicted. Owed by the closest approach when the exposure says when that is, and
    /// never later than the lead time (amendment 2).
    PassWithin {
        metres: f64,
        /// Seconds until the closest approach, when the prediction carries it.
        in_s: Option<f64>,
    },
}

/// Whether `exposure` triggers `obligation`, and how. Impact is judged first: a track
/// that will arrive is also one that will pass within any distance, and the time is the
/// more useful fact.
#[must_use]
pub fn trigger(obligation: &WarningObligation, exposure: &AssetExposure) -> Option<Trigger> {
    if let Some(seconds) = exposure.time_to_impact_s.map(f64::from) {
        if seconds <= obligation.lead_time_s {
            return Some(Trigger::ImpactIn { seconds });
        }
    }
    match (obligation.within_m, exposure.closest_approach_m) {
        (Some(within), Some(metres)) if metres <= within => Some(Trigger::PassWithin {
            metres,
            in_s: exposure.time_to_closest_approach_s,
        }),
        _ => None,
    }
}

/// Raises warnings for every asset whose obligation an exposure has triggered.
///
/// Deduplicates by asset-and-track pair only, which is mechanical. **It does not
/// suppress a duplicate warning from a second track, and it does not decide that a
/// warning is unnecessary**: both are judgements, and judgements belong with people.
pub fn raise_due(
    assets: &[DefendedAsset],
    exposures: &[(TrackId, AssetExposure)],
    open: &[Warning],
    now: MissionTime,
) -> Vec<Warning> {
    let mut raised = Vec::new();
    for (track, exposure) in exposures {
        let Some(asset) = assets.iter().find(|a| a.id == exposure.asset) else {
            continue;
        };
        let Some(obligation) = &asset.warning else {
            continue;
        };
        // Time until the threat arrives, or how near it will pass; either can
        // trigger an obligation.
        let Some(why) = trigger(obligation, exposure) else {
            continue;
        };
        // DN-03 amendment 2: a pass-close warning is due by the closest approach when
        // the prediction says when that is, and never later than the lead time.
        let due_in_s = match why {
            Trigger::ImpactIn { seconds } => seconds,
            Trigger::PassWithin { in_s, .. } => {
                in_s.map_or(obligation.lead_time_s, |t| t.min(obligation.lead_time_s))
            }
        };
        let already = open
            .iter()
            .chain(raised.iter())
            .any(|w| w.asset == asset.id && w.track == *track && w.state.is_open());
        if already {
            continue;
        }
        raised.push(Warning::new(
            asset.id,
            *track,
            MissionTime(now.0 + due_in_s),
            obligation.channel.clone(),
        ));
    }
    raised
}

/// Warnings that need somebody's attention now: late or failed.
pub fn needs_attention(warnings: &[Warning]) -> Vec<&Warning> {
    warnings
        .iter()
        .filter(|w| matches!(w.state, WarningState::Late | WarningState::Failed { .. }))
        .collect()
}

// ---------------------------------------------------------------------------------
// The ledger (GAP-042, wired 2026-09-06). Everything above was written against DN-03 on
// 2026-09-05 and had no caller; the ledger is what the desktop tick calls, and it uses
// those primitives rather than restating them.

/// Carries a warning to its channel. The binary supplies it: this crate knows the rule
/// and the record, never the wire (DN-03 §5 rule 2; D-08 for endpoints).
pub trait WarningDelivery {
    /// Attempt delivery. `Ok` means the endpoint took it (state `Sent`); `Err` names
    /// why not, and the warning becomes `Failed` with that reason.
    ///
    /// # Errors
    ///
    /// The reason delivery did not happen, verbatim into the record.
    fn deliver(&self, warning: &Warning) -> Result<(), String>;
}

/// What an evaluation changed, so the caller can journal and alert on it.
#[derive(Debug, Clone, PartialEq)]
pub enum WarningChange {
    Raised {
        asset: AssetId,
        track: TrackId,
        due_by: MissionTime,
    },
    Sent {
        asset: AssetId,
        track: TrackId,
    },
    Failed {
        asset: AssetId,
        track: TrackId,
        reason: String,
    },
    Late {
        asset: AssetId,
        track: TrackId,
    },
    Closed {
        asset: AssetId,
        track: TrackId,
        final_state: WarningState,
    },
}

/// One word for a list.
#[must_use]
pub fn state_label(state: &WarningState) -> &'static str {
    match state {
        WarningState::Owed => "owed",
        WarningState::Sent { .. } => "sent",
        WarningState::Acknowledged { .. } => "acknowledged",
        WarningState::Late => "late",
        WarningState::Failed { .. } => "failed",
        WarningState::Waived { .. } => "waived",
    }
}

/// Every warning a desktop has raised: open, and closed with the time each closed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WarningLedger {
    open: Vec<Warning>,
    closed: Vec<(Warning, MissionTime)>,
}

impl WarningLedger {
    #[must_use]
    pub fn open(&self) -> &[Warning] {
        &self.open
    }

    #[must_use]
    pub fn closed(&self) -> &[(Warning, MissionTime)] {
        &self.closed
    }

    #[must_use]
    pub fn late_count(&self) -> usize {
        self.open
            .iter()
            .filter(|w| w.state == WarningState::Late)
            .count()
    }

    #[must_use]
    pub fn failed_count(&self) -> usize {
        self.open
            .iter()
            .filter(|w| matches!(w.state, WarningState::Failed { .. }))
            .count()
    }

    /// The rule on the tick: raise what the exposures oblige (`raise_due`), offer each
    /// new warning to the endpoint **once**, mark the overdue (`mark_overdue`), and close
    /// every pair the exposures no longer trigger, keeping its final state (DN-03 §5).
    pub fn evaluate(
        &mut self,
        now: MissionTime,
        assets: &[DefendedAsset],
        exposures: &[(TrackId, AssetExposure)],
        delivery: &dyn WarningDelivery,
    ) -> Vec<WarningChange> {
        let mut changes = Vec::new();
        for mut w in raise_due(assets, exposures, &self.open, now) {
            changes.push(WarningChange::Raised {
                asset: w.asset,
                track: w.track,
                due_by: w.due_by,
            });
            match delivery.deliver(&w) {
                Ok(()) => {
                    w.sent(now);
                    changes.push(WarningChange::Sent {
                        asset: w.asset,
                        track: w.track,
                    });
                }
                Err(reason) => {
                    w.failed(reason.clone(), now);
                    changes.push(WarningChange::Failed {
                        asset: w.asset,
                        track: w.track,
                        reason,
                    });
                }
            }
            self.open.push(w);
        }
        let before: Vec<(AssetId, TrackId)> = self
            .open
            .iter()
            .filter(|w| w.state == WarningState::Late)
            .map(|w| (w.asset, w.track))
            .collect();
        mark_overdue(&mut self.open, now);
        for w in self.open.iter().filter(|w| w.state == WarningState::Late) {
            if !before.contains(&(w.asset, w.track)) {
                changes.push(WarningChange::Late {
                    asset: w.asset,
                    track: w.track,
                });
            }
        }
        // Close what no longer triggers: the pair is absent from the exposures that
        // are inside their lead time.
        let still_triggered = |w: &Warning| {
            exposures.iter().any(|(track, e)| {
                *track == w.track
                    && e.asset == w.asset
                    && assets
                        .iter()
                        .find(|a| a.id == e.asset)
                        .and_then(|a| a.warning.as_ref())
                        .is_some_and(|o| trigger(o, e).is_some())
            })
        };
        let (still, done): (Vec<Warning>, Vec<Warning>) =
            self.open.drain(..).partition(|w| still_triggered(w));
        self.open = still;
        for w in done {
            changes.push(WarningChange::Closed {
                asset: w.asset,
                track: w.track,
                final_state: w.state.clone(),
            });
            self.closed.push((w, now));
        }
        changes
    }

    /// A person waives an open warning (DN-03 §5 rule 4). `false` when none matches.
    pub fn waive(
        &mut self,
        asset: AssetId,
        track: TrackId,
        operator: String,
        reason: String,
        now: MissionTime,
    ) -> bool {
        match self
            .open
            .iter_mut()
            .find(|w| w.asset == asset && w.track == track && w.state.is_open())
        {
            Some(w) => {
                w.waive(operator, reason, now);
                true
            }
            None => false,
        }
    }

    /// A warning handed to a transport that then could not deliver it: `Sent` (or
    /// `Owed`) becomes `Failed` with the reason (GAP-040's transport answering after the
    /// fact). `false` when no open warning matches.
    pub fn fail(
        &mut self,
        asset: AssetId,
        track: TrackId,
        reason: String,
        now: MissionTime,
    ) -> bool {
        match self.open.iter_mut().find(|w| {
            w.asset == asset
                && w.track == track
                && matches!(
                    w.state,
                    WarningState::Sent { .. } | WarningState::Owed | WarningState::Late
                )
        }) {
            Some(w) => {
                w.failed(reason, now);
                true
            }
            None => false,
        }
    }

    /// The warned party acknowledged a warning that was sent (DN-03 §5 rule 2).
    ///
    /// `Late` is accepted as well as `Sent`, because [`WarningState::Late`] means "sent or
    /// still owed after the due time" and the late case is exactly the one this path
    /// exists for: a warning that went out, went late, and was then answered has had its
    /// obligation discharged, and refusing the answer because the clock passed would lose
    /// the only fact that discharges it. [`WarningLedger::fail`] already treats the three
    /// undischarged states the same way for the same reason.
    ///
    /// `Owed` is **not** accepted: nothing was sent, so an acknowledgement of it is not
    /// credible and is reported to the caller as no match rather than recorded.
    ///
    /// `false` when no open warning matches.
    pub fn acknowledge(&mut self, asset: AssetId, track: TrackId, now: MissionTime) -> bool {
        match self.open.iter_mut().find(|w| {
            w.asset == asset
                && w.track == track
                && matches!(w.state, WarningState::Sent { .. } | WarningState::Late)
        }) {
            Some(w) => {
                w.acknowledged(now);
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod ledger_tests {
    use super::*;
    use gungnir_model::{AssetExtent, AssetPriority, Geodetic, WarningObligation};

    struct Refuses;
    impl WarningDelivery for Refuses {
        fn deliver(&self, _: &Warning) -> Result<(), String> {
            Err("no transport".into())
        }
    }
    struct Takes;
    impl WarningDelivery for Takes {
        fn deliver(&self, _: &Warning) -> Result<(), String> {
            Ok(())
        }
    }

    fn assets(lead: f64) -> Vec<DefendedAsset> {
        vec![DefendedAsset {
            id: AssetId(1),
            name: "the harbour".into(),
            extent: AssetExtent::Point {
                position: Geodetic {
                    lat_rad: 0.0,
                    lon_rad: 0.0,
                    alt_m: 0.0,
                },
            },
            priority: AssetPriority::High,
            warning: Some(WarningObligation {
                lead_time_s: lead,
                channel: "port-authority".into(),
                within_m: None,
            }),
            note: None,
        }]
    }

    fn assets_within(lead: f64, within_m: f64) -> Vec<DefendedAsset> {
        let mut assets = assets(lead);
        if let Some(w) = assets[0].warning.as_mut() {
            w.within_m = Some(within_m);
        }
        assets
    }

    fn passing(track: u64, closest_m: f64) -> (TrackId, AssetExposure) {
        (
            TrackId(track),
            AssetExposure {
                asset: AssetId(1),
                range_m: 5000.0,
                time_to_impact_s: None,
                closest_approach_m: Some(closest_m),
                time_to_closest_approach_s: Some(30.0),
            },
        )
    }

    /// DN-03 amendment 1: a track that will pass inside the distance is warned about
    /// with no impact predicted; one without a distance on its obligation is not; and
    /// the warning closes when the pass moves outside the distance.
    /// DN-03 amendment 2: a pass-close warning is due by the closest approach when the
    /// prediction says when that is, never later than the lead time, and by the lead
    /// time alone when the exposure carries no time.
    #[test]
    fn a_pass_close_warning_is_due_by_the_closest_approach() {
        let exposure = |in_s: Option<f64>| {
            let (id, mut e) = passing(7, 300.0);
            e.time_to_closest_approach_s = in_s;
            (id, e)
        };
        for (in_s, expected) in [(Some(30.0), 30.0), (Some(200.0), 120.0), (None, 120.0)] {
            let mut ledger = WarningLedger::default();
            let raised = ledger.evaluate(
                MissionTime(0.0),
                &assets_within(120.0, 500.0),
                &[exposure(in_s)],
                &Refuses,
            );
            assert!(
                matches!(raised[0], WarningChange::Raised { .. }),
                "{raised:?}"
            );
            assert_eq!(
                ledger.open()[0].due_by,
                MissionTime(expected),
                "time to closest approach {in_s:?}"
            );
        }
    }

    #[test]
    fn a_pass_inside_the_distance_triggers_and_moving_out_closes() {
        let mut ledger = WarningLedger::default();
        let none = ledger.evaluate(
            MissionTime(0.0),
            &assets(120.0),
            &[passing(7, 300.0)],
            &Refuses,
        );
        assert!(none.is_empty(), "no distance on the obligation: {none:?}");

        let raised = ledger.evaluate(
            MissionTime(0.0),
            &assets_within(120.0, 500.0),
            &[passing(7, 300.0)],
            &Refuses,
        );
        assert!(
            matches!(raised[0], WarningChange::Raised { .. }),
            "{raised:?}"
        );
        assert_eq!(
            ledger.open()[0].due_by,
            MissionTime(30.0),
            "due by the closest approach (amendment 2)"
        );

        let closed = ledger.evaluate(
            MissionTime(10.0),
            &assets_within(120.0, 500.0),
            &[passing(7, 900.0)],
            &Refuses,
        );
        assert!(
            closed
                .iter()
                .any(|c| matches!(c, WarningChange::Closed { .. })),
            "{closed:?}"
        );
        assert!(ledger.open().is_empty());
    }

    #[test]
    fn impact_is_judged_before_the_pass() {
        let obligation = WarningObligation {
            lead_time_s: 120.0,
            channel: "x".into(),
            within_m: Some(500.0),
        };
        let both = AssetExposure {
            asset: AssetId(1),
            range_m: 100.0,
            time_to_impact_s: Some(30.0),
            closest_approach_m: Some(0.0),
            time_to_closest_approach_s: None,
        };
        assert_eq!(
            trigger(&obligation, &both),
            Some(Trigger::ImpactIn { seconds: 30.0 })
        );
        let far = AssetExposure {
            time_to_impact_s: Some(600.0),
            closest_approach_m: Some(700.0),
            time_to_closest_approach_s: None,
            ..both
        };
        assert_eq!(trigger(&obligation, &far), None);
    }

    fn exposure(track: u64, in_s: Option<f32>) -> (TrackId, AssetExposure) {
        (
            TrackId(track),
            AssetExposure {
                asset: AssetId(1),
                range_m: 1000.0,
                time_to_impact_s: in_s,
                closest_approach_m: None,
                time_to_closest_approach_s: None,
            },
        )
    }

    #[test]
    fn a_refused_endpoint_leaves_the_warning_failed_open_and_never_retried() {
        let mut ledger = WarningLedger::default();
        let changes = ledger.evaluate(
            MissionTime(100.0),
            &assets(120.0),
            &[exposure(7, Some(60.0))],
            &Refuses,
        );
        assert!(matches!(changes[0], WarningChange::Raised { .. }));
        assert!(
            matches!(&changes[1], WarningChange::Failed { reason, .. } if reason == "no transport")
        );
        let again = ledger.evaluate(
            MissionTime(101.0),
            &assets(120.0),
            &[exposure(7, Some(59.0))],
            &Refuses,
        );
        assert!(again.is_empty(), "{again:?}");
        assert_eq!(ledger.failed_count(), 1);
    }

    #[test]
    fn a_taken_warning_is_sent_and_the_trigger_lapsing_closes_it_with_its_state() {
        let mut ledger = WarningLedger::default();
        ledger.evaluate(
            MissionTime(0.0),
            &assets(120.0),
            &[exposure(1, Some(60.0))],
            &Takes,
        );
        assert!(matches!(ledger.open()[0].state, WarningState::Sent { .. }));
        assert!(ledger.acknowledge(AssetId(1), TrackId(1), MissionTime(1.0)));
        let changes = ledger.evaluate(
            MissionTime(2.0),
            &assets(120.0),
            &[exposure(1, None)],
            &Takes,
        );
        assert!(
            matches!(
                &changes[0],
                WarningChange::Closed {
                    final_state: WarningState::Acknowledged { .. },
                    ..
                }
            ),
            "{changes:?}"
        );
        assert!(ledger.open().is_empty());
        assert_eq!(ledger.closed().len(), 1);
    }

    /// GAP-042, DN-03 §5 rule 2: an acknowledgement discharges a warning that was sent,
    /// **and one that went late while waiting for it**, which is the case the whole
    /// acknowledgement path exists for. A warning still `Owed` was never sent, so an
    /// acknowledgement of it is refused rather than recorded.
    #[test]
    fn an_acknowledgement_discharges_a_sent_warning_and_a_late_one() {
        let mut ledger = WarningLedger::default();
        ledger.evaluate(
            MissionTime(0.0),
            &assets(120.0),
            &[exposure(1, Some(60.0))],
            &Takes,
        );
        // Due at 60 s; at 70 s it is sent and late, with nobody having answered.
        let late = ledger.evaluate(
            MissionTime(70.0),
            &assets(120.0),
            &[exposure(1, Some(5.0))],
            &Takes,
        );
        assert!(
            matches!(late.as_slice(), [WarningChange::Late { .. }]),
            "{late:?}"
        );
        assert_eq!(ledger.late_count(), 1);
        assert!(
            ledger.acknowledge(AssetId(1), TrackId(1), MissionTime(71.0)),
            "a late warning that is answered has had its obligation discharged"
        );
        assert_eq!(ledger.late_count(), 0);
        assert!(ledger.open()[0].state.is_discharged());
        assert_eq!(
            ledger.open()[0].state,
            WarningState::Acknowledged {
                at: MissionTime(71.0)
            }
        );

        // Nothing was sent, so there is nothing to have been acknowledged.
        let mut owed = WarningLedger::default();
        owed.evaluate(
            MissionTime(0.0),
            &assets(120.0),
            &[exposure(2, Some(60.0))],
            &Takes,
        );
        owed.open[0].state = WarningState::Owed;
        assert!(!owed.acknowledge(AssetId(1), TrackId(2), MissionTime(1.0)));
        // And a pair the ledger does not hold is refused rather than invented.
        assert!(!owed.acknowledge(AssetId(1), TrackId(99), MissionTime(1.0)));
    }

    #[test]
    fn a_waiver_names_a_person_and_late_is_reported_once() {
        let mut ledger = WarningLedger::default();
        ledger.evaluate(
            MissionTime(0.0),
            &assets(120.0),
            &[exposure(1, Some(60.0))],
            &Refuses,
        );
        assert!(ledger.waive(
            AssetId(1),
            TrackId(1),
            "ops-3".into(),
            "the pilot launch".into(),
            MissionTime(1.0)
        ));
        assert!(!ledger.waive(
            AssetId(1),
            TrackId(9),
            "ops-3".into(),
            "nothing".into(),
            MissionTime(1.0)
        ));
        // An Owed warning past due goes Late exactly once.
        let mut owed = WarningLedger::default();
        owed.evaluate(
            MissionTime(0.0),
            &assets(120.0),
            &[exposure(2, Some(60.0))],
            &Takes,
        );
        // Due is the predicted impact (`raise_due`): at 60 s it is not yet late, at 70 s it is.
        owed.open[0].state = WarningState::Owed;
        let not_yet = owed.evaluate(
            MissionTime(10.0),
            &assets(120.0),
            &[exposure(2, Some(50.0))],
            &Takes,
        );
        assert!(not_yet.is_empty(), "{not_yet:?}");
        let first = owed.evaluate(
            MissionTime(70.0),
            &assets(120.0),
            &[exposure(2, Some(5.0))],
            &Takes,
        );
        assert!(
            matches!(first.as_slice(), [WarningChange::Late { .. }]),
            "{first:?}"
        );
        let second = owed.evaluate(
            MissionTime(71.0),
            &assets(120.0),
            &[exposure(2, Some(4.0))],
            &Takes,
        );
        assert!(second.is_empty(), "{second:?}");
        assert_eq!(owed.late_count(), 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::{AssetExtent, AssetPriority, Geodetic, WarningObligation};

    fn asset(id: u32, lead_time_s: Option<f64>) -> DefendedAsset {
        DefendedAsset {
            id: AssetId(id),
            name: format!("asset-{id}"),
            extent: AssetExtent::Point {
                position: Geodetic {
                    lat_rad: 0.0,
                    lon_rad: 0.0,
                    alt_m: 0.0,
                },
            },
            priority: AssetPriority::High,
            warning: lead_time_s.map(|lead_time_s| WarningObligation {
                lead_time_s,
                channel: "harbour-master".into(),
                within_m: None,
            }),
            note: None,
        }
    }

    fn exposure(asset: u32, time_to_impact_s: Option<f32>) -> AssetExposure {
        AssetExposure {
            asset: AssetId(asset),
            range_m: 1_000.0,
            time_to_impact_s,
            closest_approach_m: Some(0.0),
            time_to_closest_approach_s: None,
        }
    }

    #[test]
    fn a_warning_is_raised_no_later_than_the_lead_time_before_impact() {
        let assets = [asset(1, Some(120.0))];
        // Ninety seconds out, obligation is 120: inside the lead time.
        let due = raise_due(
            &assets,
            &[(TrackId(7), exposure(1, Some(90.0)))],
            &[],
            MissionTime(0.0),
        );
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].asset, AssetId(1));
        assert_eq!(due[0].state, WarningState::Owed);
        assert_eq!(due[0].channel, "harbour-master");
    }

    #[test]
    fn nothing_is_raised_before_the_lead_time_or_without_an_obligation() {
        let with = [asset(1, Some(120.0))];
        assert!(
            raise_due(
                &with,
                &[(TrackId(7), exposure(1, Some(600.0)))],
                &[],
                MissionTime(0.0)
            )
            .is_empty(),
            "still ten minutes out"
        );

        let without = [asset(1, None)];
        assert!(
            raise_due(
                &without,
                &[(TrackId(7), exposure(1, Some(10.0)))],
                &[],
                MissionTime(0.0)
            )
            .is_empty(),
            "no obligation, no warning"
        );
    }

    #[test]
    fn a_track_that_never_arrives_raises_no_warning() {
        let assets = [asset(1, Some(120.0))];
        assert!(raise_due(
            &assets,
            &[(TrackId(7), exposure(1, None))],
            &[],
            MissionTime(0.0)
        )
        .is_empty());
    }

    #[test]
    fn one_open_warning_per_asset_and_track_pair() {
        let assets = [asset(1, Some(120.0))];
        let existing = raise_due(
            &assets,
            &[(TrackId(7), exposure(1, Some(90.0)))],
            &[],
            MissionTime(0.0),
        );
        let again = raise_due(
            &assets,
            &[(TrackId(7), exposure(1, Some(80.0)))],
            &existing,
            MissionTime(10.0),
        );
        assert!(again.is_empty(), "the same pair does not raise twice");
    }

    #[test]
    fn a_second_track_raises_its_own_warning() {
        // Deduplication is mechanical, by pair. Deciding that a second warning is
        // unnecessary is a judgement, and judgements belong with people.
        let assets = [asset(1, Some(120.0))];
        let existing = raise_due(
            &assets,
            &[(TrackId(7), exposure(1, Some(90.0)))],
            &[],
            MissionTime(0.0),
        );
        let second = raise_due(
            &assets,
            &[(TrackId(8), exposure(1, Some(90.0)))],
            &existing,
            MissionTime(0.0),
        );
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].track, TrackId(8));
    }

    #[test]
    fn an_unreachable_endpoint_leaves_the_warning_failed_and_open() {
        let mut w = Warning::new(AssetId(1), TrackId(7), MissionTime(90.0), "x".into());
        w.failed("endpoint unreachable", MissionTime(10.0));
        assert!(
            w.state.is_open(),
            "a failed delivery is not a closed matter"
        );
        assert!(!w.state.is_discharged());
        match &w.state {
            WarningState::Failed { reason } => assert!(!reason.is_empty()),
            other => panic!("expected a failure, got {other:?}"),
        }
        assert_eq!(needs_attention(&[w]).len(), 1);
    }

    #[test]
    fn a_warning_past_due_becomes_late_and_stays_open() {
        let mut warnings = vec![Warning::new(
            AssetId(1),
            TrackId(7),
            MissionTime(90.0),
            "x".into(),
        )];
        assert!(mark_overdue(&mut warnings, MissionTime(50.0)).is_empty());

        let late = mark_overdue(&mut warnings, MissionTime(100.0));
        assert_eq!(late, vec![AssetId(1)]);
        assert_eq!(warnings[0].state, WarningState::Late);
        assert!(
            warnings[0].state.is_open(),
            "it is never closed by the passage of time"
        );
        // And it does not go late twice.
        assert!(mark_overdue(&mut warnings, MissionTime(200.0)).is_empty());
    }

    #[test]
    fn an_acknowledged_warning_is_discharged_and_never_goes_late() {
        let mut warnings = vec![Warning::new(
            AssetId(1),
            TrackId(7),
            MissionTime(90.0),
            "x".into(),
        )];
        warnings[0].sent(MissionTime(10.0));
        warnings[0].acknowledged(MissionTime(20.0));
        assert!(warnings[0].state.is_discharged());
        assert!(mark_overdue(&mut warnings, MissionTime(1_000.0)).is_empty());
    }

    #[test]
    fn a_waiver_records_who_and_why() {
        let mut w = Warning::new(AssetId(1), TrackId(7), MissionTime(90.0), "x".into());
        w.waive(
            "supervisor",
            "the asset is already evacuated",
            MissionTime(30.0),
        );
        assert!(w.state.is_discharged());
        match &w.state {
            WarningState::Waived { operator, reason } => {
                assert_eq!(operator, "supervisor");
                assert!(!reason.is_empty());
            }
            other => panic!("expected a waiver, got {other:?}"),
        }
    }

    #[test]
    fn every_state_change_carries_its_time() {
        let mut w = Warning::new(AssetId(1), TrackId(7), MissionTime(90.0), "x".into());
        w.sent(MissionTime(10.0));
        w.acknowledged(MissionTime(20.0));
        assert_eq!(w.history.len(), 2);
        assert_eq!(w.history[0].at, MissionTime(10.0));
        assert_eq!(w.history[1].at, MissionTime(20.0));
    }
}

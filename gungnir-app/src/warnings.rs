// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The warning function on the desktop (GAP-042, docs/design/DN-03-warning.md).
//!
//! `gungnir-workflow` owns the rule and the record (`raise_due`, `mark_overdue`, and
//! the ledger over them); this module supplies what the rule reads -- the assets from
//! the baseline, this frame's exposures from the same ranking PN-17 lists -- and what it
//! cannot know: the wire. **No wire exists.** The generic endpoint mechanism D-08 settled
//! has no transport built (GAP-040's record half found the same), so every warning
//! raised today is offered to an endpoint that cannot be reached and becomes `Failed`
//! with that reason, loudly, which DN-03 §5 calls better than a warning function that
//! quietly fails. The operator sees it in PN-08 and makes the radio call.
//!
//! **The answer comes back through the node** (GAP-042, 2026-09-06). DN-03 §5 rule 2's
//! third state had no way in: `Warning::acknowledged` had no caller anywhere in the
//! workspace, so a delivered warning sat in `Sent` and went `Late` for ever. The warned
//! party now posts to the node's `POST /v3/warnings/{asset}/{track}/acknowledge`, the node
//! puts it on the record because it holds no ledger of its own, and
//! [`apply_acknowledgement`] discharges the warning here.
//!
//! **A figure the warning cannot give is said to be unavailable** (GAP-176, D-115;
//! `docs/design/DN-03-warning.md` §12). The body posted to the warned party is built by
//! [`endpoint_payload`] with `gungnir_eventing::nonfinite::to_partner_value`, the one
//! implementation exchange uses (D-103): a NaN or an infinity is
//! `{"unavailable": "nan" | "+inf" | "-inf"}` in the number's place, where
//! `serde_json::to_value` wrote `null`, and a warning whose figures are all finite is byte
//! for byte what it was. A warning is never withheld for a figure in it.

use gungnir_eventing::Event;
use gungnir_model::events::WarningEvent;
use gungnir_model::{AssetId, MissionTime, TrackId};
use gungnir_workflow::warning::{
    state_label, Warning, WarningChange, WarningDelivery, WarningState,
};

use crate::state::AppState;

/// The delivery this build has (GAP-040, 2026-09-06): an `http` endpoint is posted to
/// through the transport and the warning is `Sent`, meaning handed to the transport; the
/// endpoint's answer arrives on the tick and a refusal or silence turns it `Failed`. Any
/// other kind has no transport, and the warning fails at once with the reason.
struct EndpointDelivery<'a> {
    endpoints: &'a [gungnir_config::EndpointConfig],
    client: Option<&'a gungnir_remote::endpoint::EndpointClient>,
    posted: std::cell::RefCell<Vec<crate::deliveries::PendingWarning>>,
}

impl WarningDelivery for EndpointDelivery<'_> {
    fn deliver(&self, warning: &Warning) -> Result<(), String> {
        let address = crate::deliveries::http_address_in(
            self.endpoints,
            self.client.is_some(),
            &warning.channel,
        )
        .map_err(|why| format!("{why}; warn by voice"))?;
        let client = self
            .client
            .ok_or_else(|| "no endpoint client; warn by voice".to_string())?;
        let payload = endpoint_payload(warning)?;
        self.posted
            .borrow_mut()
            .push(crate::deliveries::PendingWarning {
                asset: warning.asset,
                track: warning.track,
                endpoint: warning.channel.clone(),
                in_flight: client.post_json(&address, payload),
            });
        Ok(())
    }
}

/// The body a warned party's endpoint is posted (DN-03 §12, GAP-176, D-115).
///
/// **Exactly what `serde_json::to_value` gave when every figure is finite**, so a warned
/// party reading a warning today reads the same bytes. A NaN or an infinity -- a due time
/// taken from a diverged prediction, a transition stamped by a clock that failed -- is
/// DN-18 §15's object `{"unavailable": "nan" | "+inf" | "-inf"}` in the number's place:
/// the party can tell a time this desktop could not give from one that is absent, where
/// `to_value` wrote `null` for both without a word. **A non-finite figure never stops the
/// warning going out**: a warning with one field marked unavailable still warns, and one
/// withheld leaves the party unwarned.
///
/// # Errors
///
/// Only what `serde_json` refuses for another reason -- a map whose keys are not strings,
/// which a warning has none of. The reason ends in "warn by voice", because the ledger
/// turns it into a `Failed` warning and an alert (DN-03 §5: failure is loud), which is
/// what posting `null` in its place used to hide.
pub fn endpoint_payload(warning: &Warning) -> Result<serde_json::Value, String> {
    gungnir_eventing::nonfinite::to_partner_value(warning).map_err(|err| {
        format!("the warning could not be written for its endpoint ({err}); warn by voice")
    })
}

/// Post one warning to its channel's endpoint, as the tick's delivery does: the address
/// from the baseline, the body from [`endpoint_payload`], the desktop's endpoint client.
/// The answer arrives on the returned delivery, which the tick's
/// `crate::deliveries` step reads.
///
/// Public so a test can put a warning carrying a non-finite figure on a real socket
/// through the same code, since the rule raises one only with a finite due time.
///
/// # Errors
///
/// Why the warning was not posted, ending in "warn by voice", verbatim into the record.
pub fn post(
    state: &AppState,
    warning: &Warning,
) -> Result<crate::deliveries::PendingWarning, String> {
    let delivery = EndpointDelivery {
        endpoints: &state.config.endpoints,
        client: state.endpoint_client.as_ref(),
        posted: std::cell::RefCell::new(Vec::new()),
    };
    delivery.deliver(warning)?;
    delivery
        .posted
        .into_inner()
        .pop()
        .ok_or_else(|| "the warning was not handed to the transport; warn by voice".to_string())
}

/// The tick step: evaluate the rule over this frame's exposures, journal every change,
/// alert on the loud ones.
pub fn tick(state: &mut AppState) {
    let now = state.clock.now();
    let assets: Vec<gungnir_model::DefendedAsset> = state
        .config
        .assets
        .iter()
        .map(gungnir_config::AssetConfig::to_asset)
        .collect();
    if !assets.iter().any(|a| a.warning.is_some()) {
        return;
    }
    let ranking = crate::sustainment::asset_exposure(state);
    let exposures: Vec<(TrackId, gungnir_assessment::AssetExposure)> = ranking
        .scores()
        .iter()
        .filter_map(|s| s.exposure.map(|e| (s.track_id, e)))
        .collect();
    let mut ledger = std::mem::take(&mut state.warnings);
    let delivery = EndpointDelivery {
        endpoints: &state.config.endpoints,
        client: state.endpoint_client.as_ref(),
        posted: std::cell::RefCell::new(Vec::new()),
    };
    let changes = ledger.evaluate(now, &assets, &exposures, &delivery);
    let posted = delivery.posted.into_inner();
    state.warnings = ledger;
    state.pending_warnings.extend(posted);
    for change in changes {
        let (event, alert) = describe(state, &change, now);
        if let Some(text) = alert {
            state.alerts.push(text);
        }
        crate::update::publish(state, now, Event::Warning(event));
    }
}

fn asset_name(state: &AppState, asset: AssetId) -> String {
    state
        .config
        .assets
        .iter()
        .find(|a| a.id == asset.0)
        .map_or_else(|| format!("asset {}", asset.0), |a| a.name.clone())
}

fn describe(
    state: &AppState,
    change: &WarningChange,
    now: MissionTime,
) -> (WarningEvent, Option<String>) {
    match change {
        WarningChange::Raised {
            asset,
            track,
            due_by,
        } => (
            WarningEvent::Raised {
                asset: *asset,
                track: *track,
                due_by: *due_by,
                at: now,
            },
            None,
        ),
        WarningChange::Sent { asset, track } => (
            WarningEvent::Sent {
                asset: *asset,
                track: *track,
                at: now,
            },
            None,
        ),
        WarningChange::Failed {
            asset,
            track,
            reason,
        } => (
            WarningEvent::Failed {
                asset: *asset,
                track: *track,
                reason: reason.clone(),
                at: now,
            },
            Some(format!(
                "warning owed to {} because of track {} was not delivered: {reason}",
                asset_name(state, *asset),
                track.0
            )),
        ),
        WarningChange::Late { asset, track } => (
            WarningEvent::Late {
                asset: *asset,
                track: *track,
                at: now,
            },
            Some(format!(
                "warning owed to {} because of track {} is late",
                asset_name(state, *asset),
                track.0
            )),
        ),
        WarningChange::Closed {
            asset,
            track,
            final_state,
        } => (
            WarningEvent::Closed {
                asset: *asset,
                track: *track,
                final_state: state_label(final_state).to_string(),
                at: now,
            },
            None,
        ),
    }
}

/// Apply an acknowledgement the warned party sent through the node (GAP-042, DN-03 §5
/// rule 2).
///
/// The mirror of `handoffs::apply_report`, and for the same reason: the node holds no
/// warning ledger, so it records the fact for every desktop and the one that raised the
/// warning is the only place that can apply it. An acknowledgement naming a pair this
/// desktop has no warning open for is **rejected and said**, as an untrusted external
/// input is -- the node could not know, and silently discarding it would leave the party
/// believing it had answered.
///
/// Nothing is re-published here. The acknowledgement is already on the record as the
/// event that carried it, and publishing a second one would put the same fact on the
/// stream twice.
pub fn apply_acknowledgement(
    state: &mut AppState,
    asset: AssetId,
    track: TrackId,
    party: &str,
    at: MissionTime,
) {
    let name = asset_name(state, asset);
    let outcome = if state.warnings.acknowledge(asset, track, at) {
        format!(
            "warning owed to {name} because of track {} was acknowledged by {party}",
            track.0
        )
    } else {
        format!(
            "{party} acknowledged a warning owed to {name} because of track {}, and this \
             desktop has no warning open for that pair which was ever sent",
            track.0
        )
    };
    crate::audit::record(
        state,
        gungnir_security::actions::ACKNOWLEDGE_WARNING,
        outcome.clone(),
    );
    state.alerts.push(outcome);
}

/// A person waives an open warning; journaled with their name, or with "nobody signed
/// in", which is the truth rather than a default operator.
pub fn waive(state: &mut AppState, asset: AssetId, track: TrackId, reason: String) -> bool {
    let now = state.clock.now();
    let operator = state
        .attributed_operator()
        .map_or_else(|| "nobody signed in".to_string(), |o| o.0.to_string());
    let waived = state
        .warnings
        .waive(asset, track, operator.clone(), reason.clone(), now);
    if waived {
        crate::update::publish(
            state,
            now,
            Event::Warning(WarningEvent::Waived {
                asset,
                track,
                operator,
                reason,
                at: now,
            }),
        );
    }
    waived
}

/// The open warnings as PN-08 and PN-04 list them: late and failed first, then soonest
/// due (DN-03 §7).
#[must_use]
pub fn lines(
    state: &AppState,
    only_track: Option<TrackId>,
) -> Vec<gungnir_ui::panels::alerts::WarningLine> {
    let now = state.clock.now();
    let mut lines: Vec<_> = state
        .warnings
        .open()
        .iter()
        .filter(|w| only_track.is_none_or(|t| w.track == t))
        .map(|w| gungnir_ui::panels::alerts::WarningLine {
            asset: asset_name(state, w.asset),
            track: w.track.0,
            channel: w.channel.clone(),
            state: state_label(&w.state),
            detail: match &w.state {
                WarningState::Failed { reason } => reason.clone(),
                WarningState::Waived { operator, reason } => format!("{operator}: {reason}"),
                _ => String::new(),
            },
            remaining_s: w.due_by.0 - now.0,
            loud: matches!(w.state, WarningState::Late | WarningState::Failed { .. }),
        })
        .collect();
    lines.sort_by(|a, b| {
        b.loud
            .cmp(&a.loud)
            .then(a.remaining_s.total_cmp(&b.remaining_s))
    });
    lines
}

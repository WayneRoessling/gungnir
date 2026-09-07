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
//! party now posts to the node's `POST /v2/warnings/{asset}/{track}/acknowledge`, the node
//! puts it on the record because it holds no ledger of its own, and
//! [`apply_acknowledgement`] discharges the warning here.

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
        let payload = serde_json::to_value(warning).unwrap_or(serde_json::Value::Null);
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

// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Assembling PN-01's view from `AppState` (GAP-072).
//!
//! `gungnir_ui::panels::status_strip` draws the strip and knows nothing about where
//! the values come from; this is the projection that supplies them, and like the rest
//! of the binary it is wiring rather than logic (AP-13).
//!
//! Three of the eight elements need a note, because what is available is not what the
//! specification eventually wants:
//!
//! * **Backend outbox counts.** `docs/ux/information-architecture.md` §2 wants
//!   "Detached, N queued, M dropped" from `RemoteTrackingService`. `AppState` holds a
//!   `Box<dyn TrackingService>`, and that trait has no outbox: the counters belong to
//!   the remote implementation. Until the transport exists (GAP-041) there is no
//!   detached state to report, so a remote backend is shown as configured-but-not-yet-
//!   connectable rather than with invented counts.
//! * **Alert lifecycle counts.** `AppState.alerts` is a `Vec<String>`. The lifecycle
//!   states live in `gungnir_workflow::AlertLifecycle` and are not wired to the
//!   desktop, so the strip is given [`AlertSummary::Unclassified`] and says so.
//! * **Health reasons.** `SystemHealth` carries three booleans and no reason. The
//!   hover text in the strip states the *known* reason for each service where there is
//!   one -- the tracking flag now follows `pipeline_alive`, since
//!   `PIPELINE_IMPLEMENTED` is true (GAP-011), so a false flag means the pipeline
//!   stopped rather than that there is no pipeline -- and a generic one otherwise.

use crate::state::AppState;
use gungnir_config::BackendConfig;
use gungnir_mission::MissionState;
use gungnir_model::EffectorLayer;
use gungnir_time::ClockSource as TimeClockSource;
use gungnir_ui::panels::sensor_health::{SensorHealthLine, SensorPresence};
use gungnir_ui::panels::status_strip::{
    AlertSummary, BackendStatus, ClockSource, ControlStatusLine, DelegationLine, SessionStatus,
    StatusStripView,
};
use gungnir_ui::panels::status_strip::{BaselineValidity, LinkFreshness};

/// The four effector layers, in the order the strip shows them.
const LAYERS: [EffectorLayer; 4] = [
    EffectorLayer::Area,
    EffectorLayer::Point,
    EffectorLayer::SelfDefence,
    EffectorLayer::NonKinetic,
];

/// Owned buffers the borrowed [`StatusStripView`] points into.
///
/// The view borrows, so something has to own. This is built once per frame and dropped
/// with it; it holds only what cannot be borrowed straight out of `AppState`, which is
/// the two derived lists.
#[derive(Debug, Default)]
pub struct StatusStripData<'a> {
    /// The coverage report for this frame, owned because the strip's view borrows what
    /// it reports (DN-12 §7).
    coverage: Option<gungnir_analytics::CoverageReport>,
    control_status: Vec<ControlStatusLine>,
    delegations: Vec<DelegationLine<'a>>,
    role_name: String,
    /// The signed-in operator's role, by name, for the operator line (GAP-057).
    session_role: String,
    /// The rehearsal in progress, named (GAP-089).
    rehearsal: Option<String>,
}

/// One health line per configured sensor, saying which of the four things a silent sensor
/// can mean is in force (DN-21 §5, GAP-054).
///
/// **The order of the checks is the design.** An overrun is tested before maintenance,
/// because a window that closed with the sensor still down is no longer an expected
/// absence -- reading it as one is precisely how a scheduled outage becomes an unnoticed
/// hole. Maintenance is tested before failure for the mirror-image reason: a sensor down
/// inside its window is not a fault, and alerting on it trains an operator to ignore the
/// panel.
#[must_use]
pub fn sensor_health_lines(state: &AppState) -> Vec<SensorHealthLine<'_>> {
    use gungnir_model::MaintenanceState;
    use gungnir_sensor_management::SensorRegistry;

    let now = state.clock.now();
    state
        .sensors
        .sensors()
        .iter()
        .map(|s| {
            let overrun = s
                .maintenance
                .iter()
                .find(|w| w.state == MaintenanceState::Overrun);
            let open = s.maintenance.iter().find(|w| w.is_open_at(now));
            let presence = match (overrun, open, s.is_down()) {
                (Some(w), _, _) => SensorPresence::Overrun {
                    window_closed: w.to,
                    reason: w.reason.as_str(),
                },
                (None, Some(w), true) => SensorPresence::InMaintenance {
                    until: w.to,
                    reason: w.reason.as_str(),
                },
                // A sensor still radiating during its own window is radiating. The
                // schedule said it would be down; it is not, and coverage reports what it
                // can actually see.
                (None, _, false) => SensorPresence::Radiating,
                (None, None, true) => SensorPresence::Failed,
            };
            SensorHealthLine {
                id: s.id.0,
                modality: s.modality.as_str(),
                presence,
            }
        })
        .collect()
}

/// Judge a link's silence against the node's heartbeat (D-23).
///
/// The UI crate cannot see the transport's constants, so the arithmetic lives here: inside
/// two beats is heard, past that is overdue. Two rather than one, because a beat that
/// arrives fractionally late must not flicker the strip red every few seconds.
#[must_use]
pub fn link_freshness(last_heard: Option<std::time::Duration>) -> LinkFreshness {
    let Some(age) = last_heard else {
        return LinkFreshness::NeverHeard;
    };
    let age_s = age.as_secs_f32();
    if age <= gungnir_remote::link::HEARTBEAT_INTERVAL * 2 {
        LinkFreshness::Heard { age_s }
    } else {
        LinkFreshness::Overdue { age_s }
    }
}

/// Where the baseline stands against its validity window, at the clock this session
/// runs on (DN-08 §5 and §7, GAP-052).
///
/// Four states, not a boolean: **an unconfigured window and a valid one are different
/// claims**, and so are "not yet" and "expired". An operator whose plans have started
/// being superseded has to be able to tell which of those happened.
#[must_use]
pub fn baseline_validity(state: &AppState) -> BaselineValidity {
    let now = state.clock.now();
    match state.config.validity {
        None => BaselineValidity::NoWindowConfigured,
        Some(w) if now < w.valid_from => BaselineValidity::NotYet { from: w.valid_from },
        Some(w) => match w.valid_until {
            Some(until) if now > until => BaselineValidity::Expired { since: until },
            until => BaselineValidity::InForce { until },
        },
    }
}

impl<'a> StatusStripData<'a> {
    /// Derive the two lists and the role label from the state.
    #[must_use]
    pub fn from_state(state: &'a AppState) -> Self {
        let settings = &state.config.policy.control_status;
        let control_status = LAYERS
            .iter()
            .map(|layer| ControlStatusLine {
                layer: *layer,
                status: settings.for_layer(*layer),
                // `for_layer` returns Hold for an unconfigured layer by design, so the
                // map is the only place that knows whether anyone set it.
                configured: settings.by_layer.contains_key(layer),
            })
            .collect();

        // The pre-delegated authority rules of D-15. Their action and role strings
        // live in the baseline, which outlives every frame, so the strip borrows them
        // rather than copying: this is the projection the UI standards ask for, not a
        // second copy of the policy.
        let delegations = state
            .config
            .policy
            .authority
            .rules
            .iter()
            .filter(|r| r.pre_delegated)
            .map(|r| DelegationLine {
                action: r.action.as_str(),
                role: r.role.as_str(),
                layer: r.layer,
                class: r.class.as_deref(),
            })
            .collect();

        Self {
            coverage: crate::sustainment::coverage_report(state),
            control_status,
            delegations,
            role_name: format!("{:?}", state.role()),
            session_role: match state.session_state() {
                gungnir_security::SessionState::SignedIn(s) => format!("{:?}", s.role),
                _ => String::new(),
            },
            rehearsal: state
                .rehearsal
                .as_ref()
                .map(crate::rehearsal::Rehearsal::label),
        }
    }
}

/// Build the strip's view for this frame.
#[must_use]
/// The encryption status as PN-01 words it.
///
/// The mapping lives here rather than in the panel because `gungnir-ui` may not depend on
/// `gungnir-security`, and because which of the two off states applies is a fact about
/// the deployment rather than about the drawing.
pub fn encryption_state(
    status: &gungnir_security::EncryptionStatus,
) -> gungnir_ui::panels::status_strip::EncryptionState<'_> {
    use gungnir_security::EncryptionStatus;
    use gungnir_ui::panels::status_strip::EncryptionState;
    match status {
        EncryptionStatus::Active { .. } => EncryptionState::Active,
        EncryptionStatus::UnavailableWritingPlaintext { reason } => {
            EncryptionState::UnavailableWritingPlaintext { reason }
        }
        EncryptionStatus::NotConfigured => EncryptionState::NotConfigured,
    }
}

pub fn status_strip_view<'a>(
    state: &'a AppState,
    data: &'a StatusStripData<'a>,
) -> StatusStripView<'a> {
    let backend = match &state.backend {
        BackendConfig::Embedded => BackendStatus::Embedded,
        // The transport exists (GAP-041) and connecting is an authenticated act
        // (GAP-057): nobody is signed in when the desktop starts and nothing calls
        // `connect` afterwards -- there is no sign-in surface -- so `build_backends`
        // always returns `Embedded` and this arm is unreachable today. It stays honest
        // for the day it is reached: never connected, nothing heard, counts not invented.
        // (This comment used to say "no transport exists yet", which stopped being true
        // the day GAP-041 landed.)
        BackendConfig::Remote { endpoint } => BackendStatus::Node {
            endpoint,
            connected: false,
            queued: 0,
            dropped: 0,
            freshness: LinkFreshness::NeverHeard,
        },
    };

    let session = state.mission.as_ref().map(|m| SessionStatus {
        id: m.session.0,
        // Exhaustive on purpose: a new session state must be given a label here
        // rather than falling into one that means something else.
        state: match m.state {
            MissionState::Created => "Created",
            MissionState::Live => "Live",
            MissionState::Paused => "Paused",
            // Distinct from both Paused and Closed on purpose: this session was live and
            // was never closed, so the record has a hole in it that nobody chose.
            MissionState::Interrupted => "Interrupted (was not closed)",
            MissionState::Replaying => "Replaying",
            MissionState::Closed => "Closed",
        },
    });

    StatusStripView {
        validity: baseline_validity(state),
        // Only when the deployment had a choice to make: one profile, or none, gains
        // nothing from an element that is always the same (DN-24 §9).
        profile: (state.config.mission_profiles.len() > 1)
            .then_some(state.config.active_profile.as_deref())
            .flatten(),
        encryption: encryption_state(&state.encryption),
        backend,
        session,
        mission_time: state.clock.now(),
        clock_source: match state.clock.source() {
            TimeClockSource::Wall => ClockSource::Wall,
            TimeClockSource::Replay => ClockSource::Replay,
        },
        health: state.health,
        control_status: &data.control_status,
        delegations: &data.delegations,
        // The desktop's alerts are a flat list of strings; their lifecycle states are
        // not tracked, and the strip says so rather than printing three zeros.
        alerts: AlertSummary::Unclassified {
            total: state.alerts.len(),
        },
        role: &data.role_name,
        // D-12: the deployment's words, from the baseline in force.
        vocabulary: &state.config.vocabulary,
        // DN-12 §7: computed fresh per frame from the baseline's approaches.
        coverage: crate::sustainment::coverage_status(state, data.coverage.as_ref()),
        rehearsal: data.rehearsal.as_deref(),
        operator: operator_line(state, &data.session_role),
    }
}

/// PN-01's operator line from the session authority (GAP-057).
fn operator_line<'a>(
    state: &AppState,
    role_name: &'a str,
) -> gungnir_ui::panels::status_strip::OperatorLine<'a> {
    use gungnir_security::SessionState;
    use gungnir_ui::panels::status_strip::OperatorLine;
    match state.session_state() {
        SessionState::SignedIn(s) => OperatorLine::SignedIn {
            operator: s.operator.0,
            role: role_name,
            expires_in_s: s.remaining(state.clock.now().0),
        },
        SessionState::NobodySignedIn => OperatorLine::NobodySignedIn,
        SessionState::Expired { operator, .. } => OperatorLine::Expired {
            operator: operator.0,
        },
        SessionState::StoreUnavailable { .. } => OperatorLine::StoreUnavailable,
    }
}

#[cfg(test)]
mod tests {
    /// D-23: silence is judged against the beat, and never heard is its own state.
    #[test]
    fn link_freshness_is_judged_against_the_heartbeat() {
        use gungnir_remote::link::HEARTBEAT_INTERVAL;
        use gungnir_ui::panels::status_strip::LinkFreshness;
        assert_eq!(super::link_freshness(None), LinkFreshness::NeverHeard);
        assert!(matches!(
            super::link_freshness(Some(HEARTBEAT_INTERVAL / 2)),
            LinkFreshness::Heard { .. }
        ));
        // Fractionally late is still heard: one slow beat must not flicker the strip.
        assert!(matches!(
            super::link_freshness(Some(
                HEARTBEAT_INTERVAL + std::time::Duration::from_millis(300)
            )),
            LinkFreshness::Heard { .. }
        ));
        assert!(matches!(
            super::link_freshness(Some(HEARTBEAT_INTERVAL * 3)),
            LinkFreshness::Overdue { .. }
        ));
    }

    use super::*;

    /// Every effector layer appears, so a layer cannot silently vanish from the strip
    /// and leave an operator believing it is unrestricted.
    #[test]
    fn every_effector_layer_is_shown() {
        assert_eq!(LAYERS.len(), 4);
        for layer in [
            EffectorLayer::Area,
            EffectorLayer::Point,
            EffectorLayer::SelfDefence,
            EffectorLayer::NonKinetic,
        ] {
            assert!(
                LAYERS.contains(&layer),
                "{layer:?} is missing from the strip"
            );
        }
    }

    /// An empty baseline puts every layer at Hold and marks none configured. Both
    /// halves matter: Hold is the safe status, and "nobody set it" is the honest reason.
    #[test]
    fn an_empty_baseline_is_hold_everywhere_and_configured_nowhere() {
        let settings = gungnir_model::policy_settings::ControlStatusSettings::default();
        for layer in LAYERS {
            assert_eq!(
                settings.for_layer(layer),
                gungnir_model::WeaponsControlStatus::Hold
            );
            assert!(!settings.by_layer.contains_key(&layer));
        }
    }
}

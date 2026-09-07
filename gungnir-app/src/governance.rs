//! Which algorithm configuration this deployment is running, and how it got there
//! (GAP-086, `docs/design/DN-24-mission-profiles-and-algorithm-baselines.md`).
//!
//! # What this is, and what it is not
//!
//! `gungnir-modelops` has existed since the scaffold and **no crate imported it**: the
//! registry, the promotion state machine and rollback were unreachable from any running
//! system. This module is what constructs one, from the baseline, through the real state
//! machine.
//!
//! **The promoted configuration now filters**, which it did not when this module was
//! written: `PIPELINE_IMPLEMENTED` is true (GAP-011) and `crate::state` builds the tracker
//! with `PipelineSettings::from_baseline` (GAP-053), so the record kept here and the
//! picture on screen are the same configuration. DN-24 §7's rule is held rather than
//! deferred: the tracking service stamps an `AlgorithmBaselineId` into `Provenance` only
//! when it applied one, and a baseline naming a filter outside `IMPLEMENTED_FILTERS` leaves
//! `UNGOVERNED_ALGORITHM_VERSION` standing with an alert rather than running the default
//! silently under that baseline's identity.
//!
//! # Why promotion is not offered here
//!
//! DN-24 §6 said `actions::PROMOTE_MODEL` would get its first check in this increment, and
//! §9 deferred the panel that would make a runtime promotion reachable. Both cannot be
//! true. **Nothing here promotes at runtime**, so nothing checks that action: promotion in
//! this increment is what the baseline declares, and changing it is `config.apply` — which
//! is authorized, audited, and already built. Adding an authority check no caller can reach
//! would be the "implemented but unwired" pattern this register keeps finding, one layer
//! deeper.

use crate::state::AppState;
use gungnir_eventing::Event;
use gungnir_model::events::GovernanceEvent;
use gungnir_model::{AlgorithmBaselineId, MissionProfile};
use gungnir_modelops::{InMemoryModelRegistry, ModelRegistry};

/// What a deployment is governing, once the baseline has been read.
#[derive(Debug)]
pub struct Governance {
    registry: InMemoryModelRegistry,
    profile: Option<MissionProfile>,
    /// Why there is no registry, when there is none.
    ///
    /// Held rather than logged and dropped: a console that cannot say which algorithm
    /// configuration it is running should be able to say *why* it cannot.
    unavailable: Option<String>,
}

impl Governance {
    /// Build the registry the baseline describes.
    ///
    /// A baseline whose promoted candidate fails the registry's own gate does not stop the
    /// desktop: it is reported and this deployment governs nothing, which is honest and
    /// visible. Refusing to start would take a console away from an operator over a
    /// configuration question they can see on PN-14.
    #[must_use]
    pub fn from_config(config: &gungnir_config::ConfigBaseline) -> Self {
        match InMemoryModelRegistry::from_baseline(config) {
            Ok(registry) => Self {
                registry,
                profile: config.operating_profile(),
                unavailable: None,
            },
            Err(err) => Self {
                registry: InMemoryModelRegistry::new(),
                profile: config.operating_profile(),
                unavailable: Some(err.to_string()),
            },
        }
    }

    /// The profile this deployment is operating in, when it declares one.
    #[must_use]
    pub fn profile(&self) -> Option<&MissionProfile> {
        self.profile.as_ref()
    }

    /// The baseline in force for the operating profile.
    #[must_use]
    pub fn in_force(&self) -> Option<&gungnir_modelops::ModelBaseline> {
        self.registry.promoted(self.profile.as_ref()?)
    }

    /// Every candidate in the operating profile, promoted one first in registration order.
    #[must_use]
    pub fn candidates(&self) -> Vec<gungnir_modelops::ModelBaseline> {
        self.profile
            .as_ref()
            .map(|p| self.registry.candidates(p))
            .unwrap_or_default()
    }

    /// Every profile this deployment declared.
    #[must_use]
    pub fn profiles(&self) -> Vec<MissionProfile> {
        self.registry.profiles()
    }

    /// Why nothing is governed, when nothing is.
    #[must_use]
    pub fn unavailable(&self) -> Option<&str> {
        self.unavailable.as_deref()
    }
}

/// Journal what was in force when the session opened.
///
/// **Not a `Promoted` event.** Nobody promoted anything: the file said what is in force, and
/// recording it as an act somebody took would put a name in the record that belongs to no
/// one — there is nobody signed in while a session is being built.
///
/// A deployment governing nothing publishes `NoneInForce` rather than staying silent,
/// because "nothing was in force" and "we did not record it" are opposite claims to whoever
/// reads the journal afterwards.
pub fn publish_in_force(state: &mut AppState) {
    let now = state.clock.now();
    let event = match state.governance.in_force() {
        Some(baseline) => GovernanceEvent::InForceAtStart {
            baseline: baseline.id.clone(),
            at: now,
        },
        None => GovernanceEvent::NoneInForce {
            profile: state.governance.profile().cloned(),
            at: now,
        },
    };
    crate::update::publish(state, now, Event::Governance(event));
}

/// The one-line answer to "what is this console running", for a panel or a log.
#[must_use]
pub fn in_force_label(governance: &Governance) -> String {
    if let Some(reason) = governance.unavailable() {
        return format!("no algorithm configuration is in force: {reason}");
    }
    match governance.in_force() {
        Some(b) => format!("{}", b.id),
        // Distinct wording from the error case above, because a deployment that declared
        // nothing and one whose declaration was refused are different situations.
        None => "no algorithm configuration is declared".to_owned(),
    }
}

/// The identity a track produced under this configuration would carry, once anything
/// applies it (DN-24 §7).
///
/// Returned rather than stamped: **the tracking service must not claim a governed
/// configuration produced a track until it applies one.** Since GAP-053 something does --
/// `crate::state` stamps through `LiveTrackingService::with_algorithm_baseline` when the
/// baseline is one this build can apply -- so this is the identity a baseline *would*
/// carry, and the gap between the two is a baseline this build cannot run.
#[must_use]
pub fn would_stamp(governance: &Governance) -> Option<AlgorithmBaselineId> {
    governance.in_force().map(|b| b.id.clone())
}

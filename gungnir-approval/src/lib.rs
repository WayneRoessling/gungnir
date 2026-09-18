// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The decision path both binaries run (GAP-131, D-57,
//! `docs/design/DN-31-node-approval-queue.md` §3).
//!
//! Until this crate existed the policy chain over a plan, the queue's feeding and sweep,
//! deciding with engagement opening and **the one handoff builder**, and handoff delivery
//! bookkeeping lived in `gungnir-app` alone. A binary cannot depend on a binary, so a node
//! could only have run the same path from a second copy of the safety rules -- which is the
//! duplication D-55 refused and the reason D-57 put them here, one level under the binaries
//! (edge (w), `docs/design/dependency-edges.md` §17).
//!
//! # Where the boundary sits, and why
//!
//! The desk owns the state the rules act on -- the queue, the engagements, the handoffs and
//! what is owed to an endpoint -- and owns nothing about how a host shows or carries any of
//! it. Everything it needs to *read* arrives in an [`ApprovalContext`] the host builds for
//! the call, and everything it needs to *do* leaves through [`ApprovalHost`]. So the two
//! things a host differs in are the two things it supplies: what the picture is right now,
//! and where an effect goes. Nothing here draws a panel, reads a session, or opens a socket.
//!
//! **Delivery goes through [`HandoffTransport`]**, which each binary implements with
//! `gungnir-remote`'s endpoint client (DN-31 §3 point 5, §4's refused edge). A
//! productization crate that reached the client transport would be the edge D-57 turned
//! down, and the retry rule -- a handoff is never dropped (DN-07 §5 case 3) -- belongs with
//! the record it protects rather than with the socket.
//!
//! # What this crate deliberately does not decide
//!
//! It never executes anything: a handoff is built only from a `DecisionRecord` that
//! [`gungnir_command::DecisionRecord::is_actionable`] admits, which is contract C-01 and
//! what `gungnir-app/tests/no_execution_without_decision.rs` pins. It holds no queue of its
//! own: `gungnir-command` owns the queue, the deadlines and the append-only record
//! (DN-31 §3). And it takes no view on who may be asked -- the authority matrix is
//! `gungnir-policy`'s and the permissions are `gungnir-security`'s.

pub mod chain;
pub mod deliveries;
pub mod engagements;
pub mod handoffs;
pub mod queue;

pub use chain::{
    chain_report, chain_report_for, escalation_ladder, evaluate, fires_checks, friendly_positions,
    ladder_roles, may_override, offer_to, with_chain, Offering, PolicyChainReport, CHAIN_ENGINES,
    DECISION_ACTION,
};
pub use deliveries::{
    http_address_in, DeliveryAnswer, HandoffInFlight, HandoffTransport, PendingHandoff, HTTP_KIND,
    RETRY_AFTER_S,
};
pub use handoffs::HandoffRecord;
pub use queue::{DenialHistory, QueueOutcomeCounts, Submitted};

use gungnir_command::InMemoryApprovalWorkflow;
use gungnir_config::ConfigBaseline;
use gungnir_eventing::Event;
use gungnir_intercept_service::engagement::Engagement;
use gungnir_model::{
    DecisionId, DecisionSettings, DeconflictionCheck, Geodetic, MissionTime, ResourceView,
    TrackView,
};
use gungnir_policy::GeoService;
use gungnir_security::Role;
use std::collections::HashSet;

/// The verified session a decision is attributed to (D-53, DN-23 §5 rule 1).
///
/// Both halves or neither: a role is never recorded without the operator it was verified
/// for, because D-03's arbitration rule would otherwise rank a role nobody authenticated as
/// though somebody had. The host reads them from one session state, so an expiry between two
/// reads cannot leave one without the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedIn {
    pub operator: String,
    pub role: String,
}

/// What the desk judges against, built by the host for each call (DN-31 §3).
///
/// Borrowed rather than held, because every field is state some host already owns and a
/// second copy would be a second thing to keep in step. It carries the picture and the
/// baseline; what the *policy chain* additionally reads is [`PolicyInputs`], which is
/// separate for the reason given there.
pub struct ApprovalContext<'a> {
    /// The host's clock, read once for the call so two effects of one act cannot land at
    /// two times.
    pub now: MissionTime,
    /// The role this host acts in: the signed-in account's, or a selected one where the
    /// host allows that with nobody signed in (DN-23 §5 rule 5). Every authority question
    /// asks about this one, so a queue offer and the chain's asking role cannot disagree.
    pub role: Role,
    /// Who a decision is attributed to, and `None` when nobody is signed in.
    pub signed_in: Option<SignedIn>,
    pub tracks: &'a [TrackView],
    pub resources: &'a [ResourceView],
    pub config: &'a ConfigBaseline,
    /// Whether a plan produced now is superseded rather than applied (DN-08 §5).
    ///
    /// Passed in rather than derived here: the host owns the validity window and already
    /// reports it on its status strip, and the type that names the four states it can be in
    /// lives in the user-interface layer, which a productization crate may not reach. One
    /// rule, in the host, read by both.
    pub baseline_supersedes_plans: bool,
    /// Whether D-15's delegations are in force for this host (DN-31 §6.7; GAP-134).
    ///
    /// Passed in for the same reason as the field above: whether this host has lost its
    /// node, and for how long, is the host's to know. A node and a linked desktop pass
    /// [`gungnir_policy::Delegations::AsConfigured`]; a desktop cut off for longer than
    /// the configured interval passes `Lapsed`, and every authority question the desk
    /// asks -- the chain's verdict, the offering, the delegation flag -- is then asked of
    /// the matrix without the delegated rules ([`ApprovalContext::authority`]).
    pub delegations: gungnir_policy::Delegations,
}

impl<'a> ApprovalContext<'a> {
    /// The role as the record and the authority rules spell it.
    #[must_use]
    pub fn role_name(&self) -> String {
        format!("{:?}", self.role)
    }

    /// The same picture, asked about a different role (DN-31 §6.1).
    ///
    /// What lets [`chain::offer_to`] ask the authority engine about every role on the
    /// ladder rather than about the one at a console. The borrows are re-borrowed, not
    /// copied, so the answer is about the same clock and the same tracks the caller read
    /// -- asking two roles about two pictures would be a race with itself.
    ///
    /// **`signed_in` travels unchanged**, because who is asking and who is deciding are
    /// different questions: a node walks the ladder with nobody signed in, and a decision
    /// still records only the session that took it (DN-23 §5 rule 1).
    #[must_use]
    pub fn as_role(&'a self, role: Role) -> ApprovalContext<'a> {
        ApprovalContext {
            now: self.now,
            role,
            signed_in: self.signed_in.clone(),
            tracks: self.tracks,
            resources: self.resources,
            config: self.config,
            baseline_supersedes_plans: self.baseline_supersedes_plans,
            delegations: self.delegations,
        }
    }

    /// The authority matrix in force for this host, with D-15's delegations applied
    /// (DN-31 §6.7; GAP-134).
    ///
    /// **The one place the desk reads the matrix from.** Reading
    /// `config.policy.authority` directly would ask a lapsed desktop's questions of a
    /// matrix that still holds the delegation, and the item would stay actionable by the
    /// role that lost it -- which is the lapse reaching a panel and not the queue.
    #[must_use]
    pub fn authority(&self) -> std::borrow::Cow<'a, gungnir_model::AuthoritySettings> {
        gungnir_policy::authority_in_force(&self.config.policy.authority, self.delegations)
    }
}

/// What the policy chain reads beyond [`ApprovalContext`].
///
/// Separate because the chain runs when a plan is proposed and the sweeps run on every
/// tick: a host that is only expiring items or polling an endpoint should not have to build
/// a geofence service and place every friendly track to do it. Both are still the host's to
/// supply -- the fences come from the baseline it loaded, and the friendly positions can
/// only be placed in the frame it declares.
pub struct PolicyInputs<'a> {
    /// Every fence the baseline declares, as the geofence engine reads them.
    pub geofences: &'a dyn GeoService,
    /// The friendly positions the picture can place, from [`friendly_positions`], and
    /// `None` where they cannot be placed at all (DN-05 §5 rule 1).
    pub friendly_positions: Option<&'a [Geodetic]>,
}

/// Where the desk's effects go (DN-31 §3).
///
/// Four, and each is something only a host can do: put an event on its bus, put a sentence
/// in front of the person at the console, write an audit entry attributed to whoever it has
/// verified, and republish its handoff set for coalition exchange. The desk decides *that*
/// each happens and *what it says*; the host decides where it lands.
pub trait ApprovalHost: HandoffTransport {
    /// Publish one event, which the host journals. A publish that fails is logged by the
    /// host rather than returned: nothing the desk could do about it would be safe.
    fn publish(&mut self, at: MissionTime, event: Event);

    /// Say something to the person at the console.
    fn alert(&mut self, message: String);

    /// One audit entry per gated action (contract C-04), attributed to the operator the
    /// host has verified and to nobody when it has verified none (DN-23 §5 rule 1).
    fn audit(&mut self, action: &str, detail: String);

    /// This host's whole current handoff set, for coalition exchange (GAP-065, DN-18 §5
    /// amendment 2). A no-op for a host with nowhere to publish it.
    ///
    /// Every handoff, not only the ones marked releasable: the node's two gates -- the
    /// agreement and the marking -- decide at serve time what a party may see, and
    /// filtering here as well would put one of those decisions in two places, which is
    /// what DN-18 §8's criterion exists to catch.
    fn republish_handoffs(&mut self, handoffs: &[HandoffRecord]);
}

/// The approval desk: the queue's state and everything the decision path holds between
/// calls (DN-31 §3).
///
/// One struct rather than loose fields on each host, so a host cannot end up holding a
/// handoff for a decision another part of it never opened an engagement for. A host owns
/// exactly one.
#[derive(Debug, Default)]
pub struct ApprovalDesk {
    /// The queue and the append-only decision record (`gungnir-command`).
    pub approvals: InMemoryApprovalWorkflow,
    /// Engagements opened on actionable decisions (GAP-043, DN-06). Session state: the
    /// journal carries the events, and a replay rebuilds the outcomes from them.
    pub engagements: Vec<Engagement>,
    /// Handoffs issued from actionable decisions and where their delivery stands
    /// (GAP-040, DN-07).
    pub handoffs: Vec<HandoffRecord>,
    /// Handoffs posted and not yet answered, or waiting to be posted again.
    pub pending_handoffs: Vec<PendingHandoff>,
    /// Decisions whose engaged track has been in the picture since they opened, so a
    /// track's absence can be read as "left" rather than "never arrived".
    pub engaged_seen: HashSet<DecisionId>,
    /// The last denial the chain returned, kept so an empty queue can say why.
    pub denials: DenialHistory,
    /// Expiries and escalations this session (GAP-034), which PN-17 reports as the honest
    /// measure of whether the queue is keeping up.
    pub queue_outcomes: QueueOutcomeCounts,
    /// The fires checks for the last submitted plan, every one with its result (GAP-036,
    /// DN-05 §7). Empty when the last plan was not a fires task.
    pub fires_checks: Vec<DeconflictionCheck>,
}

impl ApprovalDesk {
    /// A desk whose queue is timed by this deployment's decision settings.
    ///
    /// The settings are held by the workflow rather than passed per call, so a queue cannot
    /// end up with items timed against two different baselines; applying a new baseline
    /// replaces the desk.
    #[must_use]
    pub fn new(settings: DecisionSettings) -> Self {
        Self {
            approvals: InMemoryApprovalWorkflow::with_settings(settings),
            ..Self::default()
        }
    }
}

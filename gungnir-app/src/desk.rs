// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The desktop's side of `gungnir-approval` (GAP-131, D-57,
//! `docs/design/DN-31-node-approval-queue.md` §3 and §4).
//!
//! The decision path itself is the library's: the policy chain, the queue's feeding and
//! sweep, deciding with engagement opening, the one handoff builder and the delivery
//! schedule. What is left here is the two things only this binary can say -- what the
//! picture is right now (`gungnir_approval::ApprovalContext`) and where an effect goes
//! (`gungnir_approval::ApprovalHost`) -- and this module builds both, so
//! `decisions.rs`, `engagements.rs`, `handoffs.rs` and `deliveries.rs` are thin callers
//! rather than four places that each assemble the same inputs slightly differently.
//!
//! **Why the borrows are split the way they are.** The desk is a field of [`AppState`] and
//! its effects go to other fields of the same state, so a call needs the desk mutably and
//! the bus, the alert list and the audit log mutably at the same time as the picture and
//! the baseline immutably. Destructuring the state once is what makes that legal, and
//! doing it here once is what keeps every caller honest about reading the clock, the role
//! and the session exactly once per call -- an expiry between two reads is how a record
//! comes to name an operator with no role.

use crate::state::AppState;
use gungnir_approval::{
    ApprovalContext, ApprovalDesk, ApprovalHost, DeliveryAnswer, HandoffInFlight, HandoffRecord,
    HandoffTransport, PolicyInputs, SignedIn,
};
use gungnir_eventing::{Event, EventBus};
use gungnir_model::{ExchangeItem, MissionTime};
use gungnir_remote::endpoint::{DeliveryOutcome, EndpointClient, PendingDelivery};
use gungnir_remote::link::{ExchangeProductRecord, NodeLink};
use gungnir_security::{AuditLog, FileAuditLog, OperatorId};

/// Where this desktop's approval effects go.
///
/// Field borrows of [`AppState`], never a copy: an alert raised through this has to be the
/// alert the strip draws this frame, and an audit entry has to reach the log PN-14 shows.
pub(crate) struct DesktopHost<'a> {
    events: &'a mut Box<dyn EventBus>,
    alerts: &'a mut Vec<String>,
    audit: &'a mut FileAuditLog,
    /// Who the audit log attributes an entry to, and `None` with nobody signed in
    /// (DN-23 §5 rule 1). Read with the rest of the session, once per call.
    operator: Option<OperatorId>,
    now: MissionTime,
    endpoints: &'a [gungnir_config::EndpointConfig],
    endpoint_client: Option<&'a EndpointClient>,
    /// The node link, while one is up: where the exchange set is republished to.
    link: Option<&'a NodeLink>,
}

impl ApprovalHost for DesktopHost<'_> {
    fn publish(&mut self, at: MissionTime, event: Event) {
        crate::update::publish_on(self.events.as_mut(), at, event);
    }

    fn alert(&mut self, message: String) {
        self.alerts.push(message);
    }

    fn audit(&mut self, action: &str, detail: String) {
        self.audit
            .record(crate::audit::entry(self.operator, self.now, action, detail));
    }

    /// Republish this desktop's whole current handoff set to its node for coalition
    /// exchange (GAP-065, DN-18 §5 amendment 2), if a node is linked. A no-op otherwise:
    /// with no link there is nowhere to queue to, the same reason
    /// `LinkControlAdapter::issue` refuses a sensor task at the door rather than holding
    /// it for a link that may never come.
    ///
    /// **Handoffs only, for `Warning`.** `gungnir_workflow::warning::Warning` carries no
    /// releasability field -- DN-17 §3's marked-types list does not name it, unlike
    /// `Handoff` -- so wiring it is its own change, not a silent gap folded into this one:
    /// republishing a set that does not exist yet would be the "producer... faked to make
    /// the path look busier than it is" `gungnir-model/src/exchange.rs` already refuses to
    /// be. **`gungnir_reporting::MissionReport` is no longer in that position**: it gained
    /// its own producer 2026-09-08 (`sustainment.rs::publish_to_exchange`, GAP-065) with
    /// no `Vec` of its own added to republish -- `ReportState` already holds at most the
    /// one report PN-13 last generated, and that single value already is this desktop's
    /// whole current set for `ExchangeItem::Reports`.
    fn republish_handoffs(&mut self, handoffs: &[HandoffRecord]) {
        let Some(link) = self.link else {
            return;
        };
        let products = handoffs
            .iter()
            .map(|record| ExchangeProductRecord {
                // The whole identifier: a partner asks about a product by it (D-61).
                id: record.handoff.decision.to_string(),
                at: record.handoff.issued,
                releasability: record.handoff.releasability.clone(),
                body: serde_json::to_value(&record.handoff).unwrap_or(serde_json::Value::Null),
            })
            .collect();
        link.queue_exchange(ExchangeItem::Handoffs, products);
    }
}

impl HandoffTransport for DesktopHost<'_> {
    fn address_for(&self, endpoint: &str) -> Result<String, String> {
        crate::deliveries::http_address_in(self.endpoints, self.endpoint_client.is_some(), endpoint)
    }

    fn post(&self, address: &str, payload: serde_json::Value) -> Option<Box<dyn HandoffInFlight>> {
        self.endpoint_client
            .map(|client| Box::new(Posted(client.post_json(address, payload))) as Box<_>)
    }
}

/// One post in flight on `gungnir-remote`'s endpoint client.
///
/// The wrapper exists because the library may not name the client (D-57 refused
/// `gungnir-approval` → `gungnir-remote`, DN-31 §4): three outcomes go in, the same three
/// come out, and nothing about the retry rule is decided on this side.
#[derive(Debug)]
struct Posted(PendingDelivery);

impl HandoffInFlight for Posted {
    fn poll(&self) -> Option<DeliveryAnswer> {
        self.0.poll().map(|outcome| match outcome {
            DeliveryOutcome::Accepted { status } => DeliveryAnswer::Accepted { status },
            DeliveryOutcome::Refused { status, body } => DeliveryAnswer::Refused { status, body },
            DeliveryOutcome::Unreachable { reason } => DeliveryAnswer::Unreachable { reason },
        })
    }
}

/// The desk, the picture it judges against and the host its effects go through.
///
/// The clock, the role and the session are read before the state is split, each once: a
/// decision and the engagement it opens are one act and must carry one mission time, and
/// the operator and the role of a decision come from one session state (DN-23 §5 rule 1).
pub(crate) fn with_desk<T>(
    state: &mut AppState,
    f: impl FnOnce(&mut ApprovalDesk, &ApprovalContext<'_>, &mut DesktopHost<'_>) -> T,
) -> T {
    let now = state.clock.now();
    let role = state.role();
    let signed_in = signed_in(state);
    let operator = state.attributed_operator();
    let supersedes = crate::status::baseline_validity(state).supersedes_plans();
    let delegations = crate::failover::delegations(state);
    let AppState {
        desk,
        tracking,
        resources,
        config,
        alerts,
        events,
        audit,
        endpoint_client,
        link,
        ..
    } = state;
    let config: &gungnir_config::ConfigBaseline = config;
    let cx = ApprovalContext {
        now,
        role,
        signed_in,
        tracks: tracking.tracks(),
        resources,
        config,
        baseline_supersedes_plans: supersedes,
        delegations,
    };
    let mut host = DesktopHost {
        events,
        alerts,
        audit,
        operator,
        now,
        endpoints: &config.endpoints,
        endpoint_client: endpoint_client.as_ref(),
        link: link.as_ref(),
    };
    f(desk, &cx, &mut host)
}

/// The same, with what the policy chain additionally reads.
///
/// Split from [`with_desk`] because the chain runs when a plan is proposed and the sweeps
/// run on every tick: building the geofence service and placing every friendly track on a
/// frame that expires nothing and posts nothing would be per-frame work for an answer
/// nobody asked for.
pub(crate) fn with_policy<T>(
    state: &mut AppState,
    f: impl FnOnce(
        &mut ApprovalDesk,
        &ApprovalContext<'_>,
        &PolicyInputs<'_>,
        &mut DesktopHost<'_>,
    ) -> T,
) -> T {
    // GAP-088: the fences the baseline declares, not an empty service.
    let geo = crate::geofences::service_from_config(&state.config);
    let friendly = gungnir_approval::friendly_positions(
        state.tracking.tracks(),
        crate::sustainment::local_frame(state).as_ref(),
    );
    with_desk(state, |desk, cx, host| {
        let policy = PolicyInputs {
            geofences: &geo,
            friendly_positions: friendly.as_deref(),
        };
        f(desk, cx, &policy, host)
    })
}

/// The picture and the policy inputs, without the desk, for the panels that judge a plan
/// nobody has submitted (the alternatives and the what-if, GAP-032).
///
/// Takes `&AppState` because those answers commit to nothing: they are a shared borrow of
/// the live picture through the same chain the recommendation was held to, which is the
/// property that makes an alternative comparable with it.
pub(crate) fn with_context<T>(
    state: &AppState,
    f: impl FnOnce(&ApprovalContext<'_>, &PolicyInputs<'_>) -> T,
) -> T {
    let geo = crate::geofences::service_from_config(&state.config);
    let friendly = gungnir_approval::friendly_positions(
        state.tracking.tracks(),
        crate::sustainment::local_frame(state).as_ref(),
    );
    let cx = ApprovalContext {
        now: state.clock.now(),
        role: state.role(),
        signed_in: signed_in(state),
        tracks: state.tracking.tracks(),
        resources: &state.resources,
        config: &state.config,
        baseline_supersedes_plans: crate::status::baseline_validity(state).supersedes_plans(),
        // The alternatives and the what-if are judged by the chain the plan in force was
        // held to, and that chain reads the delegations as they stand now (GAP-134).
        delegations: crate::failover::delegations(state),
    };
    let policy = PolicyInputs {
        geofences: &geo,
        friendly_positions: friendly.as_deref(),
    };
    f(&cx, &policy)
}

/// The verified session a decision is attributed to, read once (GAP-057, DN-23 §5 rule 1).
///
/// The operator **and** the role from one session state: reading them through two calls
/// would read the clock twice, and a session that expired between the reads would record
/// an operator with no role or a role with no operator.
fn signed_in(state: &AppState) -> Option<SignedIn> {
    state.signed_in().map(|session| SignedIn {
        operator: session.operator.0.to_string(),
        role: format!("{:?}", session.role),
    })
}

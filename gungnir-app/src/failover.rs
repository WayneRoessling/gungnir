// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Mid-session failover (GAP-050, `ARCHITECTURE.md` §8.4, D-23, D-03, D-15).
//!
//! A desktop linked to a node judges the link's silence against the node's own
//! heartbeat. Past the timeout it **falls back to embedded services, says so on the strip
//! and the record, and keeps the link task retrying**; when the node answers again it
//! says that too, fetches the node's journal for the outage over `GET /v3/history`, and
//! runs `gungnir_resilience::reconcile` over the two journals. It does not switch back
//! on its own: PN-18 holds the merge and its conflicts until a person has seen them and
//! asks for the switch (D-15), and the switch is refused while the node is silent again.
//!
//! **What the report can and cannot say.** The node's history route serves its in-memory
//! window (`BACKLOG_CAPACITY` envelopes); an outage longer than the window yields "gone",
//! and PN-18 says the node's half is unavailable rather than merging what it did not
//! fetch.
//!
//! **Who resolves a conflict.** Since the owner's GAP-067 walk (2026-09-16), D-03's
//! arbitration rule does, wherever it can rank the two sides. When the reconciliation is
//! computed, every conflict `reconcile` reports goes to the rule
//! (`gungnir_model::arbitration::arbitrate`, which `gungnir_collab::RoleRankArbiter` also
//! delegates to), reading each side's recorded role as a `gungnir_security::Role`:
//!
//! - **A conflict it can rank is resolved there and then**, journaled as the rule's verdict
//!   (`LinkEvent::ConflictArbitrated`, naming the side kept, why, and both sides as read),
//!   and audited as nobody's `plan.decide`, because nobody decided.
//! - **A conflict it cannot rank honestly** -- a decision whose role was never recorded
//!   (one journaled before decisions carried a role, or taken with nobody signed in) or
//!   names no role this build knows, facing another decision -- **stays on PN-18 for a
//!   person permitted `plan.decide`**, exactly as every conflict did before, and the
//!   switch back waits for that person ([`resolve_conflict`], journaled as
//!   `ConflictResolved`). A decision facing an expiry is always rankable: the decision
//!   stands without consulting rank (DN-10 §9).
//!
//! **First and second.** The rule reads this desktop's side first and the node's second,
//! the order the merge is computed and reported in (`reconcile(local, remote)`,
//! `DecisionConflict`, `kept_local`). Only an exact tie depends on that order: two
//! decisions of equal rank at the same mission time have no earlier decision for D-03's
//! tie-break to find, the rule keeps its first side, and so **an exact tie keeps this
//! desktop's decision**, journaled under its own ground (`SameTimeOnEqualRank`) rather than
//! as the earlier of the two.

use gungnir_config::BackendConfig;
use gungnir_eventing::{Envelope, Event};
use gungnir_model::arbitration::{
    ArbitrationGround, ConflictSide, Resolution, SideOutcome, Verdict,
};
use gungnir_model::events::LinkEvent;
use gungnir_model::{DetectionView, MissionTime, PlanId, TrackView};
use gungnir_remote::link::{HistoryOutcome, NodeLink, HEARTBEAT_TIMEOUT};
use gungnir_resilience::DecisionConflict;
use gungnir_security::authz::role_permits;
use gungnir_security::{actions, AuditLog};
use gungnir_store::EventJournal;
use gungnir_tracking_service::{SubmitError, TrackingService};

use crate::state::AppState;
use crate::update::publish;

/// The fallback in force, for PN-01 and PN-18.
#[derive(Debug, Clone, PartialEq)]
pub struct Fallback {
    pub endpoint: String,
    pub since: MissionTime,
    /// How long the node had been silent when the desktop fell back.
    pub silent_s: f64,
    /// The last envelope sequence the link had applied when the desktop fell back, so
    /// the node's history is asked for from there.
    pub last_seq: u64,
    /// Set when the link answered again; the reconciliation is due from then.
    pub restored_at: Option<MissionTime>,
    /// The merge, once the node's history arrived; the reason it could not, if it
    /// could not. `None` while the fetch is in flight.
    pub reconciliation: Option<Result<Reconciliation, String>>,
}

/// What `gungnir_resilience::reconcile` found for the outage.
#[derive(Debug, Clone, PartialEq)]
pub struct Reconciliation {
    /// Envelopes this desktop journaled during the outage.
    pub local: usize,
    /// Envelopes the node journaled during the outage.
    pub remote: usize,
    pub merged: usize,
    pub duplicates_dropped: usize,
    /// Conflicts the arbitration rule could not rank, left to a person permitted
    /// `plan.decide`. The switch back is refused while any remain.
    pub conflicts: Vec<DecisionConflict>,
    /// Conflicts the arbitration rule resolved when the reconciliation was computed.
    pub arbitrated: Vec<Arbitrated>,
    /// Conflicts a person resolved, with which side was kept.
    pub resolved: Vec<(PlanId, bool)>,
}

/// A conflict the arbitration rule resolved, and how.
#[derive(Debug, Clone, PartialEq)]
pub struct Arbitrated {
    pub conflict: DecisionConflict,
    /// Whether this desktop's side was kept; `false` means the node's.
    pub kept_local: bool,
    pub ground: ArbitrationGround,
}

/// The rank the rule reads for one side: its recorded role's, when a role was recorded
/// and it names a role this build knows. Anything else is an unknown rank, never a low one.
fn rank_of(side: &ConflictSide) -> Option<u8> {
    side.role
        .as_deref()
        .and_then(crate::session::role_named)
        .map(gungnir_security::Role::rank)
}

/// The rule's verdict on one conflict, reading this desktop's side first (see the module
/// documentation for why the order matters on an exact tie, and only there), or `None`
/// when the rule cannot rank it.
fn verdict_on(conflict: &DecisionConflict) -> Option<Verdict> {
    gungnir_model::arbitration::arbitrate(
        conflict.local.facts(rank_of(&conflict.local)),
        conflict.remote.facts(rank_of(&conflict.remote)),
    )
}

/// The embedded tracker during an outage, with every accepted detection also queued on
/// the link (`ARCHITECTURE.md` §8.4): the node receives the outage's observations when it
/// answers again, in order, and the desktop's own picture is unaffected.
struct TeeTracking {
    inner: Box<dyn TrackingService>,
    link: NodeLink,
}

impl TrackingService for TeeTracking {
    fn submit_detection(&mut self, detection: DetectionView) -> Result<(), SubmitError> {
        self.link.queue_outbound(detection.clone());
        self.inner.submit_detection(detection)
    }

    fn poll(&mut self, now: MissionTime) {
        self.inner.poll(now);
    }

    fn tracks(&self) -> &[TrackView] {
        self.inner.tracks()
    }

    fn is_healthy(&self) -> bool {
        self.inner.is_healthy()
    }
}

/// The tick step.
pub fn tick(state: &mut AppState) {
    let now = state.clock.now();
    let Some(link) = state.link.clone() else {
        return;
    };
    match (&state.backend, state.fallback.as_mut()) {
        (BackendConfig::Remote { endpoint }, None) => {
            let Some(age) = link.last_heard_age() else {
                return;
            };
            if age <= HEARTBEAT_TIMEOUT {
                return;
            }
            let endpoint = endpoint.clone();
            let silent_s = age.as_secs_f64();
            let last_seq = link.last_seq();
            fall_back(state, &endpoint, silent_s, last_seq, now);
        }
        (BackendConfig::Embedded, Some(fallback))
            if fallback.restored_at.is_none() && link.connected() =>
        {
            fallback.restored_at = Some(now);
            let endpoint = fallback.endpoint.clone();
            let since = fallback.since;
            let from_seq = fallback.last_seq.saturating_add(1);
            publish(
                state,
                now,
                Event::Link(LinkEvent::Restored {
                    endpoint: endpoint.clone(),
                    fallback_since: since,
                    at: now,
                }),
            );
            state.alerts.push(format!(
                "node {endpoint} answers again; the desktop stays embedded until the \
                 reconciliation in PN-18 is seen"
            ));
            start_history_fetch(state, &endpoint, from_seq);
        }
        _ => {}
    }
    poll_history(state);
}

fn fall_back(state: &mut AppState, endpoint: &str, silent_s: f64, last_seq: u64, now: MissionTime) {
    let handle = state.runtime.handle().clone();
    let embedded = Box::new(
        gungnir_tracking_service::LiveTrackingService::new(&handle)
            .with_staleness(state.config.policy.staleness.clone()),
    );
    state.tracking = match state.link.clone() {
        Some(link) => Box::new(TeeTracking {
            inner: embedded,
            link,
        }),
        None => embedded,
    };
    state.intercept = Box::new(gungnir_intercept_service::DpInterceptService::new(
        state.config.allocation_horizon,
    ));
    state.backend = BackendConfig::Embedded;
    state.fallback = Some(Fallback {
        endpoint: endpoint.to_string(),
        since: now,
        silent_s,
        last_seq,
        restored_at: None,
        reconciliation: None,
    });
    state.pending_history = None;
    publish(
        state,
        now,
        Event::Link(LinkEvent::FellBack {
            endpoint: endpoint.to_string(),
            silent_s,
            at: now,
        }),
    );
    state.alerts.push(format!(
        "node {endpoint} silent for {silent_s:.0} s (timeout {:.0} s): running embedded; \
         decisions taken now are this desktop's and will need reconciling",
        HEARTBEAT_TIMEOUT.as_secs_f64()
    ));
}

/// Ask the node for its journal from `from_seq`, with the token the link signed in with.
fn start_history_fetch(state: &mut AppState, endpoint: &str, from_seq: u64) {
    let Some(link) = state.link.as_ref() else {
        return;
    };
    let Some(token) = link.token() else {
        set_reconciliation(
            state,
            Err(
                "the link holds no session token, so the node's journal cannot be asked for; \
                 sign in again"
                    .to_string(),
            ),
        );
        return;
    };
    match gungnir_remote::link::fetch_history(
        &gungnir_remote::RemoteEndpoint {
            url: endpoint.to_string(),
            tls: crate::session::link_tls(state),
        },
        &token,
        from_seq,
        state.runtime.handle(),
    ) {
        Ok(pending) => state.pending_history = Some(pending),
        Err(err) => set_reconciliation(state, Err(err.to_string())),
    }
}

/// Take the fetch's outcome when it lands.
fn poll_history(state: &mut AppState) {
    let Some(outcome) = state
        .pending_history
        .as_ref()
        .and_then(gungnir_remote::link::PendingHistory::poll)
    else {
        return;
    };
    state.pending_history = None;
    supply_history(state, outcome);
}

/// Fold the node's answer into the report. Public so a test can supply the answer a
/// socket would have; the live path goes through the same function.
pub fn supply_history(state: &mut AppState, outcome: HistoryOutcome) {
    let Some(fallback) = state.fallback.clone() else {
        return;
    };
    let Some(restored_at) = fallback.restored_at else {
        return;
    };
    let result = match outcome {
        HistoryOutcome::Complete(remote) => {
            let window =
                |e: &Envelope| e.mission_time >= fallback.since && e.mission_time <= restored_at;
            let remote: Vec<Envelope> = remote.into_iter().filter(|e| window(e)).collect();
            crate::update::journal_pending(state);
            let local: Vec<Envelope> = match state.session() {
                Some(session) => match state.journal.read_session(session) {
                    Ok(envelopes) => envelopes.into_iter().filter(|e| window(e)).collect(),
                    Err(err) => {
                        set_reconciliation(
                            state,
                            Err(format!("this desktop's journal could not be read: {err}")),
                        );
                        return;
                    }
                },
                None => Vec::new(),
            };
            let report = gungnir_resilience::reconcile(&local, &remote);
            // The GAP-067 walk: the rule resolves every conflict it can rank, now, and
            // leaves the rest for a person.
            let mut arbitrated = Vec::new();
            let mut conflicts = Vec::new();
            for conflict in report.conflicts {
                match verdict_on(&conflict) {
                    Some(verdict) => arbitrated.push(Arbitrated {
                        kept_local: verdict.keep == Resolution::KeepFirst,
                        ground: verdict.ground,
                        conflict,
                    }),
                    None => conflicts.push(conflict),
                }
            }
            journal_verdicts(state, &arbitrated);
            Ok(Reconciliation {
                local: local.len(),
                remote: remote.len(),
                merged: report.merged.len(),
                duplicates_dropped: report.duplicates_dropped,
                conflicts,
                arbitrated,
                resolved: Vec::new(),
            })
        }
        HistoryOutcome::Gone { reason } => Err(format!(
            "the node no longer holds its journal for the outage ({reason}); the merge cannot \
             be computed and the two records stay separate"
        )),
        HistoryOutcome::Unreachable { reason } => {
            Err(format!("the node's history could not be fetched: {reason}"))
        }
    };
    set_reconciliation(state, result);
}

/// Put each of the rule's verdicts on the record, as the rule's.
///
/// **Journaled and not audited.** The audit log is the trail of what people did under an
/// action (GAP-059), and `plan.decide` there means somebody decided; a verdict entered
/// under it would put a person's name, or a blank where a person should be, beside a
/// choice no person made. The journal is where an after-action review finds the verdict,
/// under an event of its own.
fn journal_verdicts(state: &mut AppState, arbitrated: &[Arbitrated]) {
    let now = state.clock.now();
    for a in arbitrated {
        publish(
            state,
            now,
            Event::Link(LinkEvent::ConflictArbitrated {
                plan: a.conflict.plan,
                kept_local: a.kept_local,
                ground: a.ground,
                local: a.conflict.local.clone(),
                remote: a.conflict.remote.clone(),
                at: now,
            }),
        );
    }
}

fn set_reconciliation(state: &mut AppState, result: Result<Reconciliation, String>) {
    match &result {
        Ok(r) => state.alerts.push(format!(
            "reconciliation computed: {} local and {} node envelopes, {} merged, {} \
             duplicate(s) dropped; {} conflicting decision(s) resolved by the arbitration \
             rule, {} left to a person; PN-18 holds it",
            r.local,
            r.remote,
            r.merged,
            r.duplicates_dropped,
            r.arbitrated.len(),
            r.conflicts.len()
        )),
        Err(reason) => state
            .alerts
            .push(format!("reconciliation not computed: {reason}")),
    }
    if let Some(fallback) = state.fallback.as_mut() {
        fallback.reconciliation = Some(result);
    }
}

/// Switch the desktop back to the node after the reconciliation has been seen (D-15).
///
/// # Errors
///
/// When there is no outage to end, the node has not answered, the reconciliation is
/// still being computed, or the link has gone silent again.
pub fn switch_back(state: &mut AppState) -> Result<(), String> {
    let fallback = state
        .fallback
        .clone()
        .ok_or_else(|| "no outage to switch back from".to_string())?;
    if fallback.restored_at.is_none() {
        return Err(format!("node {} has not answered yet", fallback.endpoint));
    }
    let Some(reconciliation) = fallback.reconciliation.as_ref() else {
        return Err("the reconciliation is still being computed".to_string());
    };
    // D-15: every conflict the arbitration rule could not rank is a person's decision
    // before the node's picture is taken back; switching with one open would leave two
    // decisions on the record and nobody, rule or person, having chosen between them.
    if let Ok(r) = reconciliation {
        if !r.conflicts.is_empty() {
            return Err(format!(
                "{} conflict(s) the arbitration rule could not rank are unresolved; a person \
                 permitted to decide plans resolves each before switching back",
                r.conflicts.len()
            ));
        }
    }
    let link = state
        .link
        .clone()
        .ok_or_else(|| "the link is gone; sign in again".to_string())?;
    if !link.connected() {
        return Err(format!(
            "node {} is silent again; the switch waits for it",
            fallback.endpoint
        ));
    }
    let endpoint = gungnir_remote::RemoteEndpoint {
        url: fallback.endpoint.clone(),
        tls: crate::session::link_tls(state),
    };
    state.tracking = Box::new(gungnir_remote::RemoteTrackingService::linked(
        endpoint.clone(),
        link.clone(),
    ));
    state.intercept = Box::new(gungnir_remote::RemoteInterceptService::linked(
        endpoint, link,
    ));
    state.backend = BackendConfig::Remote {
        endpoint: fallback.endpoint.clone(),
    };
    let now = state.clock.now();
    // The conflicts the merge reported, both kinds: those the rule resolved when the
    // reconciliation was computed, and those a person resolved before this switch. Each
    // of the two is on the record under its own event, so the count needs no split.
    let (merged, conflicts, node_history) = match reconciliation {
        Ok(r) => (
            r.merged,
            r.conflicts.len() + r.arbitrated.len() + r.resolved.len(),
            true,
        ),
        Err(_) => (0, 0, false),
    };
    publish(
        state,
        now,
        Event::Link(LinkEvent::SwitchedBack {
            endpoint: fallback.endpoint.clone(),
            at: now,
            merged,
            conflicts,
            node_history,
        }),
    );
    state.alerts.push(format!(
        "switched back to node {}; {} envelope(s) merged, {conflicts} conflict(s) on the record",
        fallback.endpoint, merged
    ));
    state.fallback = None;
    state.pending_history = None;
    Ok(())
}

/// A person resolves one conflict the arbitration rule could not rank (D-03, D-15; the
/// GAP-067 walk): the kept side's decision stands on this desktop's record, the act is
/// audited under `plan.decide`, and the record says who.
///
/// The role asked about is [`AppState::role`], which is the signed-in account's whenever
/// somebody is signed in, so "a person permitted `plan.decide`" means the authenticated
/// role rather than whichever one was selected.
///
/// # Errors
///
/// No reconciliation holds this plan as a conflict left to a person -- including one the
/// rule already resolved, which is not a person's to overturn here -- or the role may not
/// decide plans.
pub fn resolve_conflict(
    state: &mut AppState,
    plan: PlanId,
    keep_local: bool,
) -> Result<(), String> {
    if !role_permits(state.role(), actions::DECIDE_PLAN) {
        return Err(format!(
            "{:?} may not decide plans, and resolving a conflict is a decision",
            state.role()
        ));
    }
    let Some(Some(Ok(reconciliation))) = state.fallback.as_mut().map(|f| f.reconciliation.as_mut())
    else {
        return Err("no reconciliation holds a conflict to resolve".to_string());
    };
    let Some(at) = reconciliation.conflicts.iter().position(|c| c.plan == plan) else {
        if let Some(a) = reconciliation
            .arbitrated
            .iter()
            .find(|a| a.conflict.plan == plan)
        {
            return Err(format!(
                "plan {} was resolved by the arbitration rule ({}); it is not left to a person",
                plan.short(),
                ground_reason(a.ground)
            ));
        }
        return Err(format!(
            "plan {} is not a conflict of this outage",
            plan.short()
        ));
    };
    reconciliation.conflicts.remove(at);
    reconciliation.resolved.push((plan, keep_local));
    let now = state.clock.now();
    let operator = state.attributed_operator().map(|o| o.0.to_string());
    crate::audit::record(
        state,
        actions::DECIDE_PLAN,
        format!(
            "reconciliation: plan {} kept {}",
            plan,
            if keep_local {
                "this desktop's decision"
            } else {
                "the node's decision"
            }
        ),
    );
    publish(
        state,
        now,
        Event::Link(LinkEvent::ConflictResolved {
            plan,
            kept_local: keep_local,
            operator,
            at: now,
        }),
    );
    Ok(())
}

/// What the link has queued for the node and what it has delivered, for PN-01 and
/// PN-18 (§8.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutboxView {
    pub queued: usize,
    pub forwarded: u64,
    pub dropped: u64,
}

#[must_use]
pub fn outbox_view(state: &AppState) -> Option<OutboxView> {
    state.link.as_ref().map(|link| OutboxView {
        queued: link.outbox_len(),
        forwarded: link.forwarded(),
        dropped: link.dropped(),
    })
}

/// What PN-18 shows.
#[derive(Debug, Clone, PartialEq)]
pub enum ReconciliationView {
    /// No outage has happened this session.
    NothingDue,
    /// The desktop is running embedded after an outage that has not ended.
    OutageOngoing {
        endpoint: String,
        since: MissionTime,
    },
    /// The node is back; the outage's journals are reconciled, or the reason they could
    /// not be is here, or the fetch is still in flight.
    Due {
        endpoint: String,
        from: MissionTime,
        to: MissionTime,
        /// Decisions this desktop recorded during the outage.
        local_decisions: usize,
        reconciliation: Option<Result<Reconciliation, String>>,
        /// Whether the switch back can be asked for now.
        can_switch_back: bool,
    },
}

#[must_use]
pub fn reconciliation_view(state: &AppState) -> ReconciliationView {
    match &state.fallback {
        None => ReconciliationView::NothingDue,
        Some(f) => match f.restored_at {
            None => ReconciliationView::OutageOngoing {
                endpoint: f.endpoint.clone(),
                since: f.since,
            },
            Some(to) => ReconciliationView::Due {
                endpoint: f.endpoint.clone(),
                from: f.since,
                to,
                local_decisions: state
                    .audit
                    .entries()
                    .iter()
                    .filter(|e| {
                        e.action == gungnir_security::actions::DECIDE_PLAN
                            && e.mission_time >= f.since.0
                            && e.mission_time <= to.0
                    })
                    .count(),
                reconciliation: f.reconciliation.clone(),
                // Every conflict the rule could not rank resolved by a person (D-15), or
                // the node's half was unavailable and the report says so; and the node
                // answers.
                can_switch_back: f
                    .reconciliation
                    .as_ref()
                    .is_some_and(|r| r.as_ref().map_or(true, |r| r.conflicts.is_empty()))
                    && state
                        .link
                        .as_ref()
                        .is_some_and(gungnir_remote::link::NodeLink::connected),
            },
        },
    }
}

/// One side of a conflict in PN-18's words: what it recorded, by whom, under which role,
/// and when. A missing operator or role is said, not left blank.
#[must_use]
pub fn side_sentence(side: &ConflictSide) -> String {
    let at = side.at.0;
    let verb = match side.outcome {
        SideOutcome::Expired => return format!("expired at T+{at:.0} s with nobody deciding"),
        SideOutcome::Accepted => "accepted",
        SideOutcome::Rejected => "rejected",
    };
    match (side.operator.as_deref(), side.role.as_deref()) {
        (Some(operator), Some(role)) => {
            format!("{verb} by operator {operator} as {role} at T+{at:.0} s")
        }
        (Some(operator), None) => {
            format!("{verb} by operator {operator}, no role recorded, at T+{at:.0} s")
        }
        (None, Some(role)) => format!("{verb} with nobody signed in, as {role}, at T+{at:.0} s"),
        (None, None) => {
            format!("{verb} with nobody signed in and no role recorded, at T+{at:.0} s")
        }
    }
}

/// Why the arbitration rule kept the side it kept, in PN-18's words.
#[must_use]
pub fn ground_reason(ground: ArbitrationGround) -> &'static str {
    match ground {
        ArbitrationGround::DecisionOverExpiry => {
            "a decision stands over an expiry, whatever rank either side carried (DN-10 §9)"
        }
        ArbitrationGround::HigherRole => "the higher role wins (D-03)",
        ArbitrationGround::EarlierOnEqualRank => "equal rank, and the earlier decision wins (D-03)",
        ArbitrationGround::SameTimeOnEqualRank => {
            "equal rank at the same moment, so neither was earlier; the rule keeps this \
             desktop's, which it reads first"
        }
    }
}

/// Why the arbitration rule could not rank a conflict, in PN-18's words: which side's
/// role is missing or is no role this build knows.
#[must_use]
pub fn unranked_reason(conflict: &DecisionConflict) -> String {
    let unknown = |side: &ConflictSide, whose: &str| match side.role.as_deref() {
        _ if side.outcome == SideOutcome::Expired => None,
        None => Some(format!("{whose} decision has no recorded role")),
        Some(name) if crate::session::role_named(name).is_none() => Some(format!(
            "{whose} decision records {name:?}, which is not a role this build knows"
        )),
        Some(_) => None,
    };
    let reasons: Vec<String> = [
        unknown(&conflict.local, "this desktop's"),
        unknown(&conflict.remote, "the node's"),
    ]
    .into_iter()
    .flatten()
    .collect();
    if reasons.is_empty() {
        // Both ranks are known and still nothing was decided: only two mission times that
        // cannot be ordered get here.
        "the rule could not order the two decisions in time".to_string()
    } else {
        reasons.join(", and ")
    }
}

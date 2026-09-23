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
//!
//! # While cut off: D-15's delegations lapse (GAP-134)
//!
//! Delegations in force when the node went silent stay in force for
//! `policy.delegation.disconnected_lapse_s`, then lapse ([`delegations`]; DN-31 §6.7). A
//! lapse is applied to the queue once, at the tick it falls due: every item the delegation
//! was the only grant for is re-offered to the lowest role that still holds it, and the
//! record gets one `LinkEvent::DelegationsLapsed` naming them. From then on the chain asks
//! every authority question of the matrix without the delegated rules, so a plan only a
//! delegation could take is denied and never queued. **No new delegation is made while cut
//! off** by construction: a baseline applied while the desktop runs is in force on restart
//! (`sustainment.rs`), and the fallback does not survive a restart.
//!
//! # On reconnect: the merge, then the forwarding, then the switch (GAP-134)
//!
//! Four things happen around the moment the node answers again, and in this order:
//!
//! 1. **The merge**, when the node's history for the outage arrives: the plan conflicts and,
//!    in the same pass over the same two journals, **the engagements by track** (D-58). Two
//!    engagements of one track across the outage are published as `LinkEvent::BothActed`
//!    and alerted for a person at once, before and whatever D-03's rule then decides about
//!    any plan, because choosing which record stands does not undo an effect in the world.
//!    The rule then settles every conflict it can rank.
//! 2. **The forwarding**, as soon as every conflict is settled -- by the rule when the merge
//!    is computed, or by the last person to resolve one: every decision this desktop took
//!    while cut off, in the order it took them, **in one batch**, each carrying its
//!    settlement if its plan was in conflict (`POST /v3/decisions/forwarded`). Where the
//!    node's half could not be fetched there is nothing to settle and the batch goes at
//!    once. The node takes a batch whole or not at all.
//! 3. **The switch back**, when a person asks for it, as before (D-15), and refused while a
//!    person's conflict is open or the node has refused the batch.
//!
//! **Why the forwarding waits for the merge rather than going first.** The node's record
//! has to say what stands (MT-10 step 5). A decision forwarded before its conflict is
//! settled would put two contradictory decisions on the node's record with nothing beside
//! them saying which one stands, and a desktop that died before the settlement followed
//! would leave it that way. Sent after the merge, each decision travels with what stands.
//! Nothing is lost by waiting, and something is kept: the merge is computed from a history
//! fetched before the batch is sent, so it can never count this desktop's own decisions a
//! second time as the node's; and the two queues hold different items -- a cut-off desktop
//! decides only plans its own planner proposed, and the node's items stay on the node for
//! other desktops (DN-31 §6.7) -- so no other desktop is waiting on this batch to learn
//! that an item it could decide is already decided.
//!
//! **Why a desktop that dies mid-reconnect cannot leave the node holding half an outage.**
//! The outage is one batch and the node applies a batch whole or not at all, checking every
//! record against what it holds before writing any. So the node holds all of what this
//! desktop decided while cut off, or none of it. A batch sent twice -- after a `504`, or a
//! connection that failed with the answer in flight -- is keyed on each decision's own
//! identifier and records nothing the second time. What a process death *can* still do is
//! leave the outage unforwarded, on this desktop's disk and not the node's, because the
//! fallback lives in memory: GAP-142.

use gungnir_config::BackendConfig;
use gungnir_eventing::{Envelope, Event};
use gungnir_model::arbitration::{
    ArbitrationGround, ConflictSide, Resolution, SideOutcome, Verdict,
};
use gungnir_model::events::LinkEvent;
use gungnir_model::{DetectionView, MissionTime, PlanId, TrackView};
use gungnir_policy::Delegations;
use gungnir_remote::link::{HistoryOutcome, NodeLink, HEARTBEAT_TIMEOUT};
use gungnir_remote::queue::{ForwardReply, ForwardedDecision, Settlement};
use gungnir_resilience::{BothActed, DecisionConflict};
use gungnir_security::authz::role_permits;
use gungnir_security::{actions, AuditLog};
use gungnir_store::EventJournal;
use gungnir_tracking_service::{PipelineStats, SubmitError, TrackingService};

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
    /// When D-15's delegations lapsed on this desktop, once they have (GAP-134). Set once
    /// per outage, by the tick that applied the lapse to the queue.
    pub lapsed_at: Option<MissionTime>,
    /// Where this outage's decisions are on their way to the node (GAP-134).
    pub forwarding: Forwarding,
}

/// Where an outage's decisions stand on their way to the node's record (GAP-134,
/// DN-31 §6.8), for PN-18.
#[derive(Debug, Clone, PartialEq)]
pub enum Forwarding {
    /// Not sent yet: the merge has not been computed, or a conflict in it waits for a
    /// person. The batch goes the moment every conflict is settled.
    Waiting,
    /// This desktop decided nothing while it was cut off. Said, rather than shown as a
    /// batch of none, because "nothing to forward" and "forwarded" are different claims.
    NothingDecided,
    /// Handed to the link, `decisions` of them, and not yet answered. The link retries it
    /// under the same identifiers until the node answers.
    Sent { decisions: usize },
    /// The node answered `202`: the whole outage is on its record.
    Accepted {
        recorded: usize,
        already_held: usize,
        settled: usize,
    },
    /// The node refused the batch, and **none of it was applied**. A person has to see
    /// why before this desktop switches back.
    Refused(String),
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
    /// Who resolved each conflict in `resolved`, as the record names them: the verified
    /// operator, or `None` with nobody signed in. Kept for the settlement the node is sent
    /// (MT-10 step 5), which says who chose as the desktop's own record does.
    pub resolved_by: std::collections::BTreeMap<PlanId, Option<String>>,
    /// Tracks engaged on both sides of the outage (D-58, GAP-134). Each is published as
    /// `LinkEvent::BothActed` and alerted when the merge is computed, and PN-18 shows them
    /// above everything else. **Never resolved by anyone**: both engagements happened.
    pub both_acted: Vec<BothActed>,
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

/// D-15's delegations as they stand on this desktop now (DN-31 §6.7; GAP-134).
///
/// In force as configured until this desktop has been cut off from its node for
/// `policy.delegation.disconnected_lapse_s`, lapsed from then until it switches back. A
/// baseline that states no interval is silence about how long an offline delegation lasts,
/// and silence about authority denies (DN-08 §5): nothing survives the disconnection.
///
/// **The interval runs from the moment the node went silent, and a node that answers again
/// does not stop it.** Until a person switches back this desktop still decides on its own
/// queue, and a reconnection is not a Supervisor delegating anything afresh.
///
/// A desktop deployed on its own, and a linked one that has not lost its node, never has a
/// fallback, so for both this is the baseline's own matrix and nothing about them changes
/// (DN-31 §9 row 10).
#[must_use]
pub fn delegations(state: &AppState) -> Delegations {
    let Some(fallback) = &state.fallback else {
        return Delegations::AsConfigured;
    };
    match state.config.policy.delegation.disconnected_lapse_s {
        Some(lapse_s) if state.clock.now().seconds_since(fallback.since) < lapse_s => {
            Delegations::AsConfigured
        }
        _ => Delegations::Lapsed,
    }
}

/// Apply the lapse to the queue at the tick it falls due, once per outage (GAP-134).
///
/// Nothing happens for a baseline that delegates nothing: with no delegated rule the
/// lapsed matrix is the configured one, and an event saying delegations lapsed would be a
/// record of something that never stood.
fn lapse_if_due(state: &mut AppState, now: MissionTime) {
    let Some(fallback) = state.fallback.as_ref() else {
        return;
    };
    if fallback.lapsed_at.is_some() || delegations(state) != Delegations::Lapsed {
        return;
    }
    let endpoint = fallback.endpoint.clone();
    let cut_off_since = fallback.since;
    if let Some(f) = state.fallback.as_mut() {
        f.lapsed_at = Some(now);
    }
    if !state
        .config
        .policy
        .authority
        .rules
        .iter()
        .any(|r| r.pre_delegated)
    {
        return;
    }
    let moved = crate::desk::with_desk(state, |desk, cx, host| desk.lapse_delegations(cx, host));
    let lapse_s = state.config.policy.delegation.disconnected_lapse_s;
    publish(
        state,
        now,
        Event::Link(LinkEvent::DelegationsLapsed {
            endpoint: endpoint.clone(),
            cut_off_since,
            lapse_s,
            withdrawn: moved.iter().map(|m| m.plan).collect(),
            at: now,
        }),
    );
    state.alerts.push(match lapse_s {
        Some(s) => format!(
            "node {endpoint} has been silent for {s:.0} s: the delegations in force when it \
             went silent have lapsed (D-15); {} queued item(s) are no longer the delegated \
             role's to decide",
            moved.len()
        ),
        None => format!(
            "node {endpoint} is silent and this deployment states no interval for an \
             offline delegation, so none survives the disconnection (D-15); {} queued \
             item(s) are no longer the delegated role's to decide",
            moved.len()
        ),
    });
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

    // The three below are defaulted on the trait for a backend that has no pipeline
    // behind it, and this one has: `inner`'s. Taking the defaults instead -- which this
    // wrapper did until the rehearsal work of 2026-09-17 -- emptied PN-02's bearing rays
    // and zeroed PN-09's counters for the length of every outage, while the embedded
    // pipeline behind them went on producing both. A wrapper reporting less than what it
    // wraps is the health-flag rule read backwards, and just as wrong.
    fn bearing_rays(&self) -> &[gungnir_model::BearingRayView] {
        self.inner.bearing_rays()
    }

    fn pipeline_stats(&self) -> PipelineStats {
        self.inner.pipeline_stats()
    }

    fn is_healthy(&self) -> bool {
        self.inner.is_healthy()
    }

    fn finish(&mut self) {
        self.inner.finish();
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
            // Silent past the timeout, judged against the node's own heartbeat (D-23). A
            // link that has never been heard is not silent: it has not started. Neither
            // case returns early, because the tail below -- the lapse, the history and the
            // node's answers to a forwarded outage -- is owed on every tick (GAP-134).
            if let Some(age) = link.last_heard_age().filter(|age| *age > HEARTBEAT_TIMEOUT) {
                let endpoint = endpoint.clone();
                let silent_s = age.as_secs_f64();
                let last_seq = link.last_seq();
                fall_back(state, &endpoint, silent_s, last_seq, now);
            }
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
    // D-15's lapse, applied in the tick it falls due; a baseline that states no interval
    // falls due in the tick this desktop fell back (GAP-134).
    lapse_if_due(state, now);
    poll_history(state);
    take_forward_replies(state, &link);
}

/// The node's answers to this desktop's forwarded outages (GAP-134).
///
/// Read on every tick, fallback or not: the switch back does not wait for the answer --
/// the link retries a batch under the same identifiers until the node gives one, so a
/// transient must not strand a desktop on its embedded services -- and so an answer can
/// arrive after the switch. It is said either way; while the fallback still stands it is
/// also PN-18's, and a refusal blocks the switch.
fn take_forward_replies(state: &mut AppState, link: &NodeLink) {
    for reply in link.take_forward_replies() {
        let (forwarding, alert) = match reply {
            ForwardReply::Accepted(a) => (
                Forwarding::Accepted {
                    recorded: a.recorded,
                    already_held: a.already_held,
                    settled: a.settled,
                },
                format!(
                    "the node holds this desktop's outage: {} decision(s) recorded, {} it \
                     already held, {} settlement(s) put on its record",
                    a.recorded, a.already_held, a.settled
                ),
            ),
            ForwardReply::Refused(refused) => {
                let why = format!("{refused:?}");
                (
                    Forwarding::Refused(why.clone()),
                    format!(
                        "the node refused this desktop's outage and applied none of it: \
                         {why}. The two records disagree about a decision and a person \
                         has to see why"
                    ),
                )
            }
            ForwardReply::Rejected { status, reason } => {
                let why = format!("{status}: {reason}");
                (
                    Forwarding::Refused(why.clone()),
                    format!(
                        "the node would not take this desktop's outage ({why}); nothing of \
                         it is on the node's record"
                    ),
                )
            }
        };
        state.alerts.push(alert);
        if let Some(fallback) = state.fallback.as_mut() {
            fallback.forwarding = forwarding;
        }
    }
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
        lapsed_at: None,
        forwarding: Forwarding::Waiting,
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
            // D-58, first and whatever follows: both sides engaged one track, and no
            // verdict below changes that. On the record and in front of a person before
            // the rule is consulted about any plan (GAP-134).
            journal_both_acted(state, &report.both_acted);
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
                resolved_by: std::collections::BTreeMap::new(),
                both_acted: report.both_acted,
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
    // The batch goes as soon as nothing in the merge waits for a person, which may be
    // now: the rule settled every conflict, there were none, or the node's half could not
    // be fetched and there is nothing to settle (GAP-134).
    forward_if_settled(state);
}

/// Put each both-acted incident on the record and in front of a person (D-58, GAP-134).
///
/// **An alert per incident, never folded into the merge's summary line**: an effect in the
/// world on a track somebody else also engaged is the one thing on PN-18 a person must not
/// have to count their way to.
fn journal_both_acted(state: &mut AppState, incidents: &[BothActed]) {
    let now = state.clock.now();
    for incident in incidents {
        publish(
            state,
            now,
            Event::Link(LinkEvent::BothActed {
                track: incident.track,
                local: incident.local,
                remote: incident.remote,
                at: now,
            }),
        );
        state.alerts.push(both_acted_sentence(incident));
    }
}

/// One both-acted incident in PN-18's words and the alert's (D-58).
#[must_use]
pub fn both_acted_sentence(incident: &BothActed) -> String {
    format!(
        "BOTH MAY HAVE ACTED on track {}: this desktop engaged it at T+{:.0} s (plan {}) \
         while it was cut off, and the node engaged it at T+{:.0} s (plan {}). Choosing \
         which record stands does not undo either; a person has to establish what happened \
         to the track",
        incident.track.0,
        incident.local.at.0,
        incident.local.plan.short(),
        incident.remote.at.0,
        incident.remote.plan.short(),
    )
}

/// Send the outage's decisions to the node once nothing in the merge waits for a person
/// (GAP-134, DN-31 §6.8).
///
/// Once per outage: a batch already sent, answered or refused is not sent again from
/// here, and the link retries one that is unanswered under the same identifiers.
fn forward_if_settled(state: &mut AppState) {
    let Some(fallback) = state.fallback.as_ref() else {
        return;
    };
    if fallback.forwarding != Forwarding::Waiting {
        return;
    }
    let reconciliation = match &fallback.reconciliation {
        None => return,
        Some(Ok(r)) if !r.conflicts.is_empty() => return,
        Some(Ok(r)) => Some(r),
        Some(Err(_)) => None,
    };
    let batch = outage_batch(state, fallback, reconciliation);
    let forwarding = if batch.is_empty() {
        Forwarding::NothingDecided
    } else {
        let decisions = batch.len();
        let settled = batch.iter().filter(|f| f.settled.is_some()).count();
        match state.link.as_ref() {
            Some(link) => {
                link.queue_forward(batch);
                state.alerts.push(format!(
                    "forwarding this desktop's outage to the node: {decisions} decision(s), \
                     {settled} with the settlement of a conflict"
                ));
                Forwarding::Sent { decisions }
            }
            // The link is what carries it; without one there is nowhere to send and no
            // switch back to wait for either. Said rather than dropped.
            None => Forwarding::Refused(
                "the link to the node is gone, so the outage could not be forwarded; sign \
                 in again"
                    .to_string(),
            ),
        }
    };
    if let Some(f) = state.fallback.as_mut() {
        f.forwarding = forwarding;
    }
}

/// Every decision this desktop took while it was cut off, in the order it took them, as
/// the node's route takes them (DN-31 §5.2, §6.8).
///
/// **Read from the desk's own record**, which holds each decision whole -- the plan with
/// its assignments, the verdict, the choice and its reason, who and as which role, and
/// when. The journal's `Decided` carries no plan body and could only have been joined back
/// to one.
///
/// A decision, not an expiry: an expiry is a window that closed with nobody deciding, and
/// MOE-11 counts the decisions people took. The expiries of this desktop's own queue stay
/// on its own record, which the merge already reads.
fn outage_batch(
    state: &AppState,
    fallback: &Fallback,
    reconciliation: Option<&Reconciliation>,
) -> Vec<ForwardedDecision> {
    use gungnir_command::ApprovalWorkflow;
    let Some(restored) = fallback.restored_at else {
        return Vec::new();
    };
    let origin = crate::session::origin_of(state);
    state
        .desk
        .approvals
        .records()
        .iter()
        .filter(|r| {
            // `origin` is set only on a record another machine forwarded, which a desktop
            // never admits; the filter says what "taken here" means rather than trusting it.
            r.origin.is_none()
                && !r.is_expiry()
                && r.mission_time >= fallback.since
                && r.mission_time <= restored
        })
        .map(|r| ForwardedDecision {
            record: record_view(r),
            // GAP-141: this desktop's own name, derived from the key its certificate
            // carries, so the node can check the batch came from the machine it verified.
            origin: origin.clone(),
            settled: reconciliation.and_then(|rec| settlement_for(rec, r.plan.id)),
        })
        .collect()
}

/// A decision record as the route carries it: this desktop's record, field for field.
fn record_view(
    record: &gungnir_command::DecisionRecord,
) -> gungnir_remote::queue::DecisionRecordView {
    use gungnir_command::OperatorDecision;
    use gungnir_remote::queue::DecisionChoice;
    gungnir_remote::queue::DecisionRecordView {
        decision: record.id,
        item: record.item,
        plan: record.plan.clone(),
        verdict: record.verdict.summary(),
        choice: match &record.decision {
            OperatorDecision::Accepted => DecisionChoice::Accept,
            OperatorDecision::Overridden => DecisionChoice::Override,
            OperatorDecision::Rejected { reason } => DecisionChoice::Reject {
                reason: reason.clone(),
            },
            // Filtered out by the caller; named so a change that let one through reaches
            // the node's `400` rather than a rejection nobody made.
            OperatorDecision::Expired { .. } => DecisionChoice::Reject {
                reason: String::new(),
            },
        },
        operator: record.operator_id.clone(),
        role: record.role.clone(),
        request: record.request.clone(),
        at: record.mission_time,
    }
}

/// What the merge settled about a plan, as the node is told it (MT-10 step 5): the rule's
/// verdict with both sides as it read them, or a person's resolution and who made it.
fn settlement_for(reconciliation: &Reconciliation, plan: PlanId) -> Option<Settlement> {
    if let Some(a) = reconciliation
        .arbitrated
        .iter()
        .find(|a| a.conflict.plan == plan)
    {
        return Some(Settlement::Rule {
            kept_local: a.kept_local,
            ground: a.ground,
            local: a.conflict.local.clone(),
            remote: a.conflict.remote.clone(),
        });
    }
    reconciliation
        .resolved
        .iter()
        .find(|(p, _)| *p == plan)
        .map(|(_, kept_local)| Settlement::Person {
            kept_local: *kept_local,
            operator: reconciliation.resolved_by.get(&plan).cloned().flatten(),
        })
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
    // GAP-134: a node that refused this outage holds none of it, and the two records
    // disagree about a decision. Taking the node's picture back now would leave that for
    // nobody. A batch still in flight does not hold the switch: the link sends it again
    // under the same identifiers until the node answers, and blocking on a transient would
    // strand this desktop on its embedded services for as long as the network hesitated.
    if let Forwarding::Refused(why) = &fallback.forwarding {
        return Err(format!(
            "the node refused this desktop's outage and holds none of it ({why}); a person \
             has to see why before switching back"
        ));
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
    if let Some(Some(Ok(r))) = state.fallback.as_mut().map(|f| f.reconciliation.as_mut()) {
        r.resolved_by.insert(plan, operator.clone());
    }
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
    // The last person's resolution is what lets the outage go (GAP-134).
    forward_if_settled(state);
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
// `Due` outgrew the other two variants when GAP-134 added the forwarding and delegation
// states and the merge gained its both-acted list. The value is built once per call for
// one panel and dropped, so the size costs a copy per frame at most; boxing the variant
// would change the shape every caller and every failover test destructures.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum ReconciliationView {
    /// No outage has happened this session.
    NothingDue,
    /// The desktop is running embedded after an outage that has not ended.
    OutageOngoing {
        endpoint: String,
        since: MissionTime,
        /// Where D-15's delegations stand (GAP-134).
        delegations: DelegationState,
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
        /// Where the outage's decisions stand on their way to the node (GAP-134).
        forwarding: Forwarding,
        /// Where D-15's delegations stand (GAP-134).
        delegations: DelegationState,
    },
}

/// Where D-15's delegations stand on a cut-off desktop, as PN-18 says it (GAP-134).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DelegationState {
    /// The baseline delegates nothing, so there is nothing to lapse.
    NoneConfigured,
    /// In force until this mission time.
    InForceUntil(MissionTime),
    /// Lapsed at this mission time.
    Lapsed(MissionTime),
}

/// The delegation state for PN-18.
fn delegation_state(state: &AppState, fallback: &Fallback) -> DelegationState {
    if !state
        .config
        .policy
        .authority
        .rules
        .iter()
        .any(|r| r.pre_delegated)
    {
        return DelegationState::NoneConfigured;
    }
    match (
        fallback.lapsed_at,
        state.config.policy.delegation.disconnected_lapse_s,
    ) {
        (Some(at), _) => DelegationState::Lapsed(at),
        (None, Some(s)) => DelegationState::InForceUntil(MissionTime(fallback.since.0 + s)),
        // No interval stated: it lapses the tick it falls back, and this is read before
        // that tick has run.
        (None, None) => DelegationState::Lapsed(fallback.since),
    }
}

#[must_use]
pub fn reconciliation_view(state: &AppState) -> ReconciliationView {
    match &state.fallback {
        None => ReconciliationView::NothingDue,
        Some(f) => match f.restored_at {
            None => ReconciliationView::OutageOngoing {
                endpoint: f.endpoint.clone(),
                since: f.since,
                delegations: delegation_state(state, f),
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
                // the node's half was unavailable and the report says so; the node has
                // not refused the outage (GAP-134); and the node answers.
                can_switch_back: f
                    .reconciliation
                    .as_ref()
                    .is_some_and(|r| r.as_ref().map_or(true, |r| r.conflicts.is_empty()))
                    && !matches!(f.forwarding, Forwarding::Refused(_))
                    && state
                        .link
                        .as_ref()
                        .is_some_and(gungnir_remote::link::NodeLink::connected),
                forwarding: f.forwarding.clone(),
                delegations: delegation_state(state, f),
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

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::{BearingRayView, SensorId};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// A tracker with a pipeline behind it, reduced to the three things a wrapper has to
    /// carry and nothing else.
    struct Inner {
        rays: Vec<BearingRayView>,
        stats: PipelineStats,
        finished: Arc<AtomicBool>,
    }

    impl TrackingService for Inner {
        fn submit_detection(&mut self, _detection: DetectionView) -> Result<(), SubmitError> {
            Ok(())
        }
        fn poll(&mut self, _now: MissionTime) {}
        fn tracks(&self) -> &[TrackView] {
            &[]
        }
        fn bearing_rays(&self) -> &[BearingRayView] {
            &self.rays
        }
        fn pipeline_stats(&self) -> PipelineStats {
            self.stats
        }
        fn is_healthy(&self) -> bool {
            true
        }
        fn finish(&mut self) {
            self.finished.store(true, Ordering::SeqCst);
        }
    }

    /// The outage tee reports what the tracker under it reports.
    ///
    /// Three of these methods are defaulted on the trait, for a backend with no pipeline
    /// behind it, and this wrapper took the defaults until 2026-09-17: for the length of
    /// every outage PN-02 drew no bearing ray and PN-09 counted zero, while the embedded
    /// pipeline behind the wrapper was producing both.
    #[test]
    fn the_outage_tee_reports_what_the_tracker_under_it_reports() {
        let finished = Arc::new(AtomicBool::new(false));
        let ray = BearingRayView {
            sensor: SensorId(4),
            origin_enu: [0.0, 0.0, 0.0],
            azimuth_rad: 0.5,
            elevation_rad: None,
            azimuth_one_sigma_rad: 0.01,
            valid_until: MissionTime(60.0),
        };
        let stats = PipelineStats {
            accepted: 9,
            epochs: 3,
            bearings_retained: 1,
            ..PipelineStats::default()
        };
        let mut tee = TeeTracking {
            inner: Box::new(Inner {
                rays: vec![ray],
                stats,
                finished: Arc::clone(&finished),
            }),
            link: NodeLink::scripted(),
        };

        assert_eq!(tee.bearing_rays(), [ray], "the retained bearing is dropped");
        assert_eq!(tee.pipeline_stats(), stats, "the counters read as zero");
        tee.finish();
        assert!(
            finished.load(Ordering::SeqCst),
            "the stream is never ended under the wrapper, so the last reorder horizon is lost"
        );
    }
}

// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A commander accepting a coverage gap, and the acceptance re-opening by itself (GAP-106,
//! `docs/design/DN-33-accepting-a-coverage-gap.md`).
//!
//! # What an acceptance is (D-118)
//!
//! The gap, a reason, and the signed-in commander who accepted it -- holding for the
//! baseline revision and the laydown it was made under, and for the gap's own shape. When
//! any of the three changes it re-opens by itself, and says why ([`reconcile`]). It never
//! hides the gap and never takes it out of the measure: PN-11, the viewport and PN-16 draw
//! it marked accepted, and [`gungnir_analytics::CoverageMeasure`] reports the accepted
//! metres beside the total rather than subtracting them.
//!
//! # On the record
//!
//! Accepting journals `PlanningEvent::GapAccepted` and writes an audit entry under
//! `coverage.accept_gap`; re-opening journals `PlanningEvent::GapAcceptanceReopened` and
//! writes one attributed to nobody, because nobody did it. The ledger is folded from the
//! journal at start ([`crate::planning_record::recover`]) and retention keeps the session
//! holding each standing acceptance.
//!
//! # Linked to a node (GAP-194, D-131, D-134)
//!
//! **The node holds acceptances for the desktops linked to it**, and judges each against
//! the coverage answer it serves. So while the link is up, [`accept`] records nothing
//! here: it sends the acceptance to the node, PN-11 lists the node's gaps with the
//! acceptances the node holds ([`node_gaps`]), and the node's answer comes back as an alert
//! ([`answers_from_node`]). **While the link is down** the desktop accepts what its signed-in
//! commander may, exactly as an unlinked one does, and queues the acceptance to forward,
//! naming who accepted it, when and on which desktop. When the node answers, an acceptance
//! it holds stands here too; one it refuses re-opens here with the node's reason, journaled
//! and audited, so no console shows an acceptance the node does not hold.
//!
//! **A forward is owed until the node answers it, across a restart** (GAP-202): the ledger
//! keeps what it owes, owes every recovered acceptance again at start on the connected
//! profile, and [`forward_owed`] keeps each in the link's outbox while a commander is
//! signed in. `already-accepted` settles a forward like a recording does.

use crate::state::AppState;
use gungnir_analytics::{
    accepted_gap, same_gap, standing, CoverageMeasure, CoverageReport, Standing,
};
use gungnir_model::events::PlanningEvent;
use gungnir_model::{
    AcceptedGap, GapAcceptance, GapAcceptanceId, LaydownId, MissionTime, ReopenedBecause, SessionId,
};
use gungnir_security::actions::ACCEPT_COVERAGE_GAP;
use gungnir_security::authz::role_permits;
use gungnir_security::AuditLog as _;

/// An acceptance that stopped holding, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct Reopened {
    pub acceptance: GapAcceptance,
    pub because: ReopenedBecause,
    pub at: MissionTime,
}

/// The acceptances this desktop holds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GapAcceptances {
    /// The acceptances that stand, oldest first, each with the session its
    /// `GapAccepted` is in.
    pub standing: Vec<(GapAcceptance, Option<SessionId>)>,
    /// The acceptances that re-opened while this desktop was running, oldest first: what
    /// PN-11 lists under "re-opened", with why.
    pub reopened: Vec<Reopened>,
    /// The highest identifier the record holds, so a new one never repeats an old one,
    /// and the session it is in.
    pub(crate) highest: u64,
    pub(crate) highest_session: Option<SessionId>,
    /// The acceptances this desktop owes its node a forward of, until the node answers
    /// (GAP-202): each made here while cut off, and at start every one recovered from the
    /// journal on the connected profile, since the outbox that held their forwards did not
    /// survive the restart. [`forward_owed`] keeps each in the link's outbox.
    owed: Vec<GapAcceptanceId>,
}

impl GapAcceptances {
    /// Owe the node a forward of every standing acceptance: what a desktop on the
    /// connected profile does with the ledger it recovered at start (GAP-202). One the
    /// node already holds is answered `already-accepted`, which settles it.
    pub fn owe_all(&mut self) {
        self.owed = self.standing.iter().map(|(a, _)| a.id).collect();
    }

    /// The acceptances still owed a forward, oldest first.
    #[must_use]
    pub fn owed(&self) -> &[GapAcceptanceId] {
        &self.owed
    }

    /// The sessions retention must keep (`crate::retention::protected`): each standing
    /// acceptance's, and the one holding the highest identifier, which the serial
    /// continues past.
    #[must_use]
    pub fn sessions_to_keep(&self) -> Vec<SessionId> {
        self.standing
            .iter()
            .filter_map(|(_, s)| *s)
            .chain(self.highest_session)
            .collect()
    }
}

/// What [`accept`] did with an acceptance.
#[derive(Debug, Clone, PartialEq)]
pub enum Accepted {
    /// Recorded here: no node is linked, or the link is down and it is queued to forward.
    Recorded(GapAcceptanceId),
    /// Sent to the linked node, which records it or refuses it (GAP-194).
    SentToNode,
}

impl Accepted {
    /// The acceptance recorded here, if it was.
    #[must_use]
    pub fn recorded(&self) -> Option<GapAcceptanceId> {
        match self {
            Accepted::Recorded(id) => Some(*id),
            Accepted::SentToNode => None,
        }
    }
}

/// Why an acceptance was not recorded. Nothing is journaled or audited for any of these.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AcceptError {
    /// DN-33 §6: an acceptance names the commander who made it, and a selected role is
    /// nobody's authority.
    #[error(
        "nobody is signed in: an acceptance names the commander who made it, so accepting a \
         gap needs a signed-in commander"
    )]
    NotSignedIn,
    #[error(
        "the {role} role may not accept a coverage gap ({ACCEPT_COVERAGE_GAP}); only the \
         commander may"
    )]
    NotPermitted { role: String },
    #[error("an acceptance is recorded with its reason; none was given")]
    NoReason,
    #[error("coverage cannot be computed ({reason}), so there is no gap to accept")]
    NotMeasured { reason: String },
    #[error(
        "the gap changed before the acceptance was recorded: {gap} is no longer on the \
         approach, so nothing was accepted"
    )]
    GapChanged { gap: String },
    #[error("{gap} is already accepted ({id})")]
    AlreadyAccepted { gap: String, id: GapAcceptanceId },
}

/// The live coverage report and the declared approaches' names, or why there is none.
fn live(state: &AppState) -> (Result<CoverageReport, &'static str>, Vec<String>) {
    let names = crate::sustainment::approach_names(state);
    let report =
        crate::sustainment::coverage_report(state).ok_or(if state.config.approaches.is_empty() {
            "no approach is declared"
        } else {
            "no local frame origin is declared"
        });
    (report, names)
}

/// The laydown the running baseline marks current.
#[must_use]
pub fn laydown_in_force(state: &AppState) -> Option<LaydownId> {
    state
        .config
        .laydowns
        .iter()
        .find(|l| l.current)
        .map(|l| l.id.clone())
}

/// Accept `gap`, as it was drawn on PN-11, for `reason` (DN-33 §8 rule 2).
///
/// The gap is looked for again in the live report before anything is recorded, so an
/// acceptance is never made of a gap that changed between the frame that drew it and the
/// click.
///
/// **Linked and the link up, it is sent to the node** and nothing is recorded here: the gap
/// was drawn from the node's answer ([`node_gaps`]) and the node judges it against that
/// (GAP-194). Linked and the link down, it is recorded here and queued to forward.
///
/// # Errors
///
/// [`AcceptError`]; nothing is recorded or sent.
pub fn accept(
    state: &mut AppState,
    gap: &AcceptedGap,
    reason: &str,
) -> Result<Accepted, AcceptError> {
    let Some(session) = state.signed_in() else {
        return Err(AcceptError::NotSignedIn);
    };
    if !role_permits(session.role, ACCEPT_COVERAGE_GAP) {
        return Err(AcceptError::NotPermitted {
            role: format!("{:?}", session.role),
        });
    }
    let reason = reason.trim();
    if reason.is_empty() {
        return Err(AcceptError::NoReason);
    }
    if let Some(link) = state.link.as_ref().filter(|l| l.connected()) {
        link.accept_gap(gungnir_remote::coverage::OutboundAcceptance {
            gap: gap.clone(),
            reason: reason.to_owned(),
            accepted_offline: None,
            local: None,
            attempts: 0,
        });
        return Ok(Accepted::SentToNode);
    }
    let id = record_here(state, gap, reason, &session)?;
    // Cut off from a node this desktop is linked to: owed a forward, sent when it answers
    // (D-131), and still owed after a restart (GAP-202).
    if state.link.is_some() {
        state.gap_acceptances.owed.push(id);
        forward_owed(state);
    }
    Ok(Accepted::Recorded(id))
}

/// Keep every acceptance owed a forward in the link's outbox (GAP-202). Called every frame
/// from `update::tick` and when one is made; costs nothing while nothing is owed.
///
/// **Only while a commander is signed in**: the route takes a forward under the signed-in
/// operator's token and refuses a role without `coverage.accept_gap`, and that refusal is
/// about who is signed in, not about the acceptance. An owed acceptance that no longer
/// stands here -- re-opened for a reason of its own -- is owed nothing and is dropped.
pub fn forward_owed(state: &mut AppState) {
    if state.gap_acceptances.owed.is_empty() {
        return;
    }
    let standing: Vec<GapAcceptanceId> = state
        .gap_acceptances
        .standing
        .iter()
        .map(|(a, _)| a.id)
        .collect();
    state
        .gap_acceptances
        .owed
        .retain(|id| standing.contains(id));
    let may_forward = state
        .signed_in()
        .is_some_and(|s| role_permits(s.role, ACCEPT_COVERAGE_GAP));
    let Some(link) = state.link.as_ref().filter(|_| may_forward) else {
        return;
    };
    let in_flight: Vec<GapAcceptanceId> = link
        .acceptances_in_flight()
        .iter()
        .filter_map(|o| o.local)
        .collect();
    let origin = crate::session::origin_of(state);
    for id in &state.gap_acceptances.owed {
        if in_flight.contains(id) {
            continue;
        }
        let Some((acceptance, _)) = state
            .gap_acceptances
            .standing
            .iter()
            .find(|(a, _)| a.id == *id)
        else {
            continue;
        };
        link.accept_gap(gungnir_remote::coverage::OutboundAcceptance {
            gap: acceptance.gap.clone(),
            reason: acceptance.reason.clone(),
            accepted_offline: Some(gungnir_remote::coverage::OfflineAcceptance {
                operator: acceptance.operator.clone(),
                role: acceptance.role.clone(),
                at: acceptance.at,
                origin: origin.clone(),
            }),
            local: Some(*id),
            attempts: 0,
        });
    }
}

/// Record `gap` on this desktop's own ledger, against its own live report.
fn record_here(
    state: &mut AppState,
    gap: &AcceptedGap,
    reason: &str,
    session: &gungnir_security::OperatorSession,
) -> Result<GapAcceptanceId, AcceptError> {
    let (report, names) = live(state);
    let report = report.map_err(|reason| AcceptError::NotMeasured {
        reason: reason.to_owned(),
    })?;
    let found = report.gaps.iter().find_map(|g| {
        let candidate = accepted_gap(g, names.get(g.approach)?, report.parameters);
        same_gap(gap, &candidate).then_some(candidate)
    });
    let Some(matched) = found else {
        return Err(AcceptError::GapChanged {
            gap: gap.describe(),
        });
    };
    if let Some((held, _)) = state
        .gap_acceptances
        .standing
        .iter()
        .find(|(a, _)| same_gap(&a.gap, &matched))
    {
        return Err(AcceptError::AlreadyAccepted {
            gap: matched.describe(),
            id: held.id,
        });
    }
    let now = state.clock.now();
    state.gap_acceptances.highest += 1;
    let acceptance = GapAcceptance {
        id: GapAcceptanceId(state.gap_acceptances.highest),
        gap: matched,
        reason: reason.to_owned(),
        operator: session.operator.0.to_string(),
        role: format!("{:?}", session.role),
        revision: state.config.revision,
        laydown: laydown_in_force(state),
        at: now,
    };
    publish(state, now, PlanningEvent::GapAccepted(acceptance.clone()));
    crate::audit::record(
        state,
        ACCEPT_COVERAGE_GAP,
        format!(
            "gap acceptance {} of {} under baseline revision {}{}: {}",
            acceptance.id,
            acceptance.gap.describe(),
            acceptance.revision,
            acceptance
                .laydown
                .as_ref()
                .map_or_else(String::new, |l| format!(", laydown {l}")),
            acceptance.reason
        ),
    );
    let id = acceptance.id;
    let session_id = state.session();
    state
        .gap_acceptances
        .standing
        .push((acceptance, session_id));
    Ok(id)
}

/// Re-open every acceptance that no longer holds (DN-33 §5, §8 rule 3). Called every
/// frame from `update::tick`; costs nothing while nothing stands.
///
/// **Not while the baseline's terrain is still loading**: a report computed flat while the
/// mask is on its way would re-open every acceptance at every start. Once the terrain has
/// loaded or failed, the check runs, and a failure is a shape change like any other.
pub fn reconcile(state: &mut AppState) {
    if state.gap_acceptances.standing.is_empty()
        || matches!(state.terrain, crate::terrain::TerrainStatus::Loading { .. })
    {
        return;
    }
    let (report, names) = live(state);
    let revision = state.config.revision;
    let laydown = laydown_in_force(state);
    let now = state.clock.now();
    let mut reopened = Vec::new();
    state.gap_acceptances.standing.retain(|(acceptance, _)| {
        match standing(
            acceptance,
            report.as_ref().map_err(|r| *r),
            &names,
            revision,
            laydown.as_ref(),
        ) {
            Standing::Holds { .. } => true,
            Standing::Reopens(because) => {
                reopened.push(Reopened {
                    acceptance: acceptance.clone(),
                    because,
                    at: now,
                });
                false
            }
        }
    });
    for r in reopened {
        publish(
            state,
            now,
            PlanningEvent::GapAcceptanceReopened {
                acceptance: r.acceptance.id,
                because: r.because.clone(),
                at: now,
            },
        );
        let sentence = format!(
            "Gap acceptance {} of {} re-opened: {}",
            r.acceptance.id,
            r.acceptance.gap.describe(),
            r.because.sentence()
        );
        // Nobody re-opened it: the entry names no operator, whoever is signed in.
        state.audit.record(crate::audit::entry(
            None,
            now,
            ACCEPT_COVERAGE_GAP,
            sentence.clone(),
        ));
        state.alerts.push(sentence);
        state.gap_acceptances.reopened.push(r);
    }
}

/// For each gap of `report`, the standing acceptance that names it, if any -- matched by
/// [`standing`] against the running revision and `laydown`, which is the laydown the
/// report is of: the one in force for PN-11's live report, a laydown option for its PN-16
/// row. An acceptance holds for the laydown it was made under, so an option nobody accepted
/// anything for shows none (DN-33 §8 rule 5).
#[must_use]
pub fn acceptances_for<'a>(
    state: &'a AppState,
    report: &CoverageReport,
    laydown: Option<&LaydownId>,
) -> Vec<Option<&'a GapAcceptance>> {
    let names = crate::sustainment::approach_names(state);
    let mut out = vec![None; report.gaps.len()];
    for (acceptance, _) in &state.gap_acceptances.standing {
        if let Standing::Holds { gap } = standing(
            acceptance,
            Ok(report),
            &names,
            state.config.revision,
            laydown,
        ) {
            if let Some(slot) = out.get_mut(gap) {
                *slot = Some(acceptance);
            }
        }
    }
    out
}

/// The coverage measure of `report`, with the accepted part beside each total
/// (DN-33 §7).
#[must_use]
pub fn measure(
    state: &AppState,
    report: &CoverageReport,
    laydown: Option<&LaydownId>,
) -> CoverageMeasure {
    let flags: Vec<bool> = acceptances_for(state, report, laydown)
        .iter()
        .map(Option::is_some)
        .collect();
    CoverageMeasure::of(report, &flags)
}

/// While the link to a node is up, the node's coverage report and, for each of its gaps,
/// the acceptance the node holds for it (GAP-194, D-134): what PN-11 lists and accepts
/// from. `None` with no link, the link down, nothing read yet, or a node that computes no
/// coverage -- PN-11 then lists this desktop's own report.
#[must_use]
pub fn node_gaps(state: &AppState) -> Option<(CoverageReport, Vec<Option<GapAcceptance>>)> {
    let link = state.link.as_ref().filter(|l| l.connected())?;
    match link.coverage()? {
        gungnir_remote::coverage::CoverageResponse::Computed(computed) => {
            let mut marks = computed.acceptances;
            marks.resize(computed.report.gaps.len(), None);
            Some((computed.report, marks))
        }
        gungnir_remote::coverage::CoverageResponse::NotComputed { .. } => None,
    }
}

/// The node's answers to the acceptances this desktop sent, as alerts, and a forwarded one
/// the node refused re-opened here with the node's reason (GAP-194, D-134). Called every
/// frame from `update::tick`; costs nothing with no link.
pub fn answers_from_node(state: &mut AppState) {
    let Some(link) = state.link.as_ref() else {
        return;
    };
    let outcomes = link.take_acceptance_outcomes();
    let now = state.clock.now();
    for outcome in outcomes {
        use gungnir_remote::coverage::{AcceptanceAnswer, GapAcceptanceRefused};
        let sentence = outcome.answer.sentence();
        let refused = !matches!(outcome.answer, AcceptanceAnswer::Recorded(_));
        let Some(local) = outcome.local else {
            state.alerts.push(if refused {
                format!("Gap not accepted by the node: {sentence}")
            } else {
                format!("Gap acceptance recorded on the node: {sentence}")
            });
            continue;
        };
        match &outcome.answer {
            // GAP-202: a forward the node already holds -- sent before a restart, or by
            // another console -- is as settled as one it records now.
            AcceptanceAnswer::Recorded(_)
            | AcceptanceAnswer::Refused(GapAcceptanceRefused::AlreadyAccepted { .. }) => {
                state.gap_acceptances.owed.retain(|id| *id != local);
                state.alerts.push(format!(
                    "Gap acceptance {local}, made while cut off, forwarded: {sentence}"
                ));
                continue;
            }
            // Refused for who is signed in, not for the acceptance: still owed, sent again
            // once a commander is ([`forward_owed`]).
            AcceptanceAnswer::Rejected {
                status: 401 | 403, ..
            } => {
                state.alerts.push(format!(
                    "Gap acceptance {local}, made while cut off, is not forwarded yet: {sentence}"
                ));
                continue;
            }
            _ => state.gap_acceptances.owed.retain(|id| *id != local),
        }
        let Some(at) = state
            .gap_acceptances
            .standing
            .iter()
            .position(|(a, _)| a.id == local)
        else {
            // Already re-opened here for a reason of its own; nothing stands to re-open.
            continue;
        };
        let (acceptance, _) = state.gap_acceptances.standing.remove(at);
        let because = ReopenedBecause::RefusedByNode { reason: sentence };
        publish(
            state,
            now,
            PlanningEvent::GapAcceptanceReopened {
                acceptance: acceptance.id,
                because: because.clone(),
                at: now,
            },
        );
        let said = format!(
            "Gap acceptance {} of {} re-opened: {}",
            acceptance.id,
            acceptance.gap.describe(),
            because.sentence()
        );
        // Nobody here re-opened it: the entry names no operator.
        state.audit.record(crate::audit::entry(
            None,
            now,
            ACCEPT_COVERAGE_GAP,
            said.clone(),
        ));
        state.alerts.push(said);
        state.gap_acceptances.reopened.push(Reopened {
            acceptance,
            because,
            at: now,
        });
    }
}

fn publish(state: &mut AppState, now: MissionTime, event: PlanningEvent) {
    if let Err(err) = state
        .events
        .publish(now, gungnir_eventing::Event::Planning(event))
    {
        tracing::error!(%err, "planning event publish failed");
        state.alerts.push(format!(
            "A gap acceptance event is not on the record ({err})"
        ));
    }
}

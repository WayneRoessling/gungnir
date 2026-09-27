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
}

impl GapAcceptances {
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
/// # Errors
///
/// [`AcceptError`]; nothing is recorded.
pub fn accept(
    state: &mut AppState,
    gap: &AcceptedGap,
    reason: &str,
) -> Result<GapAcceptanceId, AcceptError> {
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

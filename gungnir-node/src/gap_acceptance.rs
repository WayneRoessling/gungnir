// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Coverage gap acceptances held by the node for the desktops linked to it (GAP-194,
//! D-131).
//!
//! # What changed, and what did not
//!
//! D-118 decided what an acceptance is: the gap, a reason, and the signed-in commander who
//! accepted it, holding for the baseline revision and the laydown it was made under and
//! for the gap's own shape, re-opening by itself when any of them changes
//! (`docs/design/DN-33-accepting-a-coverage-gap.md`). D-131 decided which machine holds it
//! in a linked deployment: **the node**, as it holds the queue (D-55). So a commander's
//! acceptance reaches every console through the coverage answer they all read, rather than
//! being drawn on the one desktop that recorded it.
//!
//! The rules are DN-33's, through the same `gungnir_analytics` functions the desktop's
//! ledger uses (`accepted_gap`, `same_gap`, `standing`), so the two cannot differ about
//! what the same gap is or when an acceptance re-opens.
//!
//! # The owner's choices of 2026-10-24
//!
//! - **What it holds for is the node's own baseline**: its `revision`, and the laydown
//!   its baseline marks `current`. The node takes no baseline from a desktop, so an
//!   acceptance is stamped with what the node runs, not with what the caller's console
//!   ran.
//! - **The gap is judged against the node's own coverage answer**, the one
//!   `GET /v3/coverage` serves, flat line of sight and all. A gap the caller drew from a
//!   different answer is refused by name rather than recorded against ground the node
//!   cannot see.
//!
//! # On the record
//!
//! Accepting journals `PlanningEvent::GapAccepted` and writes an audit entry under
//! `coverage.accept_gap` naming the operator and the machine; re-opening journals
//! `PlanningEvent::GapAcceptanceReopened` and writes an entry attributed to nobody,
//! because nobody did it. A refusal the loop makes is audited under the caller's name.
//! The ledger is folded from the journal at start ([`NodeGapAcceptances::recover`]), and
//! retention keeps every session holding a standing acceptance
//! ([`NodeGapAcceptances::sessions_to_keep`]).

use gungnir_analytics::{accepted_gap, same_gap, standing, CoverageReport, Standing};
use gungnir_api::transport::{AcceptanceAnswer, PendingAcceptance};
use gungnir_api::v3::{CoverageResponse, GapAcceptanceRefused};
use gungnir_config::ConfigBaseline;
use gungnir_eventing::{Event, EventBus, EventingError};
use gungnir_model::events::PlanningEvent;
use gungnir_model::{
    AcceptedGap, GapAcceptance, GapAcceptanceId, LaydownId, MissionTime, ReopenedBecause,
};
use gungnir_security::actions::ACCEPT_COVERAGE_GAP;
use gungnir_security::{AuditEntry, AuditLog, OperatorId, Role};
use gungnir_store::{EventJournal, SessionId};

/// What an acceptance is judged against this tick: the node's coverage answer, its
/// approaches' names, and what its baseline holds.
#[derive(Debug, Clone, Copy)]
pub struct Judged<'a> {
    /// The report, or why the node computes none.
    pub report: Result<&'a CoverageReport, &'a str>,
    /// The declared approaches' names, by index, as a gap's approach is named.
    pub names: &'a [String],
    pub revision: u32,
    pub laydown: Option<&'a LaydownId>,
}

/// The approaches' names and the laydown in force, read from the node's baseline once a
/// tick.
#[derive(Debug, Clone)]
pub struct BaselineHolds {
    pub names: Vec<String>,
    pub revision: u32,
    pub laydown: Option<LaydownId>,
}

impl BaselineHolds {
    #[must_use]
    pub fn of(config: &ConfigBaseline) -> Self {
        BaselineHolds {
            names: config.approaches.iter().map(|a| a.name.clone()).collect(),
            revision: config.revision,
            laydown: config
                .laydowns
                .iter()
                .find(|l| l.current)
                .map(|l| l.id.clone()),
        }
    }

    /// What to judge against, with `coverage` the answer the node serves.
    #[must_use]
    pub fn judged<'a>(&'a self, coverage: &'a CoverageResponse) -> Judged<'a> {
        Judged {
            report: match coverage {
                CoverageResponse::Computed(computed) => Ok(&computed.report),
                CoverageResponse::NotComputed { reason } => Err(reason.as_str()),
            },
            names: &self.names,
            revision: self.revision,
            laydown: self.laydown.as_ref(),
        }
    }
}

/// A request the loop judges: the gap, the reason, and who asked.
#[derive(Debug, Clone, PartialEq)]
pub struct AcceptanceRequest {
    pub gap: AcceptedGap,
    pub reason: String,
    /// Who asked: the commander, or for a forwarded acceptance the forwarder.
    pub operator: OperatorId,
    pub role: Role,
    /// Who accepted it at a cut-off console, when it is forwarded (D-131): the
    /// acceptance is then theirs, made when they made it.
    pub accepted_offline: Option<gungnir_api::v3::OfflineAcceptance>,
}

/// An acceptance that stopped holding, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct Reopened {
    pub acceptance: GapAcceptance,
    pub because: ReopenedBecause,
}

/// The acceptances this node holds (module documentation).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NodeGapAcceptances {
    /// The acceptances that stand, oldest first, each with the session its `GapAccepted`
    /// is in.
    standing: Vec<(GapAcceptance, Option<SessionId>)>,
    /// The highest identifier the record holds, so a new one never repeats an old one,
    /// and the session it is in.
    highest: u64,
    highest_session: Option<SessionId>,
    /// Why the fold at start read less than the whole journal, when it did.
    unreadable: Option<String>,
}

impl NodeGapAcceptances {
    /// Fold every session of `journal`, oldest first: an acceptance stands when its
    /// `GapAccepted` is followed by no `GapAcceptanceReopened`. Whether it still holds
    /// against the running baseline is the first [`Self::reconcile`]'s question.
    ///
    /// A session that cannot be read stops the fold and is said
    /// ([`Self::unreadable`]): the acceptances after it would otherwise be lost silently,
    /// and its re-openings would leave earlier acceptances standing that are over.
    #[must_use]
    pub fn recover(journal: &dyn EventJournal) -> Self {
        let mut out = Self::default();
        let mut sessions = match journal.sessions() {
            Ok(s) => s,
            Err(err) => {
                out.unreadable = Some(format!("the session list could not be read: {err}"));
                return out;
            }
        };
        sessions.sort_unstable_by_key(|s| s.0);
        for session in sessions {
            let envelopes = match journal.read_session(session) {
                Ok(envelopes) => envelopes,
                Err(err) => {
                    out.unreadable = Some(format!("session {}: {err}", session.0));
                    break;
                }
            };
            for envelope in envelopes {
                let Event::Planning(event) = envelope.event else {
                    continue;
                };
                match event {
                    PlanningEvent::GapAccepted(acceptance) => {
                        if acceptance.id.0 >= out.highest {
                            out.highest = acceptance.id.0;
                            out.highest_session = Some(session);
                        }
                        out.standing.push((acceptance, Some(session)));
                    }
                    PlanningEvent::GapAcceptanceReopened { acceptance, .. } => {
                        out.standing.retain(|(a, _)| a.id != acceptance);
                    }
                    // A rehearsal and a dead-spot what-if (GAP-249) are a desktop's; the
                    // node holds neither and has nothing to recover from them.
                    PlanningEvent::LaydownRehearsed(_) | PlanningEvent::DeadSpotWhatIf(_) => {}
                }
            }
        }
        out
    }

    /// Why the fold at start read less than the whole journal, if it did.
    #[must_use]
    pub fn unreadable(&self) -> Option<&str> {
        self.unreadable.as_deref()
    }

    /// The acceptances that stand, oldest first.
    pub fn standing(&self) -> impl Iterator<Item = &GapAcceptance> {
        self.standing.iter().map(|(a, _)| a)
    }

    /// The sessions retention must keep: each standing acceptance's, and the one holding
    /// the highest identifier, which the serial continues past.
    #[must_use]
    pub fn sessions_to_keep(&self) -> Vec<SessionId> {
        self.standing
            .iter()
            .filter_map(|(_, s)| *s)
            .chain(self.highest_session)
            .collect()
    }

    /// Judge `request` against `judged`, and record it if it holds (DN-33 §8 rule 2):
    /// the gap must be on the node's answer now, and not already accepted.
    ///
    /// # Errors
    ///
    /// [`GapAcceptanceRefused`]; nothing is recorded.
    pub fn accept(
        &mut self,
        request: AcceptanceRequest,
        judged: &Judged<'_>,
        now: MissionTime,
        session: SessionId,
    ) -> Result<GapAcceptance, GapAcceptanceRefused> {
        let report = judged
            .report
            .map_err(|reason| GapAcceptanceRefused::NotMeasured {
                reason: reason.to_owned(),
            })?;
        let matched = report
            .gaps
            .iter()
            .find_map(|g| {
                let candidate = accepted_gap(g, judged.names.get(g.approach)?, report.parameters);
                same_gap(&request.gap, &candidate).then_some(candidate)
            })
            .ok_or_else(|| GapAcceptanceRefused::GapNotOnAnswer {
                gap: request.gap.describe(),
            })?;
        if let Some((held, _)) = self
            .standing
            .iter()
            .find(|(a, _)| same_gap(&a.gap, &matched))
        {
            return Err(GapAcceptanceRefused::AlreadyAccepted {
                gap: matched.describe(),
                acceptance: held.id,
            });
        }
        self.highest += 1;
        self.highest_session = Some(session);
        let (operator, role, at) = match request.accepted_offline {
            Some(offline) => (offline.operator, offline.role, offline.at),
            None => (
                request.operator.0.to_string(),
                format!("{:?}", request.role),
                now,
            ),
        };
        let acceptance = GapAcceptance {
            id: GapAcceptanceId(self.highest),
            gap: matched,
            reason: request.reason,
            operator,
            role,
            revision: judged.revision,
            laydown: judged.laydown.cloned(),
            at,
        };
        self.standing.push((acceptance.clone(), Some(session)));
        Ok(acceptance)
    }

    /// Take out every acceptance that no longer holds against `judged` (DN-33 §5), and
    /// say why each re-opened.
    pub fn reconcile(&mut self, judged: &Judged<'_>) -> Vec<Reopened> {
        let mut reopened = Vec::new();
        self.standing.retain(|(acceptance, _)| {
            match standing(
                acceptance,
                judged.report,
                judged.names,
                judged.revision,
                judged.laydown,
            ) {
                Standing::Holds { .. } => true,
                Standing::Reopens(because) => {
                    reopened.push(Reopened {
                        acceptance: acceptance.clone(),
                        because,
                    });
                    false
                }
            }
        });
        reopened
    }

    /// For each gap of `judged`'s report, the acceptance that stands for it, for the
    /// coverage answer's marks. Empty when nothing stands or nothing was computed.
    #[must_use]
    pub fn marks(&self, judged: &Judged<'_>) -> Vec<Option<GapAcceptance>> {
        let Ok(report) = judged.report else {
            return Vec::new();
        };
        if self.standing.is_empty() {
            return Vec::new();
        }
        let mut out = vec![None; report.gaps.len()];
        for (acceptance, _) in &self.standing {
            if let Standing::Holds { gap } = standing(
                acceptance,
                Ok(report),
                judged.names,
                judged.revision,
                judged.laydown,
            ) {
                if let Some(slot) = out.get_mut(gap) {
                    *slot = Some(acceptance.clone());
                }
            }
        }
        out
    }
}

/// What one tick's acceptance work needs from the loop.
pub struct Tick<'a> {
    pub bus: &'a dyn EventBus,
    pub audit: &'a mut dyn AuditLog,
    pub now: MissionTime,
    pub session: SessionId,
}

/// Answer the acceptances the route handed over, in arrival order, then re-open what no
/// longer holds; journal and audit each (module documentation). Before the loop's journal
/// append, so what this publishes is on disk in the tick it happened.
///
/// # Errors
///
/// Only a failure to publish on the bus, which stops the loop as it does everywhere else.
pub fn tick(
    ledger: &mut NodeGapAcceptances,
    pending: Vec<PendingAcceptance>,
    judged: &Judged<'_>,
    t: &mut Tick<'_>,
) -> Result<(), EventingError> {
    for request in pending {
        let operator = request.operator;
        let party = request.party.clone();
        let reply = request.reply;
        // A forwarded acceptance's entry is the forwarder's act, and says whose
        // acceptance it carried and from where (D-131, as a forwarded decision's does).
        let forwarded = request
            .accepted_offline
            .as_ref()
            .map_or_else(String::new, |o| {
                format!(
                    "; forwarded from {}, accepted there by operator {} as {} at t = {:.1} s",
                    o.origin, o.operator, o.role, o.at.0
                )
            });
        let answer = match ledger.accept(
            AcceptanceRequest {
                gap: request.gap,
                reason: request.reason,
                operator,
                role: request.role,
                accepted_offline: request.accepted_offline,
            },
            judged,
            t.now,
            t.session,
        ) {
            Ok(acceptance) => {
                t.bus.publish(
                    t.now,
                    Event::Planning(PlanningEvent::GapAccepted(acceptance.clone())),
                )?;
                t.audit.record(
                    AuditEntry::new(
                        Some(operator),
                        ACCEPT_COVERAGE_GAP,
                        t.now.0,
                        format!(
                            "gap acceptance {} of {} under baseline revision {}{}: {}{forwarded}",
                            acceptance.id,
                            acceptance.gap.describe(),
                            acceptance.revision,
                            acceptance
                                .laydown
                                .as_ref()
                                .map_or_else(String::new, |l| format!(", laydown {l}")),
                            acceptance.reason
                        ),
                    )
                    .by_party(party),
                );
                AcceptanceAnswer::Recorded(acceptance)
            }
            Err(refused) => {
                t.audit.record(
                    AuditEntry::new(
                        Some(operator),
                        ACCEPT_COVERAGE_GAP,
                        t.now.0,
                        format!("refused: {}{forwarded}", refused.sentence()),
                    )
                    .by_party(party),
                );
                AcceptanceAnswer::Refused(refused)
            }
        };
        // A caller that stopped waiting has its answer on the record and in the coverage
        // answer; there is nobody left to tell.
        let _ = reply.send(answer);
    }
    for r in ledger.reconcile(judged) {
        t.bus.publish(
            t.now,
            Event::Planning(PlanningEvent::GapAcceptanceReopened {
                acceptance: r.acceptance.id,
                because: r.because.clone(),
                at: t.now,
            }),
        )?;
        let sentence = format!(
            "gap acceptance {} of {} re-opened: {}",
            r.acceptance.id,
            r.acceptance.gap.describe(),
            r.because.sentence()
        );
        tracing::warn!(%sentence, "a gap acceptance re-opened");
        // Nobody re-opened it: the entry names no operator.
        t.audit.record(AuditEntry::new(
            None,
            ACCEPT_COVERAGE_GAP,
            t.now.0,
            sentence,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_analytics::{CoverageGap, CoverageParameters, GapSeverity};

    fn report(gaps: &[(f64, f64)]) -> CoverageReport {
        CoverageReport {
            parameters: CoverageParameters {
                sample_spacing_m: 100.0,
                terrain_masking_applied: false,
            },
            gaps: gaps
                .iter()
                .map(|&(from_m, to_m)| CoverageGap {
                    approach: 0,
                    severity: GapSeverity::Uncovered,
                    from_m,
                    to_m,
                    samples: Vec::new(),
                })
                .collect(),
        }
    }

    fn judged<'a>(
        report: &'a CoverageReport,
        names: &'a [String],
        revision: u32,
        laydown: Option<&'a LaydownId>,
    ) -> Judged<'a> {
        Judged {
            report: Ok(report),
            names,
            revision,
            laydown,
        }
    }

    fn request(gap: AcceptedGap) -> AcceptanceRequest {
        AcceptanceRequest {
            gap,
            reason: "the hill is watched by the patrol".into(),
            operator: OperatorId(7),
            role: Role::Commander,
            accepted_offline: None,
        }
    }

    /// A forwarded acceptance is the accepting commander's, made when they made it, under
    /// the node's own revision and laydown; the forwarder is not named on it.
    #[test]
    fn a_forwarded_acceptance_is_the_accepting_commanders() {
        let names = names();
        let r = report(&[(1_000.0, 1_400.0)]);
        let j = judged(&r, &names, 4, None);
        let mut forwarded = request(accepted_gap(&r.gaps[0], "upper Vell", r.parameters));
        forwarded.operator = OperatorId(9);
        forwarded.accepted_offline = Some(gungnir_api::v3::OfflineAcceptance {
            operator: "3".into(),
            role: "Commander".into(),
            at: MissionTime(2.5),
            origin: "desktop-a".into(),
        });
        let a = NodeGapAcceptances::default()
            .accept(forwarded, &j, MissionTime(10.0), SessionId(3))
            .expect("on the answer");
        assert_eq!((a.operator.as_str(), a.role.as_str()), ("3", "Commander"));
        assert!((a.at.0 - 2.5).abs() < f64::EPSILON);
        assert_eq!(a.revision, 4, "the node's revision, not the desktop's");
    }

    fn names() -> Vec<String> {
        vec!["upper Vell".into()]
    }

    /// A gap on the node's answer is accepted, stamped with the node's revision and
    /// laydown, and marked; the same gap again is refused as already accepted.
    #[test]
    fn a_gap_on_the_nodes_answer_is_accepted_once_and_marked() {
        let names = names();
        let r = report(&[(1_000.0, 1_400.0), (3_000.0, 3_200.0)]);
        let laydown = LaydownId("north".into());
        let j = judged(&r, &names, 4, Some(&laydown));
        let gap = accepted_gap(&r.gaps[0], "upper Vell", r.parameters);
        let mut ledger = NodeGapAcceptances::default();
        let a = ledger
            .accept(request(gap.clone()), &j, MissionTime(10.0), SessionId(3))
            .expect("on the answer");
        assert_eq!(a.id, GapAcceptanceId(1));
        assert_eq!((a.revision, a.laydown.clone()), (4, Some(laydown.clone())));
        assert_eq!((a.operator.as_str(), a.role.as_str()), ("7", "Commander"));
        let marks = ledger.marks(&j);
        assert_eq!(marks.len(), 2);
        assert_eq!(marks[0].as_ref().map(|m| m.id), Some(a.id));
        assert!(marks[1].is_none(), "the other gap is not accepted");
        let again = ledger
            .accept(request(gap), &j, MissionTime(11.0), SessionId(3))
            .expect_err("already accepted");
        assert!(
            matches!(again, GapAcceptanceRefused::AlreadyAccepted { acceptance, .. } if acceptance == a.id)
        );
        assert_eq!(ledger.sessions_to_keep(), vec![SessionId(3), SessionId(3)]);
    }

    /// A gap drawn from a different answer -- a terrain-masked desktop's, or one shifted by
    /// a sample -- is refused by name, and so is one when the node computes nothing.
    #[test]
    fn a_gap_not_on_the_nodes_answer_is_refused_by_name() {
        let names = names();
        let r = report(&[(1_000.0, 1_400.0)]);
        let j = judged(&r, &names, 4, None);
        let mut masked = accepted_gap(&r.gaps[0], "upper Vell", r.parameters);
        masked.terrain_masking_applied = true;
        let mut ledger = NodeGapAcceptances::default();
        let refused = ledger
            .accept(request(masked), &j, MissionTime(10.0), SessionId(3))
            .expect_err("a different answer");
        assert!(matches!(
            refused,
            GapAcceptanceRefused::GapNotOnAnswer { .. }
        ));
        assert!(refused
            .sentence()
            .contains("not on the node's coverage answer"));
        let mut shifted = accepted_gap(&r.gaps[0], "upper Vell", r.parameters);
        shifted.to_m += 100.0;
        assert!(ledger
            .accept(
                request(shifted.clone()),
                &j,
                MissionTime(10.0),
                SessionId(3)
            )
            .is_err());
        let none = Judged {
            report: Err("no approach is declared"),
            ..j
        };
        let refused = ledger
            .accept(request(shifted), &none, MissionTime(10.0), SessionId(3))
            .expect_err("nothing computed");
        assert!(matches!(refused, GapAcceptanceRefused::NotMeasured { .. }));
        assert_eq!(ledger.standing().count(), 0, "nothing recorded");
    }

    /// DN-33 §5 on the node: a new revision, another laydown marked current, or the gap
    /// changing shape each re-opens the acceptance with its reason.
    #[test]
    fn each_change_reopens_the_acceptance_with_its_reason() {
        let names = names();
        let r = report(&[(1_000.0, 1_400.0)]);
        let north = LaydownId("north".into());
        let south = LaydownId("south".into());
        let gap = accepted_gap(&r.gaps[0], "upper Vell", r.parameters);
        let grown = report(&[(1_000.0, 1_600.0)]);
        for (later, expected) in [
            (judged(&r, &names, 5, Some(&north)), "revision"),
            (judged(&r, &names, 4, Some(&south)), "laydown"),
            (judged(&grown, &names, 4, Some(&north)), "shape"),
        ] {
            let mut ledger = NodeGapAcceptances::default();
            ledger
                .accept(
                    request(gap.clone()),
                    &judged(&r, &names, 4, Some(&north)),
                    MissionTime(10.0),
                    SessionId(3),
                )
                .expect("accepted");
            assert!(ledger
                .reconcile(&judged(&r, &names, 4, Some(&north)))
                .is_empty());
            let reopened = ledger.reconcile(&later);
            assert_eq!(reopened.len(), 1, "{expected}");
            assert!(
                matches!(
                    (&reopened[0].because, expected),
                    (
                        ReopenedBecause::RevisionChanged { from: 4, to: 5 },
                        "revision"
                    ) | (ReopenedBecause::LaydownChanged { .. }, "laydown")
                        | (ReopenedBecause::ShapeChanged { .. }, "shape")
                ),
                "{expected}: {:?}",
                reopened[0].because
            );
            assert_eq!(ledger.standing().count(), 0);
            assert!(ledger.marks(&later).is_empty());
        }
    }
}

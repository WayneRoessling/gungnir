// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The policy chain over a plan, and what it can and cannot check
//! (`docs/design/DN-31-node-approval-queue.md` §3 point 1; GAP-131, D-57).
//!
//! Moved here from `gungnir-app` with the rest of the decision path: a node running the
//! same chain from its own copy is how two machines come to give one plan two different
//! verdicts, and D-55 refused that outright. Nothing about which engines run or in what
//! order changed with the move (GAP-038).
//!
//! # What the chain is, and what it is not
//!
//! The chain is the readiness-and-geofence engine, the control status engine, the authority
//! engine and fires deconfliction. It is complete in the sense that every engine
//! `gungnir-policy` implements is in it, and incomplete in ways the operator has to know
//! about, which [`PolicyChainReport`] carries to PN-07: a verdict from a chain missing an
//! engine is not the same verdict.

use crate::{ApprovalContext, PolicyInputs};
use gungnir_command::LadderRung;
use gungnir_model::{Classification, Geodetic, LocalFrame, PlanView, TrackId, TrackView};
use gungnir_policy::{
    AuthorityPolicy, ControlStatusPolicy, FiresContext, FiresDeconflictionPolicy, GeofencePolicy,
    PolicyChain, PolicyEngine, PolicyVerdict, ReportedPositionSource,
};
use gungnir_security::authz::role_permits;
use gungnir_security::{actions, Role};

/// The authorization action a plan decision falls under.
pub const DECISION_ACTION: &str = actions::DECIDE_PLAN;

/// The engines the chain runs, in order, as PN-07 and the record name them.
///
/// Named `DESKTOP_ENGINES` until GAP-132, when the node began running the same four
/// (DN-31 §6.1) and the old name would have had a node publishing a desktop's engine
/// list. The values are unchanged, so no record reads differently.
pub const CHAIN_ENGINES: [&str; 4] = [
    "readiness and geofence",
    "control status",
    "authority",
    "fires deconfliction",
];

/// The escalation ladder: the roles that may take a plan decision, lowest authority first.
///
/// Derived from `gungnir-security` rather than configured, so it cannot drift from the
/// authorization it is supposed to follow: a role that cannot decide must never be offered
/// an item, and a role that can must not be skipped. The ordering itself is
/// [`gungnir_command::escalation_ladder`], beside the queue that walks it (DN-31 §3); the
/// roles are gathered here because `gungnir-command` cannot see them -- it depends on the
/// model and on policy, and D-57 does not give it an edge to `gungnir-security`.
#[must_use]
pub fn escalation_ladder() -> Vec<String> {
    gungnir_command::escalation_ladder(
        Role::ALL
            .iter()
            .copied()
            .filter(|r| role_permits(*r, DECISION_ACTION))
            .map(|r| LadderRung {
                role: format!("{r:?}"),
                rank: r.rank(),
            }),
    )
}

/// The roles of [`escalation_ladder`], in the same order, as roles rather than names.
///
/// Derived from the names rather than sorted here, so the *ordering* stays the one rule
/// `gungnir_command::escalation_ladder` holds beside the queue that walks it (DN-31 §3,
/// GAP-131): a second sort here is how a node and a desktop come to escalate in two
/// different orders. The round trip through the debug spelling is the same one
/// `DecisionRecord::role` and `PendingApproval::offered_to` already make.
#[must_use]
pub fn ladder_roles() -> Vec<Role> {
    escalation_ladder()
        .iter()
        .filter_map(|name| {
            Role::ALL
                .iter()
                .copied()
                .find(|r| &format!("{r:?}") == name)
        })
        .collect()
}

/// Whether this role may override rather than only accept.
#[must_use]
pub fn may_override(role: Role) -> bool {
    role_permits(role, actions::OVERRIDE_PLAN)
}

/// What the chain said about a plan, and who may take it (DN-31 §6.1).
#[derive(Debug, Clone, PartialEq)]
pub struct Offering {
    /// The chain's verdict.
    ///
    /// For the role in `offered_to` when there is one. When there is none it is the
    /// **highest** role on the ladder's verdict, which is the one that is not an artefact
    /// of who was asked: if the top of the ladder is denied for a reason other than
    /// authority, so is everybody, because the three other engines do not read the asking
    /// role at all.
    pub verdict: PolicyVerdict,
    /// The lowest role on the ladder holding authority for every solution's layer and
    /// class, which is the role the item is offered to first.
    ///
    /// `None` means the plan was denied: either nobody on the ladder may accept it --
    /// DN-09 §7's "what must go up" with nowhere left to go, which is
    /// `Denied { Authority }` -- or an engine before authority denied it for everyone.
    pub offered_to: Option<Role>,
}

/// Run the chain for every role on the escalation ladder, lowest authority first
/// (DN-31 §6.1).
///
/// **Not for one asking role.** A node has nobody signed in, so there is no asking role
/// to run the chain for: the plan an Operator may not accept is exactly the plan a
/// Supervisor should be offered. A desktop asks about the role at the console first and
/// calls this when that role may not accept (GAP-113, `ApprovalDesk::submit`), which is
/// what stops an under-authority plan being counted and queued for nobody.
///
/// The first role whose whole chain returns `RequiresHumanApproval` is the offer. That is
/// the same question as "holds authority for every solution's layer and class", because
/// the authority engine walks every assignment and denies on the first it has no rule
/// for; asking it through the chain rather than through the matrix directly is what keeps
/// the offer and the verdict from being decided by two different pieces of code.
#[must_use]
pub fn offer_to(cx: &ApprovalContext<'_>, policy: &PolicyInputs<'_>, plan: &PlanView) -> Offering {
    let mut last = None;
    for role in ladder_roles() {
        let verdict = evaluate(&cx.as_role(role), policy, plan);
        if matches!(verdict, PolicyVerdict::RequiresHumanApproval) {
            return Offering {
                verdict,
                offered_to: Some(role),
            };
        }
        last = Some(verdict);
    }
    Offering {
        // A ladder with nobody on it is a deployment in which no role may decide at all.
        // Saying so as an authority denial is the truth, and it carries no layer because
        // no layer is what failed: the ladder is empty before any plan is looked at.
        verdict: last.unwrap_or(PolicyVerdict::Denied {
            reason_code: gungnir_policy::DenialReason::InsufficientAuthority,
        }),
        offered_to: None,
    }
}

/// What the policy chain was, and which of its checks could not have failed.
///
/// The second half is the part that is easy to under-report, and it was under-reported
/// first time: the no-go check is vacuous for **two independent reasons**, and fixing
/// either one alone would leave it vacuous. Both have to be named, because an operator
/// told only about the missing fences would reasonably conclude that configuring some
/// fences makes the check real.
#[derive(Debug, Clone, PartialEq, Eq)]
// Four independent, orthogonal caveats about separately missing data sources, not a
// state machine: each names a different reason a different check cannot pass, and
// they combine freely (GAP-088, GAP-031, GAP-090 each own one).
#[allow(clippy::struct_excessive_bools)]
pub struct PolicyChainReport {
    /// Engine names in the order they were consulted.
    pub engines: Vec<&'static str>,
    /// `ConfigBaseline` has no geofence section, so the `GeoService` holds no fences and
    /// `is_within_no_go` is a search of an empty list.
    pub no_geofences_configured: bool,
    /// The planner sets `intercept_point: None` on every solution (GAP-031), so the
    /// no-go test is never reached even with fences configured.
    pub no_intercept_geometry: bool,
    /// Three of the fires checks have no data source -- no-fire areas (GAP-088), airspace
    /// measures, interceptor points (GAP-031) -- and a check without data fails (DN-05
    /// §5), so a fires task is denied until the sources exist.
    pub fires_sources_missing: bool,
    /// The friendly-position check's reported half has no source configured
    /// (DN-25 §5 rule 5; GAP-090): GAP-091's feed does not exist, so this build
    /// always passes `ReportedPositionSource::NotConfigured`, and the check
    /// fails with that reason rather than passing on an empty detected set
    /// alone.
    pub no_reported_position_source_configured: bool,
}

impl PolicyChainReport {
    /// The checks that ran and could not have failed, as PN-07 shows them.
    #[must_use]
    pub fn caveats(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.fires_sources_missing {
            out.push(
                "the fires no-fire-area, airspace-measure and interceptor-trajectory checks, \
                 which have no data source (GAP-088, GAP-031) and therefore fail: a fires \
                 task is denied until the sources exist, and PN-05 lists each check",
            );
        }
        if self.no_reported_position_source_configured {
            out.push(
                "the fires friendly-position check's reported half, because no \
                 reported-position source is configured (GAP-090, GAP-091): the check \
                 fails with that reason rather than passing on an empty detected set, \
                 and PN-05 names it",
            );
        }
        if self.no_intercept_geometry {
            out.push(
                "the no-go geofence check, because the planner computes no intercept \
                 point to test (GAP-031)",
            );
        }
        if self.no_geofences_configured {
            out.push(
                "the no-go geofence check, because the configuration baseline has no \
                 geofence section to load fences from",
            );
        }
        out
    }
}

/// What the chain consulted, for PN-07.
///
/// The engine is named "readiness and geofence" rather than "geofence" because that is
/// what it is: `GeofencePolicy::evaluate` denies an empty plan, an unknown resource and
/// an unready resource before it looks at geometry at all. Those three checks are real
/// and run against the configured resources. Only the geometry half is vacuous, and
/// calling the whole engine vacuous would understate the chain as badly as calling it
/// sound would overstate it.
#[must_use]
pub fn chain_report_for(config: &gungnir_config::ConfigBaseline) -> PolicyChainReport {
    PolicyChainReport {
        engines: CHAIN_ENGINES.to_vec(),
        // GAP-088: the real count, not a hard-coded caveat.
        no_geofences_configured: config.geofences.is_empty(),
        no_intercept_geometry: true,
        fires_sources_missing: true,
        // GAP-090/GAP-091: unconditional like `no_intercept_geometry` above --
        // there is no configuration surface for a reported-position source at
        // all yet, so there is nothing on `config` to read this from.
        no_reported_position_source_configured: true,
    }
}

/// The report for a baseline that declares nothing, which every test fixture is.
#[must_use]
pub fn chain_report() -> PolicyChainReport {
    chain_report_for(&gungnir_config::ConfigBaseline::default())
}

/// What the picture can supply to the fires checks (DN-05 §5), and nothing it cannot.
///
/// Friendly positions are the tracks carried as friendly, placed through the local frame;
/// without an origin they cannot be placed and the check is told so. No-fire areas
/// (GAP-088), airspace measures (no source) and interceptor points (GAP-031) are `None`,
/// which DN-05 §5 rule 2 turns into a failed check with the reason -- never a pass.
///
/// The reported-position half of rule 1 (DN-25 §5 rule 5; GAP-090) has no function
/// here to call: there is nothing to build. `reported_positions` is constructed
/// inline at both call sites below as `ReportedPositionSource::NotConfigured`,
/// because that is the honest state of this build -- GAP-091's feed does not exist
/// -- rather than a value a helper computes and could be mistaken for one.
///
/// The host calls this when it builds [`PolicyInputs`]: the frame is the deployment's, and
/// only the host knows whether one is declared.
#[must_use]
pub fn friendly_positions(
    tracks: &[TrackView],
    frame: Option<&LocalFrame>,
) -> Option<Vec<Geodetic>> {
    let frame = frame?;
    Some(
        tracks
            .iter()
            .filter(|t| t.classification == Classification::Friendly)
            .map(|t| frame.to_geodetic([t.state[0], t.state[1], t.state[2]]))
            .collect(),
    )
}

/// The fires checks for a plan, every one with its result (DN-05 §7 for PN-05). Empty for
/// a plan that is not a fires task.
#[must_use]
pub fn fires_checks(
    cx: &ApprovalContext<'_>,
    policy: &PolicyInputs<'_>,
    plan: &PlanView,
) -> Vec<gungnir_model::DeconflictionCheck> {
    let Some(fires) = plan.fires() else {
        return Vec::new();
    };
    let engine = FiresDeconflictionPolicy {
        settings: &cx.config.policy.fires,
        context: FiresContext {
            friendly_positions: policy.friendly_positions,
            // GAP-090/GAP-091: no reported-position feed exists in this build, so
            // this is the honest state rather than a silent stand-in for one.
            reported_positions: ReportedPositionSource::NotConfigured,
            no_fire_areas: None,
            airspace_measures: None,
            interceptor_points: None,
        },
    };
    engine.deconflict(fires).checks
}

/// Run the policy chain over a plan.
#[must_use]
pub fn evaluate(
    cx: &ApprovalContext<'_>,
    policy: &PolicyInputs<'_>,
    plan: &PlanView,
) -> PolicyVerdict {
    with_chain(cx, policy, |chain| chain.evaluate(plan, cx.resources))
}

/// Build the chain and hand it to `f`.
///
/// Continuation-passing rather than a function returning the chain, because three of the
/// four engines borrow values that have to be built first -- the role name, the friendly
/// positions and the classifier over the current snapshot -- and a chain returned by value
/// would outlive them.
///
/// **One construction site, deliberately.** [`evaluate`] and a host's own decision support
/// must judge a plan by the same four engines, or an alternative could be offered under a
/// chain the plan in force was never held to; building the chain twice is exactly how that
/// drifts. It is public for that reason: `gungnir-app`'s alternatives (GAP-032) solve
/// against this chain rather than one of their own.
pub fn with_chain<T>(
    cx: &ApprovalContext<'_>,
    policy: &PolicyInputs<'_>,
    f: impl FnOnce(&PolicyChain<'_>) -> T,
) -> T {
    let classification = classifier(cx.tracks);
    let role_name = cx.role_name();
    // D-15's delegations as they stand for this host (GAP-134): the baseline's own matrix
    // on a node and a linked desktop, and the matrix without its delegated rules on a
    // desktop cut off past the configured interval.
    let authority = cx.authority();
    let chain = PolicyChain::new(vec![
        // GAP-088: the fences the baseline declares, not an empty service.
        Box::new(GeofencePolicy {
            geo: policy.geofences,
        }),
        Box::new(ControlStatusPolicy {
            settings: &cx.config.policy.control_status,
            track_classification: &classification,
        }),
        Box::new(authority_engine(&authority, &role_name, &classification)),
        // GAP-036: fourth, with what the picture can supply (see `friendly_positions`).
        Box::new(FiresDeconflictionPolicy {
            settings: &cx.config.policy.fires,
            context: FiresContext {
                friendly_positions: policy.friendly_positions,
                // GAP-090/GAP-091: see the comment on the identical field in
                // `fires_checks` above -- no feed exists, so this is the truth.
                reported_positions: ReportedPositionSource::NotConfigured,
                no_fire_areas: None,
                airspace_measures: None,
                interceptor_points: None,
            },
        }),
    ]);
    f(&chain)
}

/// The authority engine exactly as the chain builds it: the matrix in force, the asking
/// role, the decision action and the classifier over the current picture.
///
/// One constructor for [`with_chain`] and [`holds_authority`], so the question a lapse asks
/// of a queued item and the question the chain asked when it was submitted cannot be two
/// different questions.
fn authority_engine<'a>(
    settings: &'a gungnir_model::AuthoritySettings,
    role_name: &'a str,
    classification: &'a (dyn Fn(TrackId) -> Classification + Send + Sync),
) -> AuthorityPolicy<'a> {
    AuthorityPolicy {
        settings,
        asking_role: role_name,
        action: DECISION_ACTION,
        track_classification: classification,
    }
}

/// Whether `role` holds the authority for every solution of `plan` under the matrix in
/// force for this host (D-15, DN-31 §6.7; GAP-134).
///
/// **The authority engine alone, and deliberately not the whole chain.** This is the
/// question a lapse asks of an item already in the queue, and the lapse changed exactly one
/// thing: which rules grant. The three other engines were asked when the item was
/// submitted and nothing re-asks them of a queued item on any tick; asking them here, and
/// only here, would let a lapse withdraw an item for a reason that has nothing to do with
/// the lapse -- a resource that went unready, a track reclassified -- and report it as the
/// delegation's doing.
#[must_use]
pub fn holds_authority(cx: &ApprovalContext<'_>, role: &str, plan: &PlanView) -> bool {
    let classification = classifier(cx.tracks);
    let authority = cx.authority();
    matches!(
        authority_engine(&authority, role, &classification).evaluate(plan, cx.resources),
        PolicyVerdict::RequiresHumanApproval
    )
}

/// Classification of a track, for the two engines that judge by class.
///
/// A track the snapshot no longer holds is `Unknown` rather than absent, and that is
/// the strict reading: `ControlStatusPolicy` permits `Unknown` only at Free, and
/// `AuthorityPolicy` needs a rule naming the unknown class. Defaulting to `Friendly`
/// or `Hostile` would each be a guess that changes a verdict.
fn classifier(tracks: &[TrackView]) -> impl Fn(TrackId) -> Classification + Send + Sync + '_ {
    move |id| {
        tracks
            .iter()
            .find(|t| t.id == id)
            .map_or(Classification::Unknown, |t| t.classification)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The chain says what it checked and admits what it could not.
    ///
    /// Both reasons the no-go check is vacuous have to be reported. Naming only the
    /// missing fences would tell an operator that configuring fences makes the check
    /// real, and it would not: the planner computes no intercept point to test.
    #[test]
    fn the_chain_names_its_engines_and_every_vacuous_check() {
        let report = chain_report();
        assert_eq!(report.engines.len(), 4, "{:?}", report.engines);
        assert!(report.no_geofences_configured);
        assert!(report.no_intercept_geometry);
        assert!(report.fires_sources_missing);
        assert!(report.no_reported_position_source_configured);

        let caveats = report.caveats();
        assert_eq!(
            caveats.len(),
            4,
            "every independent reason must be named, or fixing one would look like \
             fixing the check"
        );
        assert!(caveats.iter().any(|c| c.contains("GAP-031")));
        assert!(caveats.iter().any(|c| c.contains("geofence section")));
        assert!(caveats.iter().any(|c| c.contains("fires")));
        assert!(caveats.iter().any(|c| c.contains("GAP-090")));
    }

    /// Fixing either reason alone leaves the check vacuous, which is the whole point of
    /// reporting them separately.
    #[test]
    fn one_caveat_remains_when_only_one_cause_is_fixed() {
        let fences_configured = PolicyChainReport {
            engines: chain_report().engines,
            no_geofences_configured: false,
            no_intercept_geometry: true,
            fires_sources_missing: false,
            no_reported_position_source_configured: false,
        };
        assert_eq!(fences_configured.caveats().len(), 1);

        let sound = PolicyChainReport {
            engines: chain_report().engines,
            no_geofences_configured: false,
            no_intercept_geometry: false,
            fires_sources_missing: false,
            no_reported_position_source_configured: false,
        };
        assert!(
            sound.caveats().is_empty(),
            "with both causes fixed the check is real and must claim nothing"
        );
    }

    /// GAP-090's caveat is independent of the other three: it names the
    /// friendly-position check's reported half specifically, and stands alone
    /// when the other three causes are fixed.
    #[test]
    fn the_reported_position_caveat_stands_alone() {
        let only_this = PolicyChainReport {
            engines: chain_report().engines,
            no_geofences_configured: false,
            no_intercept_geometry: false,
            fires_sources_missing: false,
            no_reported_position_source_configured: true,
        };
        let caveats = only_this.caveats();
        assert_eq!(caveats.len(), 1);
        assert!(caveats[0].contains("GAP-090"));
        assert!(caveats[0].contains("GAP-091"));
    }

    /// Accepting and overriding are different authorities. An operator holds the first
    /// and not the second, and a supervisor holds both.
    #[test]
    fn overriding_is_a_higher_authority_than_accepting() {
        assert!(role_permits(Role::Operator, DECISION_ACTION));
        assert!(!may_override(Role::Operator));
        assert!(role_permits(Role::Supervisor, DECISION_ACTION));
        assert!(may_override(Role::Supervisor));
        assert!(!role_permits(Role::Analyst, DECISION_ACTION));
    }

    /// The ladder is the roles that may decide, lowest authority first, and it is
    /// derived from the authorization rather than listed beside it.
    #[test]
    fn the_ladder_is_ordered_and_holds_only_roles_that_may_decide() {
        let ladder = escalation_ladder();
        assert!(!ladder.is_empty());
        let ranks: Vec<u8> = ladder
            .iter()
            .map(|name| {
                Role::ALL
                    .iter()
                    .find(|r| format!("{r:?}") == *name)
                    .map_or_else(|| panic!("{name} is not a role"), |r| r.rank())
            })
            .collect();
        assert!(
            ranks.windows(2).all(|w| w[0] <= w[1]),
            "the ladder is not ordered by authority: {ladder:?}"
        );
        for role in Role::ALL {
            assert_eq!(
                ladder.contains(&format!("{role:?}")),
                role_permits(*role, DECISION_ACTION),
                "{role:?} is on the ladder without the authorization, or the reverse"
            );
        }
    }
}

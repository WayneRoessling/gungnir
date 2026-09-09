// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Fires deconfliction.
//!
//! Design: docs/design/DN-05-fires.md, extended by
//! docs/design/DN-25-cursor-on-target.md §5 rule 5 for the friendly-position
//! check's three states (GAP-090). Capability CAP-3.8; decision D-07 put fires
//! in the first release, so this is release content and not a domain extension.
//! Mission thread MT-06 steps 5 to 7.
//!
//! **Human-owned** (docs/agentic-workflow.md): every verdict in this crate is.
//!
//! Two rules govern this file and are what its tests check:
//!
//! 1. **Every check runs and every result is reported, including the ones that
//!    passed.** A verdict naming only the first obstacle lets the operator clear it
//!    and believe the task is clean.
//! 2. **A check that cannot be evaluated does not pass.** It fails with a detail
//!    saying the data was missing. An unevaluated safety check reported as passed is
//!    the worst possible output of this component.
//!
//! Nothing here changes the authority model. A fires plan is a recommendation, goes
//! through the same approval workflow, and needs a recorded human decision.

use crate::{DenialReason, PolicyEngine, PolicyVerdict};
use gungnir_geo::{great_circle_distance_m, Geofence};
use gungnir_model::{
    Classification, DeconflictionCheck, DeconflictionKind, DeconflictionResult, FiresPlan,
    FiresSettings, Geodetic, PlanView, ReportedPosition, ResourceView,
};

/// Where rule 1 gets a self-reported friendly position from, and whether the
/// concept even exists in this deployment (DN-25 §5 rule 5; GAP-090).
///
/// **Three states, not one.** Before this type, an empty detected-friendly set
/// could mean "the ellipse is clear" or "nothing was watching it", and those are
/// not the same fact. Distinguishing them is the entire content of GAP-090.
#[derive(Debug, Clone, Copy, Default)]
pub enum ReportedPositionSource<'a> {
    /// No reported-position source is configured in this deployment.
    ///
    /// **The honest state of every real deployment today.** GAP-091's wire
    /// adapter cannot be built yet -- it needs an actual TAK client to record a
    /// corpus from, which is the one thing this workspace cannot supply itself
    /// (`docs/design/external-standards.md` §5.6, §5.8) -- so nothing has ever
    /// constructed the other two variants outside a test. Never a pass: the
    /// concept of a self-reporting friendly not existing here is not evidence
    /// that none is present.
    #[default]
    NotConfigured,
    /// A source is configured but has not reported recently enough to trust:
    /// rule 2's existing "cannot be evaluated" failure, applied to this source
    /// rather than to the detected picture.
    Silent,
    /// A source is configured and live. An empty slice is a legitimate reading
    /// -- the source checked and found nobody -- and is reported as a pass, not
    /// folded into `NotConfigured`.
    Live(&'a [ReportedPosition]),
}

/// What the picture knows that the deconfliction rules need.
///
/// Supplied by the caller rather than fetched, so this stays a pure function of its
/// inputs and every case is testable. `None` on an `Option` field means the data is
/// unavailable, and the corresponding check fails rather than passing.
#[derive(Debug, Clone, Default)]
pub struct FiresContext<'a> {
    /// Known **detected** friendly positions: the tracks carried as friendly
    /// (GAP-036). `None` when the picture cannot supply them at all, which is a
    /// different failure from an empty list -- an empty list is a real reading,
    /// `None` is the absence of one.
    pub friendly_positions: Option<&'a [Geodetic]>,
    /// Known **self-reported** friendly positions, and whether that concept
    /// exists here yet (DN-25 §5 rule 5; GAP-090). Defaults to `NotConfigured`,
    /// which is the truth until GAP-091 delivers a feed.
    pub reported_positions: ReportedPositionSource<'a>,
    /// Areas artillery may not strike. `None` when the layer is unavailable.
    pub no_fire_areas: Option<&'a [Geofence]>,
    /// Declared airspace measures the trajectory must not violate.
    pub airspace_measures: Option<&'a [Geofence]>,
    /// Intercept points in force, which a fires trajectory must not conflict with.
    pub interceptor_points: Option<&'a [Geodetic]>,
}

/// Runs every fires deconfliction check and denies a plan that fails any of them.
pub struct FiresDeconflictionPolicy<'a> {
    pub settings: &'a FiresSettings,
    pub context: FiresContext<'a>,
}

fn check(kind: DeconflictionKind, passed: bool, detail: impl Into<String>) -> DeconflictionCheck {
    DeconflictionCheck {
        kind,
        passed,
        detail: detail.into(),
    }
}

/// The radius around the target that must be clear: its location error plus the
/// policy's separation.
fn keep_out_radius_m(plan: &FiresPlan, settings: &FiresSettings) -> f64 {
    plan.location_error_m + settings.minimum_separation_m
}

impl FiresDeconflictionPolicy<'_> {
    /// Evaluates every check and returns the full result, passes included.
    pub fn deconflict(&self, plan: &FiresPlan) -> DeconflictionResult {
        let radius = keep_out_radius_m(plan, self.settings);
        let target = plan.target_position;
        let mut checks = Vec::with_capacity(5);

        // 1. Location accuracy. Evaluated first because it sizes every other check.
        let accurate = plan.location_error_m.is_finite()
            && plan.location_error_m <= self.settings.max_location_error_m;
        checks.push(check(
            DeconflictionKind::LocationAccuracy,
            accurate,
            if accurate {
                format!(
                    "location error {:.0} m is within the limit",
                    plan.location_error_m
                )
            } else {
                format!(
                    "location error {:.0} m exceeds the {:.0} m limit for fires",
                    plan.location_error_m, self.settings.max_location_error_m
                )
            },
        ));

        // 2. Friendly positions: detected tracks and, once one exists, a
        // reported-position source (DN-25 §5 rule 5; GAP-090).
        checks.push(self.friendly_position_check(target, radius));

        // 3. No-fire areas.
        checks.push(Self::area_check(
            DeconflictionKind::NoFireArea,
            self.context.no_fire_areas,
            target,
            radius,
            "no-fire areas unavailable, so this check could not be evaluated",
            "the target error ellipse intersects a no-fire area",
            "clear of every no-fire area",
        ));

        // 4. Airspace measures.
        checks.push(Self::area_check(
            DeconflictionKind::AirspaceMeasure,
            self.context.airspace_measures,
            target,
            radius,
            "airspace measures unavailable, so this check could not be evaluated",
            "the trajectory violates a declared airspace measure",
            "clear of every declared airspace measure",
        ));

        // 5. Interceptor trajectories.
        checks.push(match self.context.interceptor_points {
            None => check(
                DeconflictionKind::InterceptorTrajectory,
                false,
                "intercept solutions unavailable, so this check could not be evaluated",
            ),
            Some(points) => {
                let conflict = points
                    .iter()
                    .any(|p| great_circle_distance_m(target, *p) <= radius);
                check(
                    DeconflictionKind::InterceptorTrajectory,
                    !conflict,
                    if conflict {
                        format!("an intercept solution lies inside the {radius:.0} m keep-out")
                    } else {
                        "no intercept solution conflicts".to_string()
                    },
                )
            }
        });

        DeconflictionResult { checks }
    }

    fn area_check(
        kind: DeconflictionKind,
        areas: Option<&[Geofence]>,
        target: Geodetic,
        radius_m: f64,
        unavailable: &'static str,
        breached: &'static str,
        clear: &'static str,
    ) -> DeconflictionCheck {
        match areas {
            None => check(kind, false, unavailable),
            Some(list) => {
                // The error ellipse intersects when the centres are closer than the
                // sum of the radii.
                let hit = list
                    .iter()
                    .any(|f| great_circle_distance_m(f.center, target) <= f.radius_m + radius_m);
                check(kind, !hit, if hit { breached } else { clear })
            }
        }
    }

    /// Rule 1, in full (DN-05 §5 rule 1; DN-25 §5 rule 5; GAP-090).
    ///
    /// A breach -- detected or reported -- is checked for and denies before
    /// anything about source availability is decided, so a known danger is never
    /// masked by an unrelated data gap. A reported position is scanned
    /// regardless of its affiliation filter result and regardless of staleness:
    /// only `Friendly` reports are ever compared against the keep-out (DN-25 §5
    /// rule 3, "may lower risk and never raise it"), and a stale one is not
    /// excluded from that comparison (DN-25 §5 rule 4, "a stale friendly is not
    /// a cleared fire mission") -- staleness is never a reason to stop treating a
    /// last-known friendly position as present.
    ///
    /// Only once no breach is found does availability decide the result, and
    /// that is where the three states DN-25 §5 rule 5 asks for are told apart:
    /// a live source reporting nobody passes and says so; a configured-but-silent
    /// source and an unconfigured one both fail, each with its own reason, and
    /// neither is ever a pass.
    fn friendly_position_check(&self, target: Geodetic, radius: f64) -> DeconflictionCheck {
        let kind = DeconflictionKind::FriendlyPosition;

        if let Some(positions) = self.context.friendly_positions {
            if let Some(p) = positions
                .iter()
                .find(|p| great_circle_distance_m(target, **p) <= radius)
            {
                return check(
                    kind,
                    false,
                    format!(
                        "a detected friendly position lies {:.0} m from the target, inside the {radius:.0} m keep-out",
                        great_circle_distance_m(target, *p)
                    ),
                );
            }
        }

        if let ReportedPositionSource::Live(reports) = self.context.reported_positions {
            if let Some(r) = reports
                .iter()
                .filter(|r| r.affiliation == Classification::Friendly)
                .find(|r| great_circle_distance_m(target, r.position) <= radius)
            {
                return check(
                    kind,
                    false,
                    format!(
                        "a reported friendly ({}) lies {:.0} m from the target, inside the {radius:.0} m keep-out",
                        r.reporter,
                        great_circle_distance_m(target, r.position)
                    ),
                );
            }
        }

        // No breach in whatever we had to look at. Whether that is a pass
        // depends on what we had, not on the absence of a breach alone.
        if self.context.friendly_positions.is_none() {
            return check(
                kind,
                false,
                "detected friendly positions unavailable, so this check could not be evaluated",
            );
        }
        match self.context.reported_positions {
            ReportedPositionSource::NotConfigured => check(
                kind,
                false,
                format!(
                    "no source configured for reported friendly positions, so a friendly beyond the detected picture cannot be ruled out of the {radius:.0} m keep-out; this check could not be evaluated"
                ),
            ),
            ReportedPositionSource::Silent => check(
                kind,
                false,
                "the reported-position source is configured and silent, so this check could not be evaluated",
            ),
            ReportedPositionSource::Live(_) => check(
                kind,
                true,
                format!(
                    "no detected friendly within {radius:.0} m, and the reported-position source is live and reports nobody within {radius:.0} m"
                ),
            ),
        }
    }
}

impl PolicyEngine for FiresDeconflictionPolicy<'_> {
    fn evaluate(&self, plan: &PlanView, _resources: &[ResourceView]) -> PolicyVerdict {
        let Some(fires) = plan.fires() else {
            // Not a fires plan: this engine has nothing to say, and saying nothing
            // means the rest of the chain decides.
            return PolicyVerdict::RequiresHumanApproval;
        };
        let result = self.deconflict(fires);
        if result.is_clear() {
            // Clear against deconfliction still requires a person. Contract C-01.
            PolicyVerdict::RequiresHumanApproval
        } else {
            PolicyVerdict::Denied {
                reason_code: DenialReason::FiresDeconfliction,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::{MissionTime, PeerOrigin, PlanId, PlanKind, ResourceId, TrackId};

    fn g(lat_deg: f64, lon_deg: f64) -> Geodetic {
        Geodetic {
            lat_rad: lat_deg.to_radians(),
            lon_rad: lon_deg.to_radians(),
            alt_m: 0.0,
        }
    }

    fn settings() -> FiresSettings {
        FiresSettings {
            max_location_error_m: 100.0,
            minimum_separation_m: 500.0,
        }
    }

    fn fires_plan(location_error_m: f64) -> FiresPlan {
        FiresPlan {
            target: TrackId(11),
            target_position: g(0.0, 0.0),
            location_error_m,
            firing_unit: ResourceId(5),
            time_on_target: None,
            deconfliction: DeconflictionResult::default(),
        }
    }

    /// A reported position, timed so `age_s()` is `receipt_time - peer_time`
    /// seconds, at `(lat_deg, lon_deg)`, claiming `affiliation`.
    fn reported(
        peer_time: f64,
        receipt_time: f64,
        lat_deg: f64,
        lon_deg: f64,
        affiliation: Classification,
    ) -> ReportedPosition {
        ReportedPosition {
            origin: PeerOrigin {
                peer: "mesh".into(),
                remote_track: String::new(),
                peer_time: MissionTime(peer_time),
                receipt_time: MissionTime(receipt_time),
                assigned_quality: 0.5,
            },
            reporter: "fire-group-2".into(),
            position: g(lat_deg, lon_deg),
            claimed_accuracy_m: Some(10.0),
            affiliation,
        }
    }

    /// Every check fully supplied and clear: detected friendlies (possibly
    /// empty), no no-fire areas, no airspace measures, no interceptors, and a
    /// **live** reported-position source with nobody in it. This is "everything
    /// the picture can supply, and it says clear" -- not today's real state,
    /// which is `FiresContext::default()` (see the tests below that use it
    /// directly).
    fn full_context<'a>(
        friendly: &'a [Geodetic],
        no_fire: &'a [Geofence],
        airspace: &'a [Geofence],
        interceptors: &'a [Geodetic],
    ) -> FiresContext<'a> {
        FiresContext {
            friendly_positions: Some(friendly),
            reported_positions: ReportedPositionSource::Live(&[]),
            no_fire_areas: Some(no_fire),
            airspace_measures: Some(airspace),
            interceptor_points: Some(interceptors),
        }
    }

    fn plan_of(fires: FiresPlan) -> PlanView {
        PlanView {
            id: PlanId(1),
            mission_time: MissionTime(0.0),
            kind: PlanKind::Fires(Box::new(fires)),
            policy_value: 1.0,
            releasability: gungnir_model::Releasability::default(),
        }
    }

    #[test]
    fn every_check_appears_in_the_result_whether_it_passed_or_failed() {
        let s = settings();
        let policy = FiresDeconflictionPolicy {
            settings: &s,
            context: full_context(&[], &[], &[], &[]),
        };
        let result = policy.deconflict(&fires_plan(40.0));
        assert_eq!(result.checks.len(), 5, "all five checks are reported");
        assert!(result.is_clear());
        for c in &result.checks {
            assert!(!c.detail.is_empty(), "{:?} must say why", c.kind);
        }
    }

    #[test]
    fn a_check_with_missing_data_fails_and_never_passes() {
        let s = settings();
        // Nothing supplied: four of the five checks cannot be evaluated.
        let policy = FiresDeconflictionPolicy {
            settings: &s,
            context: FiresContext::default(),
        };
        let result = policy.deconflict(&fires_plan(40.0));
        assert!(!result.is_clear());
        let failures: Vec<_> = result.failures().collect();
        assert_eq!(failures.len(), 4);
        for f in failures {
            assert!(
                f.detail.contains("could not be evaluated"),
                "{:?}: {}",
                f.kind,
                f.detail
            );
        }
    }

    #[test]
    fn a_target_whose_error_ellipse_touches_a_no_fire_area_is_denied() {
        let s = settings();
        // The keep-out is 40 + 500 = 540 m; a 200 m area 600 m away intersects.
        let area = [Geofence {
            center: g(0.0054, 0.0),
            radius_m: 200.0,
            no_go: false,
        }];
        let policy = FiresDeconflictionPolicy {
            settings: &s,
            context: full_context(&[], &area, &[], &[]),
        };
        let result = policy.deconflict(&fires_plan(40.0));
        assert!(!result.is_clear());
        assert!(result
            .failures()
            .any(|c| c.kind == DeconflictionKind::NoFireArea));

        let verdict = policy.evaluate(&plan_of(fires_plan(40.0)), &[]);
        assert!(matches!(
            verdict,
            PolicyVerdict::Denied {
                reason_code: DenialReason::FiresDeconfliction
            }
        ));
    }

    #[test]
    fn a_friendly_position_inside_the_keep_out_is_denied() {
        let s = settings();
        let friendly = [g(0.002, 0.0)];
        let policy = FiresDeconflictionPolicy {
            settings: &s,
            context: full_context(&friendly, &[], &[], &[]),
        };
        let result = policy.deconflict(&fires_plan(40.0));
        assert!(result
            .failures()
            .any(|c| c.kind == DeconflictionKind::FriendlyPosition));
    }

    #[test]
    fn a_poorly_located_target_fails_the_accuracy_check() {
        let s = settings();
        let policy = FiresDeconflictionPolicy {
            settings: &s,
            context: full_context(&[], &[], &[], &[]),
        };
        let result = policy.deconflict(&fires_plan(500.0));
        assert!(result
            .failures()
            .any(|c| c.kind == DeconflictionKind::LocationAccuracy));

        let non_finite = policy.deconflict(&fires_plan(f64::NAN));
        assert!(non_finite
            .failures()
            .any(|c| c.kind == DeconflictionKind::LocationAccuracy));
    }

    #[test]
    fn an_intercept_solution_inside_the_keep_out_is_denied() {
        let s = settings();
        let interceptors = [g(0.001, 0.0)];
        let policy = FiresDeconflictionPolicy {
            settings: &s,
            context: full_context(&[], &[], &[], &interceptors),
        };
        assert!(policy
            .deconflict(&fires_plan(40.0))
            .failures()
            .any(|c| c.kind == DeconflictionKind::InterceptorTrajectory));
    }

    #[test]
    fn several_failures_are_all_reported_not_just_the_first() {
        let s = settings();
        let friendly = [g(0.002, 0.0)];
        let area = [Geofence {
            center: g(0.0054, 0.0),
            radius_m: 200.0,
            no_go: false,
        }];
        let policy = FiresDeconflictionPolicy {
            settings: &s,
            context: full_context(&friendly, &area, &[], &[]),
        };
        let result = policy.deconflict(&fires_plan(40.0));
        assert!(
            result.failures().count() >= 2,
            "clearing one obstacle must not make the task look clean"
        );
    }

    #[test]
    fn a_clear_fires_plan_still_requires_a_human() {
        // Contract C-01: nothing here ever returns Approved.
        let s = settings();
        let policy = FiresDeconflictionPolicy {
            settings: &s,
            context: full_context(&[], &[], &[], &[]),
        };
        assert_eq!(
            policy.evaluate(&plan_of(fires_plan(40.0)), &[]),
            PolicyVerdict::RequiresHumanApproval
        );
    }

    #[test]
    fn an_intercept_plan_passes_through_this_engine_untouched() {
        let s = settings();
        let policy = FiresDeconflictionPolicy {
            settings: &s,
            context: FiresContext::default(),
        };
        let intercept = PlanView::intercept(PlanId(2), MissionTime(0.0), Vec::new(), 1.0);
        assert_eq!(
            policy.evaluate(&intercept, &[]),
            PolicyVerdict::RequiresHumanApproval,
            "this engine has nothing to say about an intercept"
        );
    }

    // GAP-090 / DN-25 §5 rule 5: rule 1's three states. Each row of CAP-3.8's
    // agreed pass criterion (verification-capability-table.md, "Rows added by
    // DN-25") gets its own test below, named after the clause it checks.

    fn friendly_position_result(context: FiresContext<'_>) -> DeconflictionCheck {
        let s = settings();
        let policy = FiresDeconflictionPolicy {
            settings: &s,
            context,
        };
        policy
            .deconflict(&fires_plan(40.0))
            .checks
            .into_iter()
            .find(|c| c.kind == DeconflictionKind::FriendlyPosition)
            .expect("the friendly-position check always runs")
    }

    #[test]
    fn a_reported_friendly_inside_the_ellipse_is_denied() {
        // "A reported friendly inside the target's error ellipse denies."
        let inside = [reported(100.0, 101.0, 0.002, 0.0, Classification::Friendly)];
        let fp = friendly_position_result(FiresContext {
            reported_positions: ReportedPositionSource::Live(&inside),
            ..full_context(&[], &[], &[], &[])
        });
        assert!(!fp.passed, "{}", fp.detail);
        assert!(fp.detail.contains("reported"), "{}", fp.detail);

        // The verdict carries the denial through to the policy chain, same as a
        // detected breach does today.
        let s = settings();
        let policy = FiresDeconflictionPolicy {
            settings: &s,
            context: FiresContext {
                reported_positions: ReportedPositionSource::Live(&inside),
                ..full_context(&[], &[], &[], &[])
            },
        };
        assert!(matches!(
            policy.evaluate(&plan_of(fires_plan(40.0)), &[]),
            PolicyVerdict::Denied {
                reason_code: DenialReason::FiresDeconfliction
            }
        ));
    }

    #[test]
    fn a_configured_source_reporting_nobody_passes_and_names_the_source() {
        // "a configured source reporting nobody in the ellipse passes and the
        // result names the source it read." An empty slice is a real reading
        // from a live source, not an absent one (DN-25 §5 rule 5).
        let fp = friendly_position_result(FiresContext {
            reported_positions: ReportedPositionSource::Live(&[]),
            ..full_context(&[], &[], &[], &[])
        });
        assert!(fp.passed, "{}", fp.detail);
        assert!(
            fp.detail.contains("reported-position source is live"),
            "the result must name what it read: {}",
            fp.detail
        );
        assert!(
            fp.detail.contains("detected"),
            "and where the detected half came from too: {}",
            fp.detail
        );
    }

    #[test]
    fn a_configured_and_silent_source_is_a_failed_check_never_a_pass() {
        // DN-25 §5 rule 5's middle state: configured, but not trusted right now
        // -- rule 2's existing "cannot be evaluated" failure, not a pass.
        let fp = friendly_position_result(FiresContext {
            reported_positions: ReportedPositionSource::Silent,
            ..full_context(&[], &[], &[], &[])
        });
        assert!(!fp.passed, "a silent source must never pass: {}", fp.detail);
        assert!(fp.detail.contains("silent"), "{}", fp.detail);
        assert!(
            fp.detail.contains("could not be evaluated"),
            "{}",
            fp.detail
        );
    }

    #[test]
    fn no_configured_source_is_a_failed_check_carrying_that_reason_never_a_pass() {
        // "no configured source is a failed check carrying that reason, and
        // never a pass -- an empty friendly set may not read as an absent one."
        // This is GAP-090's whole point: an empty *detected* set (a real
        // reading -- the tracker ran and found nobody friendly) must not be
        // read as "clear" when the concept of a self-reporting friendly does
        // not exist here.
        let empty_detected = friendly_position_result(FiresContext {
            friendly_positions: Some(&[]),
            reported_positions: ReportedPositionSource::NotConfigured,
            ..full_context(&[], &[], &[], &[])
        });
        assert!(!empty_detected.passed, "{}", empty_detected.detail);
        assert!(
            empty_detected.detail.contains("no source configured"),
            "{}",
            empty_detected.detail
        );

        // And never a pass even when the detected picture is non-empty and
        // clear: a source that does not exist cannot corroborate it.
        let some_detected = [g(45.0, 45.0)];
        let clear_detected = friendly_position_result(FiresContext {
            friendly_positions: Some(&some_detected),
            reported_positions: ReportedPositionSource::NotConfigured,
            ..full_context(&[], &[], &[], &[])
        });
        assert!(!clear_detected.passed, "{}", clear_detected.detail);
    }

    #[test]
    fn a_stale_reported_friendly_does_not_clear_a_fire_mission() {
        // "a reported friendly older than the configured maximum age does not
        // clear a fire mission." Staleness is visible (DN-16; DN-25 §5 rule 4)
        // but it is never a reason to stop treating a last-known friendly
        // position as present, so the check must still deny.
        let stale_and_inside = [reported(0.0, 600.0, 0.002, 0.0, Classification::Friendly)];
        assert!(
            stale_and_inside[0].origin.is_stale_beyond(30.0),
            "the fixture must actually be stale for this test to mean anything"
        );
        let fp = friendly_position_result(FiresContext {
            reported_positions: ReportedPositionSource::Live(&stale_and_inside),
            ..full_context(&[], &[], &[], &[])
        });
        assert!(
            !fp.passed,
            "a stale reported friendly inside the keep-out must still deny: {}",
            fp.detail
        );
    }

    #[test]
    fn a_reported_affiliation_other_than_friendly_changes_no_checks_result() {
        // "a report claiming any affiliation other than Friendly changes no
        // check's result." DN-25 §5 rule 3: a self-report may lower risk and
        // never raise it, so a claimed-hostile report inside the keep-out must
        // not deny -- only a claimed-friendly one may (the previous test).
        for affiliation in [
            Classification::Hostile,
            Classification::Neutral,
            Classification::Unknown,
        ] {
            let inside = [reported(100.0, 101.0, 0.002, 0.0, affiliation)];
            let fp = friendly_position_result(FiresContext {
                reported_positions: ReportedPositionSource::Live(&inside),
                ..full_context(&[], &[], &[], &[])
            });
            assert!(
                fp.passed,
                "a claimed {affiliation:?} report inside the ellipse must not deny: {}",
                fp.detail
            );
        }
    }

    #[test]
    fn todays_real_default_state_is_no_configured_source_never_a_silent_pass() {
        // GAP-090's honesty target. `FiresContext::default()` is what every
        // real construction site builds today (`gungnir-app/src/decisions.rs`),
        // because GAP-091's feed does not exist: `reported_positions` defaults
        // to `NotConfigured`. Before this change an empty detected set here
        // read as a silent pass; now it must fail and say why.
        assert!(matches!(
            FiresContext::default().reported_positions,
            ReportedPositionSource::NotConfigured
        ));
        let fp = friendly_position_result(FiresContext {
            friendly_positions: Some(&[]),
            ..FiresContext::default()
        });
        assert!(!fp.passed, "{}", fp.detail);
        assert!(fp.detail.contains("no source configured"), "{}", fp.detail);
    }
}

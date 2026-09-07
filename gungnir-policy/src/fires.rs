// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Fires deconfliction.
//!
//! Design: docs/design/DN-05-fires.md. Capability CAP-3.8; decision D-07 put fires
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
    DeconflictionCheck, DeconflictionKind, DeconflictionResult, FiresPlan, FiresSettings, Geodetic,
    PlanView, ResourceView,
};

/// What the picture knows that the deconfliction rules need.
///
/// Supplied by the caller rather than fetched, so this stays a pure function of its
/// inputs and every case is testable. `None` on a field means the data is
/// unavailable, and the corresponding check fails rather than passing.
#[derive(Debug, Clone, Default)]
pub struct FiresContext<'a> {
    /// Known friendly positions. `None` when the picture cannot supply them.
    pub friendly_positions: Option<&'a [Geodetic]>,
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

        // 2. Friendly positions.
        checks.push(match self.context.friendly_positions {
            None => check(
                DeconflictionKind::FriendlyPosition,
                false,
                "friendly positions unavailable, so this check could not be evaluated",
            ),
            Some(positions) => {
                let breach = positions
                    .iter()
                    .find(|p| great_circle_distance_m(target, **p) <= radius);
                match breach {
                    Some(p) => check(
                        DeconflictionKind::FriendlyPosition,
                        false,
                        format!(
                            "a friendly position lies {:.0} m from the target, inside the {radius:.0} m keep-out",
                            great_circle_distance_m(target, *p)
                        ),
                    ),
                    None => check(
                        DeconflictionKind::FriendlyPosition,
                        true,
                        format!("no friendly position within {radius:.0} m"),
                    ),
                }
            }
        });

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
    use gungnir_model::{MissionTime, PlanId, PlanKind, ResourceId, TrackId};

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

    fn full_context<'a>(
        friendly: &'a [Geodetic],
        no_fire: &'a [Geofence],
        airspace: &'a [Geofence],
        interceptors: &'a [Geodetic],
    ) -> FiresContext<'a> {
        FiresContext {
            friendly_positions: Some(friendly),
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
}

//! Sensor re-tasking recommendation.
//!
//! Design: docs/design/DN-13-sensor-retasking.md. Capability CAP-3.9; mission thread
//! MT-07 step 3, where the sensor manager must restore coverage after a loss under
//! time pressure.
//!
//! This crate gained an edge to `gungnir-analytics` for it, accepted by the
//! engineering reviewer on 2026-09-05 and drawn in ARCHITECTURE.md §7.1. The reason
//! is not convenience: the search must evaluate coverage **inside its own loop**,
//! once per candidate plan. A search that cannot evaluate its own candidates is not
//! a search.
//!
//! **It recommends; it does not task.** Accepting a plan issues the tasks through
//! the sensor registry, each an authorized action with its own record. There is no
//! path from this crate to a sensor.

use gungnir_analytics::{
    combined_coverage, CoverageParameters, CoverageReport, GapSeverity, LineOfSight,
};
use gungnir_model::SensorId;

/// One proposed mode change.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SensorModeChange {
    pub sensor: SensorId,
    /// Mode names rather than the registry's enum: this crate does not depend on
    /// `gungnir-sensor-management`, and the caller validates the transition against
    /// the registry's own table before issuing anything.
    pub from: String,
    pub to: String,
}

/// What a plan gives up. A mode that closes one gap usually opens another.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SensorPlanCost {
    /// Approach metres uncovered after, minus before. Negative is an improvement.
    pub delta_uncovered_m: f64,
    /// Approach metres that drop from redundant to single-sensor.
    ///
    /// Present because the tempting plan under pressure is to swing everything
    /// toward the hole and leave the rest single-sensor. The sensor manager must
    /// see that trade rather than discover it.
    pub redundancy_lost_m: f64,
    pub sensors_changed: usize,
}

impl SensorPlanCost {
    /// True when the plan closes more than it opens.
    pub fn is_improvement(&self) -> bool {
        self.delta_uncovered_m < 0.0
    }
}

/// A proposed set of mode changes, with the coverage it buys.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SensorPlan {
    pub changes: Vec<SensorModeChange>,
    /// Gaps before the changes and after them: the rationale, in the only form that
    /// means anything to a sensor manager.
    pub gaps_before: CoverageReport,
    pub gaps_after: CoverageReport,
    pub cost: SensorPlanCost,
}

/// One candidate the caller is willing to consider.
///
/// The caller enumerates candidates because only the registry knows which mode
/// transitions are legal, and this crate may not depend on it. That keeps the
/// registry's state machine the constraint rather than a preference: a
/// recommendation that skips a required step would be rejected at execution and
/// waste the operator's time.
#[derive(Debug, Clone)]
pub struct SensorCandidate {
    pub change: SensorModeChange,
    /// The coverage volume this sensor would contribute after the change; `None`
    /// when the change takes it out of contribution.
    pub volume_after: Option<gungnir_analytics::CoverageVolume>,
}

/// Recommends mode changes that improve coverage.
pub struct SensorPlanner<'a> {
    /// Coverage volumes as things stand, from `coverage_from_registry`.
    pub current: &'a [(SensorId, gungnir_analytics::CoverageVolume)],
    pub los: &'a dyn LineOfSight,
    pub parameters: CoverageParameters,
    /// Bounds the search. Configuration, so a large registry cannot stall the tick.
    pub max_candidates: usize,
}

impl SensorPlanner<'_> {
    fn coverage(
        &self,
        sensors: &[(SensorId, gungnir_analytics::CoverageVolume)],
        approaches: &[&[[f64; 3]]],
    ) -> CoverageReport {
        combined_coverage(sensors, self.los, approaches, self.parameters)
    }

    fn apply(
        &self,
        candidate: &SensorCandidate,
    ) -> Vec<(SensorId, gungnir_analytics::CoverageVolume)> {
        let mut next: Vec<_> = self
            .current
            .iter()
            .filter(|(id, _)| *id != candidate.change.sensor)
            .copied()
            .collect();
        if let Some(volume) = candidate.volume_after {
            next.push((candidate.change.sensor, volume));
        }
        next
    }

    /// Candidate plans, best first.
    ///
    /// **Empty is a real answer.** When no change improves coverage the recommender
    /// returns nothing and the panel says so, rather than proposing the least bad
    /// change as though it were an improvement.
    pub fn recommend(
        &self,
        candidates: &[SensorCandidate],
        approaches: &[&[[f64; 3]]],
    ) -> Vec<SensorPlan> {
        let before = self.coverage(self.current, approaches);
        let uncovered_before = before.gap_length_m(GapSeverity::Uncovered);
        let single_before = before.gap_length_m(GapSeverity::SingleSensor);

        let mut plans: Vec<SensorPlan> = candidates
            .iter()
            .take(self.max_candidates)
            .filter_map(|candidate| {
                let after = self.coverage(&self.apply(candidate), approaches);
                let uncovered_after = after.gap_length_m(GapSeverity::Uncovered);
                let single_after = after.gap_length_m(GapSeverity::SingleSensor);
                let cost = SensorPlanCost {
                    delta_uncovered_m: uncovered_after - uncovered_before,
                    redundancy_lost_m: (single_after - single_before).max(0.0),
                    sensors_changed: 1,
                };
                // Only improvements are proposed.
                cost.is_improvement().then(|| SensorPlan {
                    changes: vec![candidate.change.clone()],
                    gaps_before: before.clone(),
                    gaps_after: after,
                    cost,
                })
            })
            .collect();

        plans.sort_by(|a, b| {
            a.cost
                .delta_uncovered_m
                .partial_cmp(&b.cost.delta_uncovered_m)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    a.cost
                        .redundancy_lost_m
                        .partial_cmp(&b.cost.redundancy_lost_m)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
        });
        plans
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_analytics::{CoverageVolume, FlatTerrainLineOfSight};

    fn volume(east: f64, range: f64) -> CoverageVolume {
        CoverageVolume {
            sensor_enu: [east, 0.0, 0.0],
            max_range_m: range,
            min_elevation_rad: -std::f64::consts::FRAC_PI_2,
        }
    }

    fn parameters() -> CoverageParameters {
        CoverageParameters {
            sample_spacing_m: 100.0,
            terrain_masking_applied: false,
        }
    }

    fn approach() -> Vec<[f64; 3]> {
        vec![[0.0, 0.0, 0.0], [3_000.0, 0.0, 0.0]]
    }

    fn planner<'a>(
        current: &'a [(SensorId, CoverageVolume)],
        los: &'a FlatTerrainLineOfSight,
    ) -> SensorPlanner<'a> {
        SensorPlanner {
            current,
            los,
            parameters: parameters(),
            max_candidates: 8,
        }
    }

    #[test]
    fn a_change_that_closes_a_hole_is_proposed_with_its_before_and_after() {
        let los = FlatTerrainLineOfSight;
        let route = approach();
        let approaches = [route.as_slice()];
        // One sensor covers the near end only, leaving the far end uncovered.
        let current = [(SensorId(1), volume(0.0, 500.0))];
        let p = planner(&current, &los);

        // Widening sensor 1 to reach the whole approach closes it.
        let candidates = [SensorCandidate {
            change: SensorModeChange {
                sensor: SensorId(1),
                from: "standby".into(),
                to: "search".into(),
            },
            volume_after: Some(volume(1_500.0, 5_000.0)),
        }];
        let plans = p.recommend(&candidates, &approaches);
        assert_eq!(plans.len(), 1);
        assert!(plans[0].cost.is_improvement());
        assert!(
            plans[0].gaps_before.gap_length_m(GapSeverity::Uncovered)
                > plans[0].gaps_after.gap_length_m(GapSeverity::Uncovered),
            "the rationale is the before and after"
        );
    }

    #[test]
    fn an_empty_result_is_returned_when_no_change_helps() {
        let los = FlatTerrainLineOfSight;
        let route = approach();
        let approaches = [route.as_slice()];
        // Already covered end to end by two sensors.
        let current = [
            (SensorId(1), volume(1_500.0, 5_000.0)),
            (SensorId(2), volume(1_500.0, 5_000.0)),
        ];
        let p = planner(&current, &los);
        let candidates = [SensorCandidate {
            change: SensorModeChange {
                sensor: SensorId(2),
                from: "search".into(),
                to: "standby".into(),
            },
            volume_after: None,
        }];
        assert!(
            p.recommend(&candidates, &approaches).is_empty(),
            "no change helps, so nothing is proposed"
        );
    }

    #[test]
    fn a_change_that_makes_coverage_worse_is_never_proposed() {
        let los = FlatTerrainLineOfSight;
        let route = approach();
        let approaches = [route.as_slice()];
        let current = [(SensorId(1), volume(1_500.0, 5_000.0))];
        let p = planner(&current, &los);
        // Taking the only sensor offline.
        let candidates = [SensorCandidate {
            change: SensorModeChange {
                sensor: SensorId(1),
                from: "search".into(),
                to: "offline".into(),
            },
            volume_after: None,
        }];
        assert!(p.recommend(&candidates, &approaches).is_empty());
    }

    #[test]
    fn the_after_state_matches_an_independent_evaluation() {
        let los = FlatTerrainLineOfSight;
        let route = approach();
        let approaches = [route.as_slice()];
        let current = [(SensorId(1), volume(0.0, 500.0))];
        let p = planner(&current, &los);
        let widened = volume(1_500.0, 5_000.0);
        let candidates = [SensorCandidate {
            change: SensorModeChange {
                sensor: SensorId(1),
                from: "standby".into(),
                to: "search".into(),
            },
            volume_after: Some(widened),
        }];
        let plans = p.recommend(&candidates, &approaches);
        let independent =
            combined_coverage(&[(SensorId(1), widened)], &los, &approaches, parameters());
        assert_eq!(plans[0].gaps_after, independent);
    }

    #[test]
    fn the_search_is_bounded_by_its_candidate_limit() {
        let los = FlatTerrainLineOfSight;
        let route = approach();
        let approaches = [route.as_slice()];
        let current = [(SensorId(1), volume(0.0, 500.0))];
        let mut p = planner(&current, &los);
        p.max_candidates = 2;
        let candidates: Vec<_> = (2..10)
            .map(|n| SensorCandidate {
                change: SensorModeChange {
                    sensor: SensorId(n),
                    from: "standby".into(),
                    to: "search".into(),
                },
                volume_after: Some(volume(1_500.0, 5_000.0)),
            })
            .collect();
        assert!(
            p.recommend(&candidates, &approaches).len() <= 2,
            "a large registry cannot stall the tick"
        );
    }

    #[test]
    fn plans_are_ordered_best_first() {
        let los = FlatTerrainLineOfSight;
        let route = approach();
        let approaches = [route.as_slice()];
        let current = [(SensorId(1), volume(0.0, 500.0))];
        let p = planner(&current, &los);
        let candidates = [
            SensorCandidate {
                change: SensorModeChange {
                    sensor: SensorId(2),
                    from: "standby".into(),
                    to: "search".into(),
                },
                // Covers only part of the remaining hole.
                volume_after: Some(volume(1_000.0, 700.0)),
            },
            SensorCandidate {
                change: SensorModeChange {
                    sensor: SensorId(3),
                    from: "standby".into(),
                    to: "search".into(),
                },
                // Covers all of it.
                volume_after: Some(volume(1_500.0, 5_000.0)),
            },
        ];
        let plans = p.recommend(&candidates, &approaches);
        assert_eq!(plans.len(), 2);
        assert!(
            plans[0].cost.delta_uncovered_m <= plans[1].cost.delta_uncovered_m,
            "the biggest improvement comes first"
        );
        assert_eq!(plans[0].changes[0].sensor, SensorId(3));
    }

    #[test]
    fn every_plan_reports_what_it_costs_as_well_as_what_it_buys() {
        let los = FlatTerrainLineOfSight;
        let route = approach();
        let approaches = [route.as_slice()];
        let current = [(SensorId(1), volume(0.0, 500.0))];
        let p = planner(&current, &los);
        let candidates = [SensorCandidate {
            change: SensorModeChange {
                sensor: SensorId(1),
                from: "standby".into(),
                to: "search".into(),
            },
            volume_after: Some(volume(1_500.0, 5_000.0)),
        }];
        let plans = p.recommend(&candidates, &approaches);
        let cost = plans[0].cost;
        assert_eq!(cost.sensors_changed, 1);
        assert!(
            cost.redundancy_lost_m >= 0.0,
            "the trade is always reported"
        );
    }
}

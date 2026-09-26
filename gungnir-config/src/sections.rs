// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Which sections of a baseline a candidate changes, and what kind of section each is
//! (GAP-162, D-91; `docs/mission/roles-and-stakeholders.md` §4, "Applying a baseline,
//! section by section"; DN-08 §10).
//!
//! **Why sections.** A baseline is one file and `config.apply` was one action for all of
//! it, so a sensor manager applying a calibration change could change weapons control
//! status and the authority rules in the same file. What a person may apply is decided
//! by what the candidate changes against the baseline in force, and this module says
//! what that is. Which action each [`SectionKind`] needs is authority, and lives beside
//! the rest of it: `gungnir-app`'s PN-14 apply maps a kind to a `gungnir-security`
//! action, which this crate may not reach.
//!
//! **Why a destructuring rather than a list of names.** [`changed_sections`] binds every
//! field of [`ConfigBaseline`] and of [`gungnir_model::PolicySettings`] by name, with no
//! `..`, so a field added to either without a kind here does not compile. A list of
//! names would let a new field through unclassified, and an unclassified section is one
//! nobody's authority covers.

use crate::ConfigBaseline;

/// What kind of section a baseline field is, which decides the authority a change to
/// it needs (§4, D-91).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SectionKind {
    /// Sensors, feeds, laydowns, tracking calibration, terrain, point clouds: the sensor
    /// manager's calibration row.
    Sensing,
    /// The engagement chain: every policy section, resources, assets, geofences, hazards,
    /// approaches, the allocation horizon and solve budget, assessment.
    EngagementChain,
    /// Accounts and authentication, keys, TLS, escrow, machine identities, retention.
    Security,
    /// The deployment's shape: backend, node, peers, exchange agreements, endpoints and
    /// the rest of what a whole-baseline apply covers.
    Deployment,
}

/// One section a candidate changes, named as the file names it (`policy` is split into
/// its own sections, `policy.control_status` and the rest, because one of them is what
/// this whole module is for).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChangedSection {
    pub name: &'static str,
    pub kind: SectionKind,
}

/// Every section `candidate` changes against `in_force`, in the file's field order.
///
/// `version` and `revision` are left out: every apply advances the revision, and the
/// schema version is checked by validation, so neither is a change of content anyone
/// needs authority for.
// One line per field of the baseline is the point: splitting it would split the one place
// every field is named.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn changed_sections(
    in_force: &ConfigBaseline,
    candidate: &ConfigBaseline,
) -> Vec<ChangedSection> {
    use SectionKind::{Deployment, EngagementChain, Security, Sensing};

    let ConfigBaseline {
        version: _,
        revision: _,
        sensors,
        radar_feeds,
        ais_feeds,
        adsb_feeds,
        misb_feeds,
        sapient_feeds,
        sapient_node_id,
        peers,
        exchange,
        machine_identities,
        resources,
        laydowns,
        tracking,
        backend,
        node,
        allocation_horizon,
        plan_solve_budget_ms,
        data_dir,
        assets,
        endpoints,
        policy,
        assessment,
        terrain,
        point_cloud,
        ui,
        vocabulary,
        analytics,
        sensor_task_ack_window_s,
        approaches,
        origin,
        hazards,
        geofences,
        validity,
        reporting,
        mission_profiles,
        tracking_profiles,
        active_profile,
        security,
        retention,
    } = candidate;
    let gungnir_model::PolicySettings {
        identification,
        staleness,
        control_status,
        authority,
        decisions,
        delegation,
        fires,
    } = policy;
    let was = in_force;

    // The exact float comparison is deliberate: any change to the value is a change the
    // applier needs authority for, however small.
    #[allow(clippy::float_cmp)]
    let sections = [
        (sensors != &was.sensors, "sensors", Sensing),
        (radar_feeds != &was.radar_feeds, "radar_feeds", Sensing),
        (ais_feeds != &was.ais_feeds, "ais_feeds", Sensing),
        (adsb_feeds != &was.adsb_feeds, "adsb_feeds", Sensing),
        (misb_feeds != &was.misb_feeds, "misb_feeds", Sensing),
        (
            sapient_feeds != &was.sapient_feeds,
            "sapient_feeds",
            Sensing,
        ),
        (
            sapient_node_id != &was.sapient_node_id,
            "sapient_node_id",
            Sensing,
        ),
        (peers != &was.peers, "peers", Deployment),
        (exchange != &was.exchange, "exchange", Deployment),
        (
            machine_identities != &was.machine_identities,
            "machine_identities",
            Security,
        ),
        (resources != &was.resources, "resources", EngagementChain),
        (laydowns != &was.laydowns, "laydowns", Sensing),
        (tracking != &was.tracking, "tracking", Sensing),
        (backend != &was.backend, "backend", Deployment),
        (node != &was.node, "node", Deployment),
        (
            allocation_horizon != &was.allocation_horizon,
            "allocation_horizon",
            EngagementChain,
        ),
        (
            plan_solve_budget_ms != &was.plan_solve_budget_ms,
            "plan_solve_budget_ms",
            EngagementChain,
        ),
        (data_dir != &was.data_dir, "data_dir", Deployment),
        (assets != &was.assets, "assets", EngagementChain),
        (endpoints != &was.endpoints, "endpoints", Deployment),
        (
            identification != &was.policy.identification,
            "policy.identification",
            EngagementChain,
        ),
        (
            staleness != &was.policy.staleness,
            "policy.staleness",
            EngagementChain,
        ),
        (
            control_status != &was.policy.control_status,
            "policy.control_status",
            EngagementChain,
        ),
        (
            authority != &was.policy.authority,
            "policy.authority",
            EngagementChain,
        ),
        (
            decisions != &was.policy.decisions,
            "policy.decisions",
            EngagementChain,
        ),
        (
            delegation != &was.policy.delegation,
            "policy.delegation",
            EngagementChain,
        ),
        (fires != &was.policy.fires, "policy.fires", EngagementChain),
        (assessment != &was.assessment, "assessment", EngagementChain),
        (terrain != &was.terrain, "terrain", Sensing),
        (point_cloud != &was.point_cloud, "point_cloud", Sensing),
        (ui != &was.ui, "ui", Deployment),
        (vocabulary != &was.vocabulary, "vocabulary", Deployment),
        (analytics != &was.analytics, "analytics", Deployment),
        (
            sensor_task_ack_window_s != &was.sensor_task_ack_window_s,
            "sensor_task_ack_window_s",
            Sensing,
        ),
        (approaches != &was.approaches, "approaches", EngagementChain),
        (origin != &was.origin, "origin", Deployment),
        (hazards != &was.hazards, "hazards", EngagementChain),
        (geofences != &was.geofences, "geofences", EngagementChain),
        (validity != &was.validity, "validity", Deployment),
        (reporting != &was.reporting, "reporting", Deployment),
        (
            mission_profiles != &was.mission_profiles,
            "mission_profiles",
            Deployment,
        ),
        (
            tracking_profiles != &was.tracking_profiles,
            "tracking_profiles",
            Deployment,
        ),
        (
            active_profile != &was.active_profile,
            "active_profile",
            Deployment,
        ),
        (security != &was.security, "security", Security),
        (retention != &was.retention, "retention", Security),
    ];
    sections
        .into_iter()
        .filter(|(changed, _, _)| *changed)
        .map(|(_, name, kind)| ChangedSection { name, kind })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unchanged_candidate_changes_nothing_and_a_new_revision_is_not_a_change() {
        let in_force = ConfigBaseline::default();
        let mut candidate = in_force.clone();
        assert!(changed_sections(&in_force, &candidate).is_empty());
        candidate.revision += 1;
        assert!(changed_sections(&in_force, &candidate).is_empty());
    }

    #[test]
    fn a_control_status_change_is_named_and_is_the_engagement_chain() {
        let in_force = ConfigBaseline::default();
        let mut candidate = in_force.clone();
        candidate.policy.control_status.by_layer.insert(
            gungnir_model::EffectorLayer::Point,
            gungnir_model::WeaponsControlStatus::Free,
        );
        assert_ne!(
            in_force.policy.control_status, candidate.policy.control_status,
            "the fixture must change the control status"
        );
        assert_eq!(
            changed_sections(&in_force, &candidate),
            vec![ChangedSection {
                name: "policy.control_status",
                kind: SectionKind::EngagementChain,
            }]
        );
    }

    #[test]
    fn a_sensor_change_is_sensing_and_a_security_change_is_security() {
        let in_force = ConfigBaseline::default();
        let mut candidate = in_force.clone();
        candidate.sensor_task_ack_window_s += 5.0;
        candidate.retention = Some(gungnir_model::RetentionPolicy::default());
        let changed = changed_sections(&in_force, &candidate);
        assert_eq!(
            changed,
            vec![
                ChangedSection {
                    name: "sensor_task_ack_window_s",
                    kind: SectionKind::Sensing,
                },
                ChangedSection {
                    name: "retention",
                    kind: SectionKind::Security,
                },
            ]
        );
    }
}

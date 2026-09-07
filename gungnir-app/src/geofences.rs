// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Geofences from the baseline to the policy chain and the map (GAP-088).
//!
//! `InMemoryGeoService` was "loaded from config or an operator action" and nothing did
//! either, so the geofence engine ran against an empty service on both binaries. The
//! baseline now declares fences and this builds the service and the outlines the
//! viewport draws. The same conversion lives in `gungnir-node`, which has no app edge.

use crate::state::AppState;
use gungnir_config::ConfigBaseline;
use gungnir_geo::{Geofence, InMemoryGeoService};
use gungnir_viewport3d::layers::GeofenceOutline;

/// The service the chain reads, holding every fence the baseline declares.
#[must_use]
pub fn service_from_config(config: &ConfigBaseline) -> InMemoryGeoService {
    InMemoryGeoService::new(
        Vec::new(),
        config
            .geofences
            .iter()
            .map(|g| Geofence {
                center: gungnir_model::Geodetic {
                    lat_rad: g.center[0],
                    lon_rad: g.center[1],
                    alt_m: g.center[2],
                },
                radius_m: g.radius_m,
                no_go: g.no_go,
            })
            .collect(),
    )
}

/// One fence placed in the local frame, owning what the viewport borrows.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedGeofence {
    pub name: String,
    pub center_enu: [f64; 3],
    pub radius_m: f64,
    pub no_go: bool,
}

/// Every fence placed at the local frame; empty without an origin, and PN-11's counts
/// say declared against placed the way they do for hazards.
#[must_use]
pub fn placed(state: &AppState) -> Vec<PlacedGeofence> {
    let Some(frame) = crate::sustainment::local_frame(state) else {
        return Vec::new();
    };
    state
        .config
        .geofences
        .iter()
        .map(|g| PlacedGeofence {
            name: g.name.clone(),
            center_enu: frame.to_enu(gungnir_model::Geodetic {
                lat_rad: g.center[0],
                lon_rad: g.center[1],
                alt_m: g.center[2],
            }),
            radius_m: g.radius_m,
            no_go: g.no_go,
        })
        .collect()
}

#[must_use]
pub fn outlines(placed: &[PlacedGeofence]) -> Vec<GeofenceOutline<'_>> {
    placed
        .iter()
        .map(|p| GeofenceOutline {
            name: &p.name,
            center_enu: p.center_enu,
            radius_m: p.radius_m,
            no_go: p.no_go,
        })
        .collect()
}

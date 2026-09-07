// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The static hazard layer, from the baseline to the map (DN-14, GAP-017).
//!
//! `gungnir_geo::HazardLayer` existed with its tests and nothing built one. This is the
//! builder and the placement: the baseline's [`HazardConfig`] list becomes the layer,
//! stamped with the baseline version (DN-14 §5: currency is stated), and the layer becomes
//! ENU outlines the viewport draws.
//!
//! **Descriptive, never a rule.** Nothing here is reachable from `gungnir-policy` or
//! `gungnir-command`, and `gungnir-geo/tests/no_hazard_in_the_policy_chain.rs` fails the
//! build the day somebody wires it in.

use crate::state::AppState;
use gungnir_config::{ConfigBaseline, ConfigError, HazardConfig, HazardShapeConfig};
use gungnir_geo::{Hazard, HazardExtent, HazardKind, HazardLayer};
use gungnir_model::Geodetic;
use gungnir_viewport3d::layers::HazardOutline;

/// Samples around a circular hazard's outline. Enough that a 40 m net reads as round
/// at harbour zoom; few enough that a hundred shoals are not a frame-time problem.
const CIRCLE_SAMPLES: usize = 48;

/// The kind, as the baseline spells it.
///
/// # Errors
///
/// A [`ConfigError::Invalid`] for a spelling `gungnir_geo::HazardKind` has no variant
/// for. `gungnir_config::validate` refuses these first; this is the guard for a baseline
/// that reached the desktop without it.
pub fn parse_kind(kind: &str) -> Result<HazardKind, ConfigError> {
    Ok(match kind {
        "boom" => HazardKind::Boom,
        "net" => HazardKind::Net,
        "barrier" => HazardKind::Barrier,
        "wreck" => HazardKind::Wreck,
        "shoal" => HazardKind::Shoal,
        "other" => HazardKind::Other,
        other => {
            return Err(ConfigError::Invalid(format!(
                "hazard kind {other:?} is not one of {:?}",
                HazardConfig::KINDS
            )))
        }
    })
}

/// The kind, in the words the map labels it with.
#[must_use]
pub fn kind_label(kind: HazardKind) -> &'static str {
    match kind {
        HazardKind::Boom => "boom",
        HazardKind::Net => "net",
        HazardKind::Barrier => "barrier",
        HazardKind::Wreck => "wreck",
        HazardKind::Shoal => "shoal",
        HazardKind::Other => "hazard",
    }
}

fn geodetic([lat_rad, lon_rad, alt_m]: [f64; 3]) -> Geodetic {
    Geodetic {
        lat_rad,
        lon_rad,
        alt_m,
    }
}

/// Build the layer the baseline declares, stamped with the baseline's version.
///
/// # Errors
///
/// See [`parse_kind`].
pub fn layer_from_config(config: &ConfigBaseline) -> Result<HazardLayer, ConfigError> {
    let hazards = config
        .hazards
        .iter()
        .map(|h| {
            Ok(Hazard {
                name: h.name.clone(),
                kind: parse_kind(&h.kind)?,
                extent: match &h.shape {
                    HazardShapeConfig::Polyline { points } => HazardExtent::Polyline {
                        points: points.iter().copied().map(geodetic).collect(),
                    },
                    HazardShapeConfig::Circle { center, radius_m } => HazardExtent::Circle {
                        center: geodetic(*center),
                        radius_m: *radius_m,
                    },
                },
                blocks_surface: h.blocks_surface,
                height_m: h.height_m,
            })
        })
        .collect::<Result<Vec<_>, ConfigError>>()?;
    Ok(HazardLayer {
        baseline_version: config.revision,
        hazards,
    })
}

/// One hazard placed in the local ENU frame.
///
/// Owns what it carries, so the viewport's borrowing [`HazardOutline`] points into this
/// list and not into `AppState` -- the viewport state is borrowed mutably beside it, the
/// same reason `coverage_circles` returns owned circles.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedHazard {
    pub name: String,
    pub kind: HazardKind,
    pub blocks_surface: bool,
    pub samples: Vec<[f64; 3]>,
}

/// Place the layer in the local frame.
///
/// **Empty without an origin**, and the count PN-11 shows beside the declared count is how
/// the operator learns that: the hazards are not missing, they are unplaceable, the same
/// claim the coverage rings make (GAP-007).
#[must_use]
pub fn placed(state: &AppState) -> Vec<PlacedHazard> {
    let Some(frame) = crate::sustainment::local_frame(state) else {
        return Vec::new();
    };
    state
        .hazards
        .hazards
        .iter()
        .map(|hazard| {
            let samples = match &hazard.extent {
                HazardExtent::Polyline { points } => {
                    points.iter().map(|p| frame.to_enu(*p)).collect()
                }
                HazardExtent::Circle { center, radius_m } => {
                    // A ring in the tangent plane about the placed centre: at hazard
                    // radii (tens to hundreds of metres) the plane and the sphere agree
                    // to well under a metre.
                    let [e, n, u] = frame.to_enu(*center);
                    (0..=CIRCLE_SAMPLES)
                        .map(|i| {
                            #[allow(clippy::cast_precision_loss)]
                            let theta = std::f64::consts::TAU * (i % CIRCLE_SAMPLES) as f64
                                / CIRCLE_SAMPLES as f64;
                            [e + radius_m * theta.cos(), n + radius_m * theta.sin(), u]
                        })
                        .collect()
                }
            };
            PlacedHazard {
                name: hazard.name.clone(),
                kind: hazard.kind,
                blocks_surface: hazard.blocks_surface,
                samples,
            }
        })
        .collect()
}

/// The outlines as the viewport takes them.
#[must_use]
pub fn outlines(placed: &[PlacedHazard]) -> Vec<HazardOutline<'_>> {
    placed
        .iter()
        .map(|p| HazardOutline {
            name: &p.name,
            kind: kind_label(p.kind),
            samples: &p.samples,
            blocks_surface: p.blocks_surface,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two spellings of the kind list -- the baseline's and the geometry crate's --
    /// are kept in step here, where they meet.
    #[test]
    fn every_configurable_kind_maps_to_a_geo_kind_and_back() {
        for kind in HazardConfig::KINDS {
            let parsed = parse_kind(kind).unwrap_or_else(|e| panic!("{kind}: {e}"));
            let label = kind_label(parsed);
            assert!(
                label == kind || (kind == "other" && label == "hazard"),
                "{kind} -> {parsed:?} -> {label}"
            );
        }
        assert!(parse_kind("minefield").is_err());
    }
}

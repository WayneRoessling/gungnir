// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Coverage on the map (GAP-007), and the static hazards (GAP-017).
//!
//! `docs/ux/ux-to-code-map.md` §1 puts this in `gungnir-viewport3d/src/layers.rs`, drawn
//! in the 2D projection and in the three-d scene.
//!
//! Hazards follow the same rule as gaps: [`HazardOutline`] is an ENU polyline the caller
//! has already placed, because `gungnir_geo::Hazard` is geodetic and lives in a crate this
//! one must not depend on (DN-14 §4 adds no edge, and this crate adds none either).
//!
//! # Plain ENU circles, not `CoverageRegion`
//!
//! `gungnir_sensor_management::CoverageRegion` is geodetic and lives in a crate this one
//! must not depend on. So the caller converts, and this takes [`CoverageCircle`] --
//! centre in local ENU metres, radius in metres, and the confidence that decides how
//! solidly it is drawn. Same reason the panels take view structs: the rendering crate
//! draws what it is given and knows nothing about the sensor registry.
//!
//! # A coverage circle is a claim, and drawing one nobody can place is the failure
//!
//! Placing a geodetic circle in an ENU picture needs the local frame's origin, and a
//! deployment may not have declared one (GAP-007 forced that question; see
//! `gungnir_model::LocalFrame`). There is no sound default for it, so when there is no
//! origin the caller passes no circles and the viewport says why. Drawing coverage
//! centred on a guessed origin would put a plausible ring in the wrong place, and a
//! sensor manager reading a coverage map trusts exactly that ring.

use crate::interaction::TopDownView;
use gungnir_ui::theme;

/// One sensor's coverage, already in the local ENU frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoverageCircle {
    pub sensor: u32,
    /// Centre `[e, n, u]` in metres.
    pub center: [f64; 3],
    pub radius_m: f64,
    /// 1.0 for a tracking sensor, lower for a searching one. Drawn as opacity, so a
    /// weaker claim looks like one.
    pub confidence: f32,
}

/// Why there is no coverage to draw. Distinct cases, because they are different
/// situations for the person looking at the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoCoverage<'a> {
    /// Sensors are configured and none is searching or tracking, so none is covering
    /// anything. A true and complete answer.
    NoSensorsActive,
    /// The deployment has not declared a local frame origin, so a geodetic coverage
    /// region cannot be put anywhere on this picture. Names what would fix it.
    NoOrigin { setting: &'a str },
}

impl NoCoverage<'_> {
    /// The sentence the viewport draws.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            NoCoverage::NoSensorsActive => {
                "No sensor is searching or tracking, so nothing is covered.".to_owned()
            }
            NoCoverage::NoOrigin { setting } => format!(
                "Coverage cannot be placed: this deployment has declared no local frame \
                 origin ({setting}). The rings are not missing, they are unplaceable."
            ),
        }
    }
}

/// A stretch of an approach the picture does not cover, ready to draw (DN-12 §7).
///
/// Samples rather than endpoints, because DN-12 §3 says so and the reason is that an
/// approach is a polyline: a gap that bent round a corner would be drawn as a straight
/// line through ground that is actually covered.
#[derive(Debug, Clone, PartialEq)]
pub struct GapPolyline<'a> {
    /// What the approach is called, so a gap names somewhere rather than an index.
    pub approach: &'a str,
    /// Sample positions along the gap, in local ENU metres.
    pub samples: &'a [[f64; 3]],
    /// Covered by nothing, as opposed to covered by one sensor.
    pub uncovered: bool,
}

/// The coverage layer for a frame: circles, or the reason there are none.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CoverageLayer<'a> {
    Circles {
        circles: &'a [CoverageCircle],
        /// Set when these are the sensors' *configured* ranges rather than coverage
        /// observed from their modes, naming the entry that would make them observed.
        ///
        /// The difference is operationally real: a nominal ring says what a sensor
        /// could cover if it were searching, and an observed one says what it is
        /// covering now. A standby sensor has a nominal ring and no observed coverage
        /// at all, and a map that showed the two alike would tell a sensor manager the
        /// sector is watched when it is not.
        nominal: Option<&'a str>,
    },
    None(NoCoverage<'a>),
}

/// Draw the coverage layer under the track glyphs.
///
/// Under, deliberately: coverage is context and a track is the thing being looked at, so
/// a ring must never sit on top of a symbol. The caller draws this before the glyphs.
pub fn draw_coverage_2d(
    painter: &egui::Painter,
    rect: egui::Rect,
    view: &TopDownView,
    layer: CoverageLayer<'_>,
) {
    draw_rings(painter, rect, view, layer);
}

/// Gaps along the declared approaches (DN-12 §7).
///
/// Drawn after the rings and before the glyphs: a gap is the absence the rings are
/// context for, and it still must not sit on top of a track.
///
/// **Uncovered and single-sensor differ in pattern as well as colour**, which DN-12 §7
/// asks for and `docs/ux/accessibility.md` requires generally: an operator who cannot
/// separate the two colours still has to see that one stretch is watched by nobody and
/// the other by one sensor, because those call for different actions.
pub fn draw_gaps_2d(
    painter: &egui::Painter,
    rect: egui::Rect,
    view: &TopDownView,
    gaps: &[GapPolyline<'_>],
) {
    for gap in gaps {
        let points: Vec<egui::Pos2> = gap.samples.iter().map(|p| view.project(*p, rect)).collect();
        if points.len() < 2 {
            continue;
        }
        let stroke = egui::Stroke::new(
            if gap.uncovered {
                theme::STROKE_GAP_UNCOVERED
            } else {
                theme::STROKE_GAP_PARTIAL
            },
            if gap.uncovered {
                theme::CLASS_HOSTILE_COLOR
            } else {
                theme::WARNING_COLOR
            },
        );
        if gap.uncovered {
            painter.add(egui::Shape::line(points, stroke));
        } else {
            // Single-sensor is drawn as a dashed line: the same stretch is watched, but
            // by one sensor, and the broken line is the second cue.
            for pair in points.chunks(2) {
                if let [a, b] = pair {
                    painter.line_segment([*a, *b], stroke);
                }
            }
        }
    }
}

/// Everything the map draws under the glyphs, already placed by the caller: coverage,
/// gaps, hazards and geofences. One argument rather than four, because the two renderers
/// take the same set and must not be able to disagree about it.
#[derive(Debug, Clone, Copy)]
pub struct LayerInputs<'a> {
    pub coverage: CoverageLayer<'a>,
    pub gaps: &'a [GapPolyline<'a>],
    pub hazards: &'a [HazardOutline<'a>],
    pub geofences: &'a [GeofenceOutline<'a>],
    /// Predicted track lines (GAP-020, DN-02 §7), drawn dashed so they are never read
    /// as observed history.
    pub predictions: &'a [PredictedPath<'a>],
    /// The terrain surface, when a DEM is placed (GAP-023). `None` draws no ground and
    /// claims none.
    pub terrain: Option<TerrainLayer<'a>>,
}

/// The placed terrain as the viewport draws it: the grid's vertices in local ENU metres.
///
/// Borrowed from `gungnir_data::geospatial::TerrainMesh` by the caller rather than that
/// type itself, so the drawing does not depend on the loader's shape; `rows` and
/// `columns` let the drawing decimate a large grid by stride, which keeps the surface
/// whole (a decimated grid has no holes) where skipping triangles would not.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerrainLayer<'a> {
    /// `rows * columns` vertices, north row first; a `NaN` height is a no-data cell.
    pub positions: &'a [[f32; 3]],
    pub rows: u32,
    pub columns: u32,
}

/// The most vertices a frame shades. A 1 000 by 1 000 DEM has a million; the picture
/// cannot show that many and egui should not be asked to.
pub const TERRAIN_VERTEX_BUDGET: usize = 16_384;

/// Stride that brings a grid under the vertex budget.
#[must_use]
pub fn terrain_stride(rows: u32, columns: u32) -> u32 {
    let vertices = (rows as usize).saturating_mul(columns as usize);
    if vertices <= TERRAIN_VERTEX_BUDGET {
        return 1;
    }
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let stride = (vertices as f64 / TERRAIN_VERTEX_BUDGET as f64)
        .sqrt()
        .ceil() as u32;
    stride.max(1)
}

/// The colour of a height between the surface's lowest and highest: dark green low,
/// pale brown high, translucent so tracks and rings stay legible over it.
#[must_use]
pub fn terrain_color(t: f32) -> egui::Color32 {
    let t = if t.is_finite() {
        t.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let low = [46.0_f32, 92.0, 52.0];
    let high = [176.0_f32, 150.0, 110.0];
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let mix = |i: usize| (low[i] + (high[i] - low[i]) * t) as u8;
    egui::Color32::from_rgba_unmultiplied(mix(0), mix(1), mix(2), theme::TERRAIN_ALPHA)
}

/// Draw the terrain as a shaded surface in the top-down view. Nothing is drawn for
/// `None`, and nothing is written either: the health panel is where the absence of
/// terrain is said (PN-09), and a label on every empty map would be noise.
pub fn draw_terrain_2d(
    painter: &egui::Painter,
    rect: egui::Rect,
    view: &TopDownView,
    terrain: Option<TerrainLayer<'_>>,
) {
    let Some(terrain) = terrain else { return };
    let (rows, columns) = (terrain.rows, terrain.columns);
    if rows < 2 || columns < 2 || terrain.positions.len() != (rows as usize) * (columns as usize) {
        return;
    }
    let (lo, hi) = terrain
        .positions
        .iter()
        .map(|p| p[2])
        .filter(|z| z.is_finite())
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), z| {
            (lo.min(z), hi.max(z))
        });
    if !lo.is_finite() {
        return;
    }
    let span = if hi > lo { hi - lo } else { 1.0 };
    let stride = terrain_stride(rows, columns);
    let sampled_rows: Vec<u32> = (0..rows).step_by(stride as usize).collect();
    let sampled_columns: Vec<u32> = (0..columns).step_by(stride as usize).collect();
    let mut mesh = egui::Mesh::default();
    // Vertex (i, j) of the decimated grid is at index i * sampled_columns.len() + j.
    let mut ids: Vec<Option<u32>> = Vec::with_capacity(sampled_rows.len() * sampled_columns.len());
    for &r in &sampled_rows {
        for &c in &sampled_columns {
            let p = terrain.positions[(r as usize) * (columns as usize) + c as usize];
            if !p[2].is_finite() {
                ids.push(None);
                continue;
            }
            let pos = view.project([f64::from(p[0]), f64::from(p[1]), 0.0], rect);
            #[allow(clippy::cast_possible_truncation)]
            let id = mesh.vertices.len() as u32;
            mesh.colored_vertex(pos, terrain_color((p[2] - lo) / span));
            ids.push(Some(id));
        }
    }
    let width = sampled_columns.len();
    for i in 0..sampled_rows.len().saturating_sub(1) {
        for j in 0..width.saturating_sub(1) {
            let (nw, ne, sw, se) = (
                ids[i * width + j],
                ids[i * width + j + 1],
                ids[(i + 1) * width + j],
                ids[(i + 1) * width + j + 1],
            );
            if let (Some(nw), Some(ne), Some(sw), Some(se)) = (nw, ne, sw, se) {
                mesh.add_triangle(nw, sw, ne);
                mesh.add_triangle(ne, sw, se);
            }
        }
    }
    if !mesh.indices.is_empty() {
        painter.add(egui::Shape::mesh(mesh));
    }
}

/// One track's predicted line in the local ENU frame (GAP-020).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PredictedPath<'a> {
    pub track: u64,
    /// Positions at the requested horizons, nearest first.
    pub points: &'a [[f64; 3]],
    /// False when the uncertainty is not drawable (DN-02 §5 rule 3): the line is drawn,
    /// the end marker is not.
    pub uncertainty_drawable: bool,
}

/// Draw the predicted lines: dashed, in the warning colour, with a hollow end marker
/// where the uncertainty could be drawn. Observed history is solid; the difference is
/// the point (DN-02 §7).
pub fn draw_predictions_2d(
    painter: &egui::Painter,
    rect: egui::Rect,
    view: &TopDownView,
    predictions: &[PredictedPath<'_>],
) {
    for p in predictions {
        let points: Vec<egui::Pos2> = p.points.iter().map(|q| view.project(*q, rect)).collect();
        if points.len() < 2 {
            continue;
        }
        let stroke = egui::Stroke::new(theme::STROKE_EMPHASIS, theme::WARNING_COLOR);
        for pair in points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            // Dashes: split each segment into thirds and draw the outer two.
            let d = b - a;
            painter.line_segment([a, a + d / 3.0], stroke);
            painter.line_segment([a + d * (2.0 / 3.0), b], stroke);
        }
        if p.uncertainty_drawable {
            if let Some(end) = points.last() {
                painter.circle_stroke(*end, 4.0, stroke);
            }
        }
    }
}

/// One geofence placed in the local ENU frame (GAP-088). A rule, not a survey fact:
/// drawn under everything else as a dashed circle, red when it denies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeofenceOutline<'a> {
    pub name: &'a str,
    pub center_enu: [f64; 3],
    pub radius_m: f64,
    pub no_go: bool,
}

/// Draw the geofences in the top-down projection: dashed circles, labelled, red for a
/// no-go fence and muted for one that marks without denying.
pub fn draw_geofences_2d(
    painter: &egui::Painter,
    rect: egui::Rect,
    view: &TopDownView,
    fences: &[GeofenceOutline<'_>],
) {
    const SEGMENTS: usize = 64;
    for fence in fences {
        let colour = if fence.no_go {
            theme::ALERT_COLOR
        } else {
            theme::MUTED_TEXT_COLOR
        };
        let stroke = egui::Stroke::new(theme::STROKE_EMPHASIS, colour);
        let points: Vec<egui::Pos2> = (0..=SEGMENTS)
            .map(|i| {
                #[allow(clippy::cast_precision_loss)]
                let theta = std::f64::consts::TAU * (i % SEGMENTS) as f64 / SEGMENTS as f64;
                view.project(
                    [
                        fence.center_enu[0] + fence.radius_m * theta.cos(),
                        fence.center_enu[1] + fence.radius_m * theta.sin(),
                        fence.center_enu[2],
                    ],
                    rect,
                )
            })
            .collect();
        for pair in points.chunks(2) {
            if let [a, b] = pair {
                painter.line_segment([*a, *b], stroke);
            }
        }
        painter.text(
            view.project(fence.center_enu, rect),
            egui::Align2::CENTER_CENTER,
            if fence.no_go {
                format!("{} (no-go)", fence.name)
            } else {
                fence.name.to_string()
            },
            egui::FontId::proportional(theme::SMALL_FONT_SIZE),
            colour,
        );
    }
}

/// One hazard, already placed in the local ENU frame (DN-14, GAP-017).
///
/// A circle arrives as a sampled ring; the drawing code does not know or care which shape
/// the baseline declared, only where the outline runs and whether it stops a surface craft.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HazardOutline<'a> {
    pub name: &'a str,
    /// The kind, in words, for the label: "boom", "net", ...
    pub kind: &'a str,
    /// ENU metres, in order along the outline. A ring repeats its first point last.
    pub samples: &'a [[f64; 3]],
    /// Drawn solid when true, dotted when not: a shoal a jet ski crosses is a different
    /// obstacle from a boom, and the line says which.
    pub blocks_surface: bool,
}

/// Draw the hazard layer in the top-down projection.
///
/// **Distinct from geofences and from gaps**, because a hazard means something different
/// from both: not a rule about where we may act, not a hole in the coverage, but a thing
/// that is there. Its own colour, and a label, since an unlabelled line across a harbour
/// mouth is a puzzle rather than a boom.
pub fn draw_hazards_2d(
    painter: &egui::Painter,
    rect: egui::Rect,
    view: &TopDownView,
    hazards: &[HazardOutline<'_>],
) {
    for hazard in hazards {
        let points: Vec<egui::Pos2> = hazard
            .samples
            .iter()
            .map(|p| view.project(*p, rect))
            .collect();
        if points.len() < 2 {
            continue;
        }
        let stroke = egui::Stroke::new(theme::STROKE_HAZARD, theme::HAZARD_COLOR);
        if hazard.blocks_surface {
            painter.add(egui::Shape::line(points.clone(), stroke));
        } else {
            for pair in points.chunks(2) {
                if let [a, b] = pair {
                    painter.line_segment([*a, *b], stroke);
                }
            }
        }
        // The label sits at the outline's first point; for a ring that is its eastmost
        // sample, for a boom one end.
        painter.text(
            points[0] + egui::vec2(theme::PANEL_SPACING, -theme::PANEL_SPACING),
            egui::Align2::LEFT_BOTTOM,
            format!("{} ({})", hazard.name, hazard.kind),
            egui::FontId::proportional(theme::SMALL_FONT_SIZE),
            theme::HAZARD_COLOR,
        );
    }
}

fn draw_rings(
    painter: &egui::Painter,
    rect: egui::Rect,
    view: &TopDownView,
    layer: CoverageLayer<'_>,
) {
    let (circles, nominal) = match layer {
        CoverageLayer::Circles { circles, nominal } => (circles, nominal),
        CoverageLayer::None(reason) => {
            painter.text(
                rect.left_bottom() + egui::vec2(theme::PANEL_SPACING, -theme::PANEL_SPACING),
                egui::Align2::LEFT_BOTTOM,
                reason.sentence(),
                egui::FontId::proportional(theme::SMALL_FONT_SIZE),
                theme::MUTED_TEXT_COLOR,
            );
            return;
        }
    };

    if let Some(gap) = nominal {
        painter.text(
            rect.left_bottom() + egui::vec2(theme::PANEL_SPACING, -theme::PANEL_SPACING),
            egui::Align2::LEFT_BOTTOM,
            format!(
                "Coverage shown is each sensor's configured range, not what it is covering now: sensor modes are not read ({gap})."
            ),
            egui::FontId::proportional(theme::SMALL_FONT_SIZE),
            theme::WARNING_COLOR,
        );
    }

    for circle in circles {
        let center = view.project(circle.center, rect);
        let radius_px = radius_in_pixels(circle.radius_m, view);
        // A ring smaller than a couple of pixels is not coverage an operator can read;
        // drawing it anyway leaves specks that look like tracks.
        if radius_px < 2.0 {
            continue;
        }
        painter.circle_stroke(
            center,
            radius_px,
            egui::Stroke::new(theme::STROKE_HAIRLINE, coverage_color(circle.confidence)),
        );
    }
}

/// Coverage radius in screen pixels for the current zoom.
#[allow(clippy::cast_possible_truncation)]
#[must_use]
pub fn radius_in_pixels(radius_m: f64, view: &TopDownView) -> f32 {
    (radius_m / view.meters_per_px.max(f64::EPSILON)) as f32
}

/// Colour for a coverage ring: the theme's coverage colour, faded by confidence.
///
/// Confidence is drawn rather than labelled because it is a property of the whole ring,
/// and a searching sensor's ring should look like a weaker claim than a tracking one's
/// without an operator having to read a legend.
#[must_use]
pub fn coverage_color(confidence: f32) -> egui::Color32 {
    let base = theme::COVERAGE_COLOR;
    // `clamp` on a NaN returns NaN, and `NaN as u8` is 0 -- nothing drawn, which is the
    // right answer for a confidence nobody can read.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let alpha = (f32::from(theme::COVERAGE_MAX_ALPHA) * confidence.clamp(0.0, 1.0)) as u8;
    egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small grid is drawn whole; a large one is decimated to the budget, never
    /// skipped triangle by triangle.
    #[test]
    fn terrain_is_decimated_by_stride_to_the_vertex_budget() {
        assert_eq!(terrain_stride(100, 100), 1);
        let stride = terrain_stride(1000, 1000);
        assert!(stride >= 8, "{stride}");
        let kept = (1000 / stride as usize + 1).pow(2);
        assert!(
            kept <= TERRAIN_VERTEX_BUDGET + 2 * (1000 / stride as usize + 1),
            "{kept}"
        );
        assert_eq!(terrain_stride(0, 0), 1);
    }

    #[test]
    fn terrain_colour_runs_low_to_high_and_tolerates_nan() {
        let low = terrain_color(0.0);
        let high = terrain_color(1.0);
        assert!(high.r() > low.r(), "higher ground is paler");
        assert_eq!(terrain_color(f32::NAN), low);
        assert!(low.a() < 255, "translucent, so tracks stay legible over it");
    }

    fn circle(sensor: u32, radius_m: f64, confidence: f32) -> CoverageCircle {
        CoverageCircle {
            sensor,
            center: [0.0, 0.0, 0.0],
            radius_m,
            confidence,
        }
    }

    /// The two reasons for an empty coverage layer are different situations and must not
    /// look alike: one says the sector is uncovered, the other says the map cannot place
    /// what covers it. A sensor manager would act differently on each.
    #[test]
    fn an_unplaceable_layer_is_not_an_uncovered_one() {
        let unplaceable = NoCoverage::NoOrigin { setting: "origin" };
        assert_ne!(unplaceable, NoCoverage::NoSensorsActive);
        assert!(
            unplaceable.sentence().contains("unplaceable"),
            "the sentence must say the rings exist and cannot be drawn: {}",
            unplaceable.sentence()
        );
        assert!(
            NoCoverage::NoSensorsActive
                .sentence()
                .contains("nothing is covered"),
            "the other sentence must say the opposite"
        );
    }

    /// A weaker claim is drawn more faintly, and a full-confidence ring is the theme's
    /// own colour. A searching sensor's ring that looked identical to a tracking one's
    /// would overstate what is known.
    #[test]
    fn lower_confidence_is_drawn_more_faintly() {
        let strong = coverage_color(1.0);
        let weak = coverage_color(0.7);
        let none = coverage_color(0.0);
        assert_eq!(strong.a(), theme::COVERAGE_MAX_ALPHA);
        assert!(weak.a() < strong.a());
        assert_eq!(none.a(), 0);
        // `Color32` stores premultiplied, so the way to compare hues is egui's own
        // inverse rather than dividing by alpha by hand -- integer rounding at low
        // alpha makes that drift, which the first version of this assertion did.
        //
        // This is the bug the test caught: reading a premultiplied base colour and
        // re-fading its components shifted the hue as well as the opacity, so a
        // searching sensor's ring was a different colour rather than a fainter one.
        let strong_rgb = &strong.to_srgba_unmultiplied()[..3];
        let weak_rgb = &weak.to_srgba_unmultiplied()[..3];
        for (s, w) in strong_rgb.iter().zip(weak_rgb) {
            assert!(
                s.abs_diff(*w) <= 1,
                "confidence changed the hue, not only the opacity: {strong_rgb:?} vs {weak_rgb:?}"
            );
        }
    }

    /// Confidence outside 0..=1 cannot produce a brighter-than-full or negative ring:
    /// the value comes from another crate and a bad one must not corrupt the drawing.
    #[test]
    fn confidence_outside_the_range_is_clamped() {
        assert_eq!(coverage_color(5.0).a(), theme::COVERAGE_MAX_ALPHA);
        assert_eq!(coverage_color(-1.0).a(), 0);
        assert_eq!(
            coverage_color(f32::NAN).a(),
            0,
            "NaN clamps to nothing drawn"
        );
    }

    /// The ring scales with the zoom, so it stays the same ground however far out the
    /// operator is.
    #[test]
    fn the_radius_follows_the_zoom() {
        let close = TopDownView {
            meters_per_px: 10.0,
            ..TopDownView::default()
        };
        assert!((radius_in_pixels(50_000.0, &close) - 5_000.0).abs() < 1e-3);
        let wide = TopDownView {
            meters_per_px: 100.0,
            ..TopDownView::default()
        };
        assert!((radius_in_pixels(50_000.0, &wide) - 500.0).abs() < 1e-3);
        // A collapsed zoom must not divide by zero.
        let collapsed = TopDownView {
            meters_per_px: 0.0,
            ..TopDownView::default()
        };
        assert!(radius_in_pixels(50_000.0, &collapsed).is_finite());
    }

    /// A nominal layer and an observed one are different claims about the sector, and
    /// the type keeps them apart: a standby sensor has a configured range and covers
    /// nothing.
    #[test]
    fn a_nominal_layer_is_not_an_observed_one() {
        let circles = [circle(1, 50_000.0, 1.0)];
        let observed = CoverageLayer::Circles {
            circles: &circles,
            nominal: None,
        };
        let nominal = CoverageLayer::Circles {
            circles: &circles,
            nominal: Some("GAP-003"),
        };
        assert_ne!(observed, nominal);
    }

    /// A circle too small to read is skipped rather than drawn as a speck that could be
    /// mistaken for a track.
    #[test]
    fn rings_below_a_readable_size_are_skipped() {
        let view = TopDownView {
            meters_per_px: 10_000.0,
            ..TopDownView::default()
        };
        assert!(radius_in_pixels(circle(1, 5_000.0, 1.0).radius_m, &view) < 2.0);
        assert!(radius_in_pixels(circle(2, 50_000.0, 1.0).radius_m, &view) >= 2.0);
    }
}

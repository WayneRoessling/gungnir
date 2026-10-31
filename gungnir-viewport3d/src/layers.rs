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
    /// The bearings the sensor sees, **in this frame** -- the caller has already turned a
    /// sector surveyed on true north into the frame (GAP-118, D-84). `None` is the full
    /// circle and is drawn as a ring; a sector is drawn as a wedge, so a panel radar's
    /// blind side is not painted as watched.
    pub sector: Option<gungnir_model::AzimuthSector>,
}

/// The outline of a sectored coverage region in ENU, ready to project: the sensor, the arc
/// from the sector's anticlockwise edge clockwise to its clockwise edge, and back to the
/// sensor. `None` for a full circle, which is drawn as a ring instead.
///
/// `segments` is the number of straight pieces the arc is drawn with at a full circle; a
/// narrower sector gets proportionally fewer, never fewer than two.
#[must_use]
pub fn sector_outline(circle: &CoverageCircle, segments: u32) -> Option<Vec<[f64; 3]>> {
    sector_outline_points(circle, segments).map(Iterator::collect)
}

/// [`sector_outline`]'s points in order, produced as they are read, so the frame projects
/// them straight into the one vector egui's line shape owns rather than collecting them
/// twice (GAP-257).
fn sector_outline_points(
    circle: &CoverageCircle,
    segments: u32,
) -> Option<impl Iterator<Item = [f64; 3]>> {
    let sector = circle.sector?;
    if sector.is_full_circle() || sector.validate().is_err() {
        return None;
    }
    let tau = std::f64::consts::TAU;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let pieces = ((f64::from(segments) * sector.width_rad / tau).ceil() as u32).max(2);
    let start = sector.start_rad();
    let width = sector.width_rad;
    let center = circle.center;
    let [e, n, u] = center;
    let radius = circle.radius_m;
    let arc = (0..=pieces).map(move |i| {
        let bearing = start + width * f64::from(i) / f64::from(pieces);
        [e + radius * bearing.sin(), n + radius * bearing.cos(), u]
    });
    Some(
        std::iter::once(center)
            .chain(arc)
            .chain(std::iter::once(center)),
    )
}

/// Arc pieces for a full circle's worth of a sector outline on screen.
const SECTOR_SEGMENTS: u32 = 96;

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
    /// A commander accepted this gap and the acceptance stands (GAP-106,
    /// `docs/design/DN-33-accepting-a-coverage-gap.md` §8 rule 4). **Drawn as the gap it
    /// is**, in its own colour and pattern, and marked "accepted" beside it: an accepted
    /// gap is a risk somebody took, not a stretch that is covered.
    pub accepted: bool,
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
/// `note` is the layer's sentence as [`CoverageNoteKey::write`] wrote it for this layer:
/// why there are no rings, or that the rings are nominal; drawn only when the layer calls
/// for one.
pub fn draw_coverage_2d(
    painter: &egui::Painter,
    palette: &theme::Palette,
    rect: egui::Rect,
    view: &TopDownView,
    layer: CoverageLayer<'_>,
    note: &str,
) {
    draw_rings(painter, palette, rect, view, layer, note);
}

/// What the coverage layer's note states (GAP-257): nothing, why there are no rings, or
/// that the rings are nominal and why. The note was formatted on every frame; it is written
/// when this key stops matching the layer, which is when the baseline or the sensors change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverageNoteKey {
    /// Observed rings: no note.
    Nothing,
    NoSensorsActive,
    NoOrigin(String),
    Nominal(String),
}

impl CoverageNoteKey {
    /// Whether a note written from this key is the note `layer` calls for.
    #[must_use]
    pub fn matches(&self, layer: &CoverageLayer<'_>) -> bool {
        match (self, layer) {
            (Self::Nothing, CoverageLayer::Circles { nominal: None, .. })
            | (Self::NoSensorsActive, CoverageLayer::None(NoCoverage::NoSensorsActive)) => true,
            (
                Self::Nominal(gap),
                CoverageLayer::Circles {
                    nominal: Some(g), ..
                },
            ) => gap == g,
            (Self::NoOrigin(setting), CoverageLayer::None(NoCoverage::NoOrigin { setting: s })) => {
                setting == s
            }
            _ => false,
        }
    }

    /// Write `layer`'s note into `text`, and return the key it was written from.
    pub fn write(layer: &CoverageLayer<'_>, text: &mut String) -> Self {
        use std::fmt::Write as _;
        match layer {
            CoverageLayer::Circles { nominal: None, .. } => Self::Nothing,
            CoverageLayer::Circles {
                nominal: Some(gap), ..
            } => {
                let _ = write!(
                    text,
                    "Coverage shown is each sensor's configured range, not what it is covering now: sensor modes are not read ({gap})."
                );
                Self::Nominal((*gap).to_owned())
            }
            CoverageLayer::None(reason) => {
                text.push_str(&reason.sentence());
                match reason {
                    NoCoverage::NoSensorsActive => Self::NoSensorsActive,
                    NoCoverage::NoOrigin { setting } => Self::NoOrigin((*setting).to_owned()),
                }
            }
        }
    }
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
    palette: &theme::Palette,
    rect: egui::Rect,
    view: &TopDownView,
    gaps: &[GapPolyline<'_>],
) {
    for gap in gaps {
        if gap.samples.len() < 2 {
            continue;
        }
        let stroke = egui::Stroke::new(
            if gap.uncovered {
                palette.stroke_gap_uncovered
            } else {
                palette.stroke_gap_partial
            },
            if gap.uncovered {
                palette.class_hostile_color
            } else {
                palette.warning_color
            },
        );
        // GAP-106: the mark goes beside the gap's middle sample, in the muted colour, so it
        // labels the gap without covering it.
        if gap.accepted {
            painter.text(
                view.project(gap.samples[gap.samples.len() / 2], rect) + egui::vec2(6.0, -6.0),
                egui::Align2::LEFT_BOTTOM,
                "accepted",
                egui::FontId::proportional(palette.small_font_size),
                palette.muted_text_color(),
            );
        }
        if gap.uncovered {
            // egui owns a line's points, so this is the one vector the layer allocates.
            painter.add(egui::Shape::line(
                gap.samples.iter().map(|p| view.project(*p, rect)).collect(),
                stroke,
            ));
        } else {
            // Single-sensor is drawn as a dashed line: the same stretch is watched, but
            // by one sensor, and the broken line is the second cue. Each dash is projected
            // as it is drawn (GAP-257).
            for pair in gap.samples.chunks(2) {
                if let [a, b] = pair {
                    painter.line_segment([view.project(*a, rect), view.project(*b, rect)], stroke);
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
    /// A staged terrain store, once its tiles' footprint is placed (GAP-227, DN-36 §5.1):
    /// drawn by view in place of `terrain`, which a baseline naming a store never has.
    pub terrain_tiles: Option<crate::streaming::terrain::TerrainStoreLayer<'a>>,
    /// The configured point-cloud pair, once both files have loaded (GAP-098). Empty
    /// until then -- a lone cloud is not the pair registration needs, so drawing one
    /// while the other is still loading or failed would show a claim nothing backs.
    pub point_clouds: &'a [PointCloudLayer<'a>],
    /// The laydown option PN-16 has selected for a before-and-after preview (GAP-087),
    /// or `None` when nothing is selected.
    pub laydown_preview: Option<LaydownPreview<'a>>,
    /// The dense group the tracker could not resolve into tracks (GAP-015), or `None`
    /// for a resolvable scene. Drawn as a group, never as track symbols.
    pub dense_group: Option<&'a gungnir_model::DenseGroupView>,
    /// Where the terrain blinds the sensors (GAP-246), or why that is not known.
    pub dead_spots: DeadSpotLayer<'a>,
}

/// One cell of a dead-spot field as the viewport draws it (GAP-246,
/// `docs/design/DN-34-sensor-dead-spots.md` §4 and §8.1): what the analytics found, folded
/// to what the map shows. Its own type rather than `gungnir_analytics::CellSight`, because
/// this crate does not reach the analytics and draws what it is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeadSpotCell {
    /// Nothing drawn: no sensor claims it, or two or more see it.
    Clear,
    /// Claimed by a sensor and hidden from every one that claims it: **the dead spot**.
    Shadow,
    /// Seen by one sensor alone.
    Single,
    /// Claimed, over ground the DEM has no height for: unknown, which is not clear.
    NoData,
    /// A what-if (GAP-249, D-150): a dead spot now that the change would clear. Drawn as
    /// the dead spot it is, with a hatch over it, because it is still dark until somebody
    /// makes the change.
    WouldClear,
    /// A what-if: not a dead spot now, and one with the change.
    WouldDarken,
}

/// A computed dead-spot field, ready to draw (GAP-246).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeadSpotField<'a> {
    /// Changes whenever the cells do, so the texture is rebuilt once per field, never per
    /// frame (`docs/rust-ui-architecture-coding-standards.md` §5).
    pub revision: u64,
    /// ENU east and north of the field's south-west corner, metres.
    pub origin_enu: [f64; 2],
    pub cell_m: f64,
    pub rows: u32,
    pub columns: u32,
    /// `rows * columns` cells, north row first.
    pub cells: &'a [DeadSpotCell],
    /// The probe's height above the ground, metres: the field says what it was computed
    /// for, because a shadow at 100 m is not one at 3 000 m.
    pub height_agl_m: f64,
    /// Seconds since the field was computed.
    pub age_s: f64,
    /// The inputs have changed since, and a newer field is being computed. Drawn faded and
    /// said, never torn down: a stale shadow that says so is a better claim than none for
    /// the seconds a recompute takes (DN-34 §6.5).
    pub stale: bool,
    /// One sensor's own shadow, named, rather than every active sensor's.
    pub sensor: Option<&'a str>,
    /// The desktop is linked: the field is this desktop's own, from the node's sensors and
    /// this desktop's DEM, since the node computes none (D-148).
    pub linked: bool,
    /// A what-if is shown, described in words (GAP-249, D-150): the cells are the live
    /// dead spots with what the change would clear and darken marked over them.
    pub what_if: Option<&'a str>,
}

/// The dead-spot layer for a frame: a field, or why there is none (DN-34 §4 rule 4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeadSpotLayer<'a> {
    /// Not computed, and why, in words. **The dead spots are not absent, they are
    /// unknown**, and the map says so rather than drawing a clear sector.
    NotComputed {
        reason: &'a str,
    },
    /// The first field is being computed.
    Computing,
    Field(DeadSpotField<'a>),
}

/// What the map says under a dead-spot field: what it is, the height, its age, the key to
/// its patterns, and that it is geometric line of sight.
#[must_use]
pub fn dead_spot_line(field: &DeadSpotField<'_>) -> String {
    if let Some(what_if) = field.what_if {
        let mut line = format!(
            "WHAT-IF, not the live picture: {what_if}. Shown over the live dead spots at {:.0} m \
             above ground: green hatching would clear, red cross-hatching would go dark. \
             Computed {:.0} s ago, geometric line of sight",
            field.height_agl_m, field.age_s
        );
        if field.stale {
            line.push_str("; recomputing, the inputs have changed");
        }
        return line;
    }
    let whose = field
        .sensor
        .map_or_else(String::new, |s| format!(" for {s} alone"));
    let mut line = format!(
        "Terrain dead spots{whose} at {:.0} m above ground (dark: hidden by terrain; \
         hatched: one sensor; dotted: no terrain data), computed {:.0} s ago, geometric \
         line of sight",
        field.height_agl_m, field.age_s
    );
    if field.linked {
        line.push_str("; this desktop's own, from the node's sensors");
    }
    if field.stale {
        line.push_str("; recomputing, the inputs have changed");
    }
    line
}

/// What the map says when there is no field.
#[must_use]
pub fn dead_spot_not_computed_line(reason: &str) -> String {
    format!("Terrain dead spots not computed: {reason}. They are not absent, they are unknown.")
}

/// What the dead-spot layer's line states (GAP-257): the facts [`dead_spot_line`] and
/// [`dead_spot_not_computed_line`] print, so the line is written when one of them changes
/// and not on every frame. The age is kept as the line prints it, in whole seconds, so the
/// line is rewritten once a second while the field ages and not thirty times.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeadSpotLineKey {
    NotComputed(String),
    Computing,
    Field {
        revision: u64,
        /// `age_s` rounded as `{:.0}` rounds it, as bits.
        age: u64,
        stale: bool,
        linked: bool,
        height_agl_m: u64,
        sensor: Option<String>,
        what_if: Option<String>,
    },
}

/// The age as the line prints it: whole seconds, ties to even as `{:.0}` rounds.
fn shown_age(age_s: f64) -> u64 {
    age_s.round_ties_even().to_bits()
}

impl DeadSpotLineKey {
    /// Whether a line written from this key is the line `layer` calls for.
    #[must_use]
    pub fn matches(&self, layer: &DeadSpotLayer<'_>) -> bool {
        match (self, layer) {
            (Self::NotComputed(a), DeadSpotLayer::NotComputed { reason }) => a == reason,
            (Self::Computing, DeadSpotLayer::Computing) => true,
            (
                Self::Field {
                    revision,
                    age,
                    stale,
                    linked,
                    height_agl_m,
                    sensor,
                    what_if,
                },
                DeadSpotLayer::Field(f),
            ) => {
                *revision == f.revision
                    && *age == shown_age(f.age_s)
                    && *stale == f.stale
                    && *linked == f.linked
                    && *height_agl_m == f.height_agl_m.to_bits()
                    && sensor.as_deref() == f.sensor
                    && what_if.as_deref() == f.what_if
            }
            _ => false,
        }
    }

    /// Write `layer`'s line into `text`, and return the key it was written from.
    pub fn write(layer: &DeadSpotLayer<'_>, text: &mut String) -> Self {
        match layer {
            DeadSpotLayer::NotComputed { reason } => {
                text.push_str(&dead_spot_not_computed_line(reason));
                Self::NotComputed((*reason).to_owned())
            }
            DeadSpotLayer::Computing => {
                text.push_str("Terrain dead spots: computing the first field.");
                Self::Computing
            }
            DeadSpotLayer::Field(f) => {
                text.push_str(&dead_spot_line(f));
                Self::Field {
                    revision: f.revision,
                    age: shown_age(f.age_s),
                    stale: f.stale,
                    linked: f.linked,
                    height_agl_m: f.height_agl_m.to_bits(),
                    sensor: f.sensor.map(str::to_owned),
                    what_if: f.what_if.map(str::to_owned),
                }
            }
        }
    }
}

/// The pattern period, in cells, that keeps a hatch about this many pixels apart on screen.
const DEAD_SPOT_PATTERN_PX: f64 = 8.0;

/// The hatch and dot period for a field drawn at `meters_per_px`, in cells, rounded up to
/// a power of two so the texture is rebuilt only when the zoom crosses a doubling.
#[must_use]
// A period of at most 64 cells.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn dead_spot_pattern_cells(cell_m: f64, meters_per_px: f64) -> u32 {
    let px_per_cell = cell_m / meters_per_px.max(f64::EPSILON);
    if !px_per_cell.is_finite() || px_per_cell <= 0.0 {
        return 64;
    }
    let cells = (DEAD_SPOT_PATTERN_PX / px_per_cell).ceil().clamp(2.0, 64.0) as u32;
    cells.next_power_of_two()
}

/// A dead-spot field as an image, one pixel per `stride` cells, the meaning carried in
/// pattern as well as colour (DN-34 §8.1, `docs/ux/accessibility.md`): a dead spot is a
/// solid dark fill, single-sensor a diagonal hatch in the warning colour, no data a dot
/// grid in the muted colour over a faint wash, and clear cells transparent so the terrain
/// and the rings stay legible. `period` is the pattern's repeat in image pixels.
#[must_use]
pub fn dead_spot_image(
    field: &DeadSpotField<'_>,
    palette: &theme::Palette,
    period: u32,
    stride: u32,
) -> egui::ColorImage {
    let stride = stride.max(1) as usize;
    let period = period.max(2) as usize;
    let (rows, columns) = (field.rows as usize, field.columns as usize);
    let (width, height) = (columns.div_ceil(stride), rows.div_ceil(stride));
    let with_alpha =
        |c: egui::Color32, a: u8| egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a);
    let shadow = with_alpha(palette.shadow_color, palette.shadow_alpha);
    let single = with_alpha(palette.warning_color, 160);
    let dot = with_alpha(palette.muted_text_color(), 220);
    let wash = with_alpha(palette.muted_text_color(), 36);
    let clear = with_alpha(palette.healthy_color(), 230);
    let darken = with_alpha(palette.alert_color, 230);
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let cell = field
                .cells
                .get(y * stride * columns + x * stride)
                .copied()
                .unwrap_or(DeadSpotCell::Clear);
            pixels.push(match cell {
                DeadSpotCell::Clear => egui::Color32::TRANSPARENT,
                DeadSpotCell::Shadow => shadow,
                DeadSpotCell::Single => {
                    if (x + y) % period < period / 2 {
                        single
                    } else {
                        egui::Color32::TRANSPARENT
                    }
                }
                DeadSpotCell::NoData => {
                    if x % period == period / 2 && y % period == period / 2 {
                        dot
                    } else {
                        wash
                    }
                }
                // The dead spot's own fill, with a hatch running the other way from
                // single-sensor's, in the healthy colour.
                DeadSpotCell::WouldClear => {
                    if (x + period - y % period) % period < period / 2 {
                        clear
                    } else {
                        shadow
                    }
                }
                // A cross-hatch in the alert colour over nothing: ground that is not dark
                // now, which a single direction of hatch would confuse with single-sensor.
                DeadSpotCell::WouldDarken => {
                    if (x + y).is_multiple_of(period)
                        || (x + period - y % period).is_multiple_of(period)
                    {
                        darken
                    } else {
                        egui::Color32::TRANSPARENT
                    }
                }
            });
        }
    }
    egui::ColorImage {
        size: [width, height],
        pixels,
        ..egui::ColorImage::default()
    }
}

/// The dead-spot texture the viewport keeps across frames, and what it was built from.
pub struct DeadSpotTexture {
    /// The field's revision, the pattern period, and the palette's colours the image is
    /// drawn in: a change to any of them rebuilds the image.
    key: (u64, u32, egui::Color32, egui::Color32, egui::Color32),
    pub handle: egui::TextureHandle,
}

/// Bring `slot` up to date with `layer`: build the texture when the field, the zoom's
/// pattern bucket or the palette changed, and drop it when there is no field. Called once
/// a frame before the layers are drawn; costs nothing when nothing changed.
pub fn sync_dead_spot_texture(
    ctx: &egui::Context,
    palette: &theme::Palette,
    view: &TopDownView,
    slot: &mut Option<DeadSpotTexture>,
    layer: DeadSpotLayer<'_>,
) {
    let DeadSpotLayer::Field(field) = layer else {
        *slot = None;
        return;
    };
    if field.rows == 0 || field.columns == 0 {
        *slot = None;
        return;
    }
    let period = dead_spot_pattern_cells(field.cell_m, view.meters_per_px);
    let key = (
        field.revision,
        period,
        palette.shadow_color,
        palette.warning_color,
        palette.alert_color,
    );
    if slot.as_ref().is_some_and(|t| t.key == key) {
        return;
    }
    let max_side = ctx.input(|i| i.max_texture_side).max(1);
    #[allow(clippy::cast_possible_truncation)]
    let stride = (field.rows.max(field.columns) as usize)
        .div_ceil(max_side)
        .max(1) as u32;
    let image = dead_spot_image(&field, palette, (period / stride).max(2), stride);
    let handle = ctx.load_texture("gungnir-dead-spots", image, egui::TextureOptions::LINEAR);
    *slot = Some(DeadSpotTexture { key, handle });
}

/// The screen rectangle a field covers.
#[must_use]
pub fn dead_spot_rect(
    field: &DeadSpotField<'_>,
    rect: egui::Rect,
    view: &TopDownView,
) -> egui::Rect {
    let [e, n] = field.origin_enu;
    let width = f64::from(field.columns) * field.cell_m;
    let height = f64::from(field.rows) * field.cell_m;
    egui::Rect::from_two_pos(
        view.project([e, n + height, 0.0], rect),
        view.project([e + width, n, 0.0], rect),
    )
}

/// Draw the dead-spot layer (GAP-246): the field's texture over the terrain and under
/// everything else, and the line that says what it is; or, with no field, the sentence
/// that says why. A texture built for an older field is not drawn. `line` is the layer's
/// sentence as [`DeadSpotLineKey::write`] wrote it for this layer.
pub fn draw_dead_spots_2d(
    painter: &egui::Painter,
    palette: &theme::Palette,
    rect: egui::Rect,
    view: &TopDownView,
    texture: Option<&DeadSpotTexture>,
    layer: DeadSpotLayer<'_>,
    line: &str,
) {
    let anchor = rect.left_bottom()
        + egui::vec2(
            palette.panel_spacing,
            -palette.panel_spacing - 1.6 * palette.small_font_size,
        );
    let colour = match layer {
        DeadSpotLayer::NotComputed { .. } | DeadSpotLayer::Computing => palette.muted_text_color(),
        DeadSpotLayer::Field(field) => {
            if let Some(texture) = texture.filter(|t| t.key.0 == field.revision) {
                let tint = if field.stale {
                    egui::Color32::WHITE.gamma_multiply(0.5)
                } else {
                    egui::Color32::WHITE
                };
                painter.image(
                    texture.handle.id(),
                    dead_spot_rect(&field, rect, view),
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    tint,
                );
            }
            if field.stale {
                palette.warning_color
            } else {
                palette.viewport_text_color
            }
        }
    };
    painter.text(
        anchor,
        egui::Align2::LEFT_BOTTOM,
        line,
        egui::FontId::proportional(palette.small_font_size),
        colour,
    );
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
    /// Which placement these positions are (GAP-252, DN-35 §6). The caller sets it when it
    /// places a terrain and changes it on any later placement, so the surface both
    /// renderers draw is built once per load and never per frame.
    pub revision: u64,
}

/// One loaded point cloud as the viewport draws it (GAP-098).
///
/// Borrowed from `gungnir_data::pointcloud::PointBuffer` by the caller rather than that
/// type itself, the same reason [`TerrainLayer`] borrows from `TerrainMesh`: the drawing
/// does not depend on the loader's shape.
///
/// **Positions are relative to the cloud's own origin, not placed against the
/// deployment's ENU origin.** `PointBuffer` carries no parsed CRS -- unlike a DEM's
/// `GridCrs`, nothing here reads a LAS file's own coordinate reference system and
/// nothing rejects one that contradicts `PointCloudConfig::frame` -- so there is no
/// placement step to mirror `TerrainMesh::placed()`. Drawing the raw, origin-relative
/// positions is the honest choice: it shows the cloud's real shape and extent rather
/// than a claimed alignment with the rest of the picture that nothing has computed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointCloudLayer<'a> {
    pub positions: &'a [[f32; 3]],
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

/// The height ramp's colour, unmultiplied: dark green low, pale brown high.
#[must_use]
pub fn terrain_rgb(t: f32) -> [u8; 3] {
    let t = if t.is_finite() {
        t.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let low = [46.0_f32, 92.0, 52.0];
    let high = [176.0_f32, 150.0, 110.0];
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let mix = |i: usize| (low[i] + (high[i] - low[i]) * t) as u8;
    [mix(0), mix(1), mix(2)]
}

/// The colour of a height between the surface's lowest and highest: dark green low,
/// pale brown high, translucent so tracks and rings stay legible over it.
#[must_use]
pub fn terrain_color(palette: &theme::Palette, t: f32) -> egui::Color32 {
    let [r, g, b] = terrain_rgb(t);
    egui::Color32::from_rgba_unmultiplied(r, g, b, palette.terrain_alpha)
}

/// Where the hillshade's light comes from (GAP-252, D-157, DN-35 amendment 1 item 1): the
/// north-west, the cartographic convention, because relief lit from the upper left of the
/// screen is read as relief rather than inverted.
pub const HILLSHADE_AZIMUTH_DEG: f64 = 315.0;
/// The light's height above the horizon, in degrees.
pub const HILLSHADE_ALTITUDE_DEG: f64 = 45.0;
/// The darkest a slope facing away from the light is drawn, as a factor on the ramp's
/// colour. With [`HILLSHADE_BRIGHTEST`] it keeps every slope off black and white, and a dead
/// spot over the lightest ground darker than the darkest ground with none (a test holds it).
pub const HILLSHADE_DARKEST: f64 = 0.75;
/// The lightest a slope facing the light is drawn.
pub const HILLSHADE_BRIGHTEST: f64 = 1.25;

/// The hillshade factor for ground rising `de` metres per metre east and `dn` per metre
/// north: the light's cosine on the ground's normal, over its cosine on flat ground, so
/// **flat ground is exactly one** and keeps the ramp's colour unchanged. Bounded by
/// [`HILLSHADE_DARKEST`] and [`HILLSHADE_BRIGHTEST`].
#[must_use]
pub fn hillshade(de: f64, dn: f64) -> f64 {
    let (sin_az, cos_az) = HILLSHADE_AZIMUTH_DEG.to_radians().sin_cos();
    let (sin_alt, cos_alt) = HILLSHADE_ALTITUDE_DEG.to_radians().sin_cos();
    // The light's direction in east, north, up; the ground's normal is (-de, -dn, 1).
    let lit = (-de * sin_az * cos_alt - dn * cos_az * cos_alt + sin_alt)
        / (de * de + dn * dn + 1.0).sqrt();
    let factor = lit / sin_alt;
    if factor.is_finite() {
        factor.clamp(HILLSHADE_DARKEST, HILLSHADE_BRIGHTEST)
    } else {
        1.0
    }
}

/// The ground's rise per metre east and north at kept vertex `(i, j)` of a decimated grid,
/// by central differences between its kept neighbours, one-sided where only one is kept.
/// A neighbour with no height is left out; an axis with none is taken as flat.
fn gradient(grid: &[Option<[f32; 3]>], width: usize, i: usize, j: usize) -> (f64, f64) {
    let height = grid.len() / width.max(1);
    let at = |i: usize, j: usize| grid[i * width + j].map(|p| p.map(f64::from));
    let Some(here) = at(i, j) else {
        return (0.0, 0.0);
    };
    let difference = |before: Option<[f64; 3]>, after: Option<[f64; 3]>| {
        let (from, to) = match (before, after) {
            (Some(a), Some(b)) => (a, b),
            (Some(a), None) => (a, here),
            (None, Some(b)) => (here, b),
            (None, None) => return None,
        };
        Some([to[0] - from[0], to[1] - from[1], to[2] - from[2]])
    };
    let across = difference(
        j.checked_sub(1).and_then(|j| at(i, j)),
        (j + 1 < width).then(|| at(i, j + 1)).flatten(),
    );
    let down = difference(
        i.checked_sub(1).and_then(|i| at(i, j)),
        (i + 1 < height).then(|| at(i + 1, j)).flatten(),
    );
    rise(across, down)
}

/// The ground's rise per metre east and north from a difference vector along each of the
/// grid's two axes, either of which may be missing (GAP-252; shared with the staged tiles,
/// GAP-227, which choose their neighbours differently at a tile's edge).
pub(crate) fn rise(across: Option<[f64; 3]>, down: Option<[f64; 3]>) -> (f64, f64) {
    // Along one axis only: the rise is along that axis's horizontal direction.
    let along = |v: [f64; 3]| {
        let run = v[0] * v[0] + v[1] * v[1];
        if run > 0.0 {
            (v[2] * v[0] / run, v[2] * v[1] / run)
        } else {
            (0.0, 0.0)
        }
    };
    match (across, down) {
        (Some(a), Some(b)) => {
            // The plane through both differences: solve for the rise per metre east and
            // north. A placed grid's axes are never parallel; if they were, take one.
            let det = a[0] * b[1] - a[1] * b[0];
            if det.abs() > f64::EPSILON * (a[0] * a[0] + a[1] * a[1] + b[0] * b[0] + b[1] * b[1]) {
                (
                    (a[2] * b[1] - a[1] * b[2]) / det,
                    (a[0] * b[2] - a[2] * b[0]) / det,
                )
            } else {
                along(a)
            }
        }
        (Some(v), None) | (None, Some(v)) => along(v),
        (None, None) => (0.0, 0.0),
    }
}

/// The ramp's colour lit by `shade`: each component multiplied, the alpha unchanged. A
/// factor of exactly one gives exactly [`terrain_color`]'s colour.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(crate) fn shaded(palette: &theme::Palette, t: f32, shade: f64) -> egui::Color32 {
    let [r, g, b] = terrain_rgb(t).map(|c| (f64::from(c) * shade).round().clamp(0.0, 255.0) as u8);
    egui::Color32::from_rgba_unmultiplied(r, g, b, palette.terrain_alpha)
}

/// The placed terrain as both renderers draw it: the decimated grid, its colours and its
/// triangles, built once per load (`rust-ui-architecture-coding-standards.md` §5), never
/// per frame (GAP-252, DN-35 §6).
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainSurface {
    /// The [`TerrainLayer::revision`] this was built from.
    pub revision: u64,
    /// ENU metres, the kept vertices only; a no-data corner keeps no vertex.
    pub positions: Vec<[f32; 3]>,
    /// The height ramp over the **full** grid's lowest and highest height, as the projection
    /// coloured it before GAP-252, lit by the hillshade (DN-35 amendment 1 item 1).
    pub colors: Vec<egui::Color32>,
    /// Two triangles per decimated cell whose four corners all have a height.
    pub indices: Vec<u32>,
}

/// Build the surface both renderers draw from a placed grid: decimated to
/// [`TERRAIN_VERTEX_BUDGET`], coloured by height over the full grid, and hillshaded on the
/// decimated grid, so the shade matches the triangles drawn.
///
/// # Errors
///
/// [`ViewportError::MalformedMesh`] when the positions do not fill `rows * columns`, and
/// [`ViewportError::EmptyMesh`] when no cell has four corners with a height. Every refusal
/// happens here, before anything reaches a GPU.
///
/// [`ViewportError::MalformedMesh`]: crate::ViewportError::MalformedMesh
/// [`ViewportError::EmptyMesh`]: crate::ViewportError::EmptyMesh
pub fn terrain_surface(
    palette: &theme::Palette,
    layer: TerrainLayer<'_>,
) -> Result<TerrainSurface, crate::ViewportError> {
    let (rows, columns) = (layer.rows as usize, layer.columns as usize);
    if layer.positions.len() != rows.saturating_mul(columns) {
        return Err(crate::ViewportError::MalformedMesh {
            reason: format!(
                "{} positions do not fill a grid of {rows} rows by {columns} columns",
                layer.positions.len()
            ),
        });
    }
    if rows < 2 || columns < 2 {
        return Err(crate::ViewportError::EmptyMesh);
    }
    let (lo, hi) = layer
        .positions
        .iter()
        .map(|p| p[2])
        .filter(|z| z.is_finite())
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), z| {
            (lo.min(z), hi.max(z))
        });
    if !lo.is_finite() {
        return Err(crate::ViewportError::EmptyMesh);
    }
    let span = if hi > lo { hi - lo } else { 1.0 };
    let stride = terrain_stride(layer.rows, layer.columns) as usize;
    let sampled_rows: Vec<usize> = (0..rows).step_by(stride).collect();
    let sampled_columns: Vec<usize> = (0..columns).step_by(stride).collect();
    let width = sampled_columns.len();
    // Vertex (i, j) of the decimated grid is at index i * width + j.
    let grid: Vec<Option<[f32; 3]>> = sampled_rows
        .iter()
        .flat_map(|&r| {
            sampled_columns.iter().map(move |&c| {
                let p = layer.positions[r * columns + c];
                p[2].is_finite().then_some(p)
            })
        })
        .collect();
    let mut positions = Vec::with_capacity(grid.len());
    let mut colors = Vec::with_capacity(grid.len());
    let mut ids: Vec<Option<u32>> = Vec::with_capacity(grid.len());
    for (k, p) in grid.iter().enumerate() {
        let Some(p) = p else {
            ids.push(None);
            continue;
        };
        let (de, dn) = gradient(&grid, width, k / width, k % width);
        #[allow(clippy::cast_possible_truncation)]
        ids.push(Some(positions.len() as u32));
        positions.push(*p);
        colors.push(shaded(palette, (p[2] - lo) / span, hillshade(de, dn)));
    }
    let mut indices = Vec::new();
    for i in 0..sampled_rows.len().saturating_sub(1) {
        for j in 0..width.saturating_sub(1) {
            let (nw, ne, sw, se) = (
                ids[i * width + j],
                ids[i * width + j + 1],
                ids[(i + 1) * width + j],
                ids[(i + 1) * width + j + 1],
            );
            if let (Some(nw), Some(ne), Some(sw), Some(se)) = (nw, ne, sw, se) {
                indices.extend_from_slice(&[nw, sw, ne, ne, sw, se]);
            }
        }
    }
    if indices.is_empty() {
        return Err(crate::ViewportError::EmptyMesh);
    }
    Ok(TerrainSurface {
        revision: layer.revision,
        positions,
        colors,
        indices,
    })
}

/// The surface the viewport holds and the revision it was built from. A refusal is held
/// too, so a grid that cannot be drawn is refused once a load rather than once a frame.
#[derive(Debug)]
pub struct TerrainSurfaceSlot {
    pub revision: u64,
    pub surface: Result<std::sync::Arc<TerrainSurface>, crate::ViewportError>,
}

/// Bring `slot` up to date with `layer`: build the surface when the revision changed, and
/// drop it when there is no terrain. Called once a frame before anything is drawn; costs
/// nothing when nothing changed, as [`sync_dead_spot_texture`] does. Returns whether it
/// built.
pub fn sync_terrain_surface(
    palette: &theme::Palette,
    slot: &mut Option<TerrainSurfaceSlot>,
    layer: Option<TerrainLayer<'_>>,
) -> bool {
    let Some(layer) = layer else {
        *slot = None;
        return false;
    };
    if slot.as_ref().is_some_and(|s| s.revision == layer.revision) {
        return false;
    }
    *slot = Some(TerrainSurfaceSlot {
        revision: layer.revision,
        surface: terrain_surface(palette, layer).map(std::sync::Arc::new),
    });
    true
}

/// The surface projected into the top-down view as an egui mesh. The colours and
/// triangles were decided once, in [`terrain_surface`]; the projection is done when the
/// surface, the view or the rect changes, by [`sync_terrain_mesh_2d`], and not per frame
/// (GAP-255).
#[must_use]
pub fn terrain_mesh_2d(
    rect: egui::Rect,
    view: &TopDownView,
    surface: &TerrainSurface,
) -> egui::Mesh {
    let mut mesh = egui::Mesh::default();
    mesh.vertices.reserve(surface.positions.len());
    for (p, colour) in surface.positions.iter().zip(&surface.colors) {
        let pos = view.project([f64::from(p[0]), f64::from(p[1]), 0.0], rect);
        mesh.colored_vertex(pos, *colour);
    }
    mesh.indices.clone_from(&surface.indices);
    mesh
}

/// What the overlay's terrain mesh was projected under (GAP-255): which surface, at which
/// view, into which rect. Any change to one of them is a reprojection; nothing else is.
/// `PartialEq` and not `Eq` because a view holds floats: a NaN view rebuilds every frame,
/// which errs towards drawing, as `glyphs_need_rebuild` does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerrainMeshKey {
    /// [`TerrainSurface::revision`].
    pub revision: u64,
    pub view: TopDownView,
    pub rect: egui::Rect,
}

impl TerrainMeshKey {
    #[must_use]
    pub fn new(surface: &TerrainSurface, view: &TopDownView, rect: egui::Rect) -> Self {
        Self {
            revision: surface.revision,
            view: *view,
            rect,
        }
    }
}

/// The overlay's terrain mesh the viewport keeps across frames, and what it was projected
/// under (GAP-255), the model being [`DeadSpotTexture`]. Held behind an `Arc` because
/// `egui::Shape::Mesh` takes one, so drawing it each frame clones a pointer and not up to
/// [`TERRAIN_VERTEX_BUDGET`] vertices.
#[derive(Debug, Clone)]
pub struct TerrainMesh2d {
    key: TerrainMeshKey,
    pub mesh: std::sync::Arc<egui::Mesh>,
}

impl TerrainMesh2d {
    /// What the mesh was projected under.
    #[must_use]
    pub fn key(&self) -> TerrainMeshKey {
        self.key
    }
}

/// Bring `slot` up to date for a frame that draws `surface` into `rect` at `view`: project
/// the mesh when the surface, the view or the rect differs from what the slot holds, and
/// drop it when there is no surface to draw. Called once a frame before the layers are
/// drawn, as [`sync_dead_spot_texture`] is; costs nothing when nothing changed. Returns
/// whether it projected (GAP-255; before it, `draw_terrain_2d` projected every frame).
pub fn sync_terrain_mesh_2d(
    slot: &mut Option<TerrainMesh2d>,
    surface: Option<&TerrainSurface>,
    view: &TopDownView,
    rect: egui::Rect,
) -> bool {
    let Some(surface) = surface else {
        *slot = None;
        return false;
    };
    if surface.indices.is_empty() {
        *slot = None;
        return false;
    }
    let key = TerrainMeshKey::new(surface, view, rect);
    if slot.as_ref().is_some_and(|m| m.key == key) {
        return false;
    }
    *slot = Some(TerrainMesh2d {
        key,
        mesh: std::sync::Arc::new(terrain_mesh_2d(rect, view, surface)),
    });
    true
}

/// Draw the terrain as a shaded surface in the top-down view: the mesh
/// [`sync_terrain_mesh_2d`] projected for this frame. Nothing is drawn for no surface, and
/// nothing is written either: the health panel is where the absence of terrain is said
/// (PN-09), and a label on every empty map would be noise. A mesh projected for another
/// surface, view or rect is not drawn, as a dead-spot texture built for an older field is
/// not: the paint-order tests see a missing ground, where a silent reprojection here would
/// put the per-frame build back.
pub fn draw_terrain_2d(
    painter: &egui::Painter,
    rect: egui::Rect,
    view: &TopDownView,
    surface: Option<&TerrainSurface>,
    mesh: Option<&TerrainMesh2d>,
) {
    let (Some(surface), Some(mesh)) = (surface, mesh) else {
        return;
    };
    if mesh.key != TerrainMeshKey::new(surface, view, rect) {
        return;
    }
    painter.add(egui::Shape::mesh(std::sync::Arc::clone(&mesh.mesh)));
}

/// The most points a frame draws per cloud, the same reasoning as
/// [`TERRAIN_VERTEX_BUDGET`]: a bounded COPC query alone can return several thousand
/// points, egui should not be asked to place all of them every frame, and a stride
/// through the array (never a truncation) keeps the sampled cloud's shape whole rather
/// than showing only whichever points happened to load first.
pub const POINT_CLOUD_VERTEX_BUDGET: usize = 20_000;

/// Stride that brings a cloud's point count under the budget.
#[must_use]
pub fn point_cloud_stride(point_count: usize) -> usize {
    if point_count <= POINT_CLOUD_VERTEX_BUDGET {
        return 1;
    }
    point_count.div_ceil(POINT_CLOUD_VERTEX_BUDGET)
}

/// Draw the configured point-cloud pair (GAP-098). Nothing is drawn for an empty slice,
/// and nothing is written either, the same convention [`draw_terrain_2d`] follows: the
/// health panel is not this crate's to draw, so the absence of a pair is said there
/// rather than as a label on every empty map.
///
/// Source and target are drawn in different colours -- the only way this view
/// distinguishes them, since neither carries a name past this point -- so a pair that
/// has loaded reads as a pair rather than as one undifferentiated cloud.
pub fn draw_point_clouds_2d(
    painter: &egui::Painter,
    palette: &theme::Palette,
    rect: egui::Rect,
    view: &TopDownView,
    clouds: &[PointCloudLayer<'_>],
) {
    const DOT_RADIUS: f32 = 1.5;
    for (index, cloud) in clouds.iter().enumerate() {
        let color = point_cloud_color(palette, index);
        let stride = point_cloud_stride(cloud.positions.len());
        for p in cloud.positions.iter().step_by(stride) {
            let pos = view.project([f64::from(p[0]), f64::from(p[1]), 0.0], rect);
            if rect.contains(pos) {
                painter.circle_filled(pos, DOT_RADIUS, color);
            }
        }
    }
}

/// The source cloud (index 0) and the target (index 1, and every index after it, which
/// only arises if a caller hands this more than the pair `LayerInputs::point_clouds`
/// documents) get their own colours; anything past the pair repeats the target's rather
/// than indexing past a fixed palette.
#[must_use]
pub fn point_cloud_color(palette: &theme::Palette, index: usize) -> egui::Color32 {
    if index == 0 {
        palette.point_cloud_source_color
    } else {
        palette.point_cloud_target_color
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
    palette: &theme::Palette,
    rect: egui::Rect,
    view: &TopDownView,
    predictions: &[PredictedPath<'_>],
) {
    for p in predictions {
        let Some((nearest, after)) = p.points.split_first() else {
            continue;
        };
        if after.is_empty() {
            continue;
        }
        let stroke = egui::Stroke::new(palette.stroke_emphasis, palette.warning_color);
        // Each point is projected once, as the segment ending at it is drawn (GAP-257).
        let mut a = view.project(*nearest, rect);
        for q in after {
            let b = view.project(*q, rect);
            // Dashes: split each segment into thirds and draw the outer two.
            let d = b - a;
            painter.line_segment([a, a + d / 3.0], stroke);
            painter.line_segment([a + d * (2.0 / 3.0), b], stroke);
            a = b;
        }
        if p.uncertainty_drawable {
            painter.circle_stroke(a, 4.0, stroke);
        }
    }
}

/// The least a component of a dense group must account for to be drawn: half a target,
/// the PHD's own conventional extraction threshold (`gungnir_rfs::PhdSettings`: "a
/// component accounting for less than half a target is not one"). The mixture holds up to
/// hundreds of components, most of them slivers of mass, and drawing every one would
/// paint the raid's whole neighbourhood rather than where the targets are.
pub const GROUP_COMPONENT_DRAW_WEIGHT: f64 = 0.5;

/// What a dense group's label says (GAP-015): that it is **not tracks**, the expected
/// count as the mean it is, the filter, the CPHD's most likely count where there is one,
/// the epoch it is of, and whether it has gone stale.
#[must_use]
pub fn dense_group_label(group: &gungnir_model::DenseGroupView) -> String {
    use std::fmt::Write as _;
    let mut label = format!(
        "dense group, not tracks: about {:.1} targets ({})",
        group.expected_targets, group.filter
    );
    if let Some(n) = group.most_probable_count {
        let _ = write!(label, ", most likely {n}");
    }
    let _ = write!(label, ", as of t = {:.0} s", group.epoch.0);
    if group.is_stale {
        label.push_str(", stale");
    }
    label
}

/// What a dense group's label states (GAP-257): the facts [`dense_group_label`] prints, so
/// the label is written when the estimate changes and not on every frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DenseGroupLabelKey {
    /// A resolvable scene: no group, no label.
    None,
    Group {
        epoch: u64,
        expected_targets: u64,
        most_probable_count: Option<u64>,
        filter: String,
        is_stale: bool,
    },
}

impl DenseGroupLabelKey {
    /// Whether a label written from this key is the label `group` calls for.
    #[must_use]
    pub fn matches(&self, group: Option<&gungnir_model::DenseGroupView>) -> bool {
        match (self, group) {
            (Self::None, None) => true,
            (
                Self::Group {
                    epoch,
                    expected_targets,
                    most_probable_count,
                    filter,
                    is_stale,
                },
                Some(g),
            ) => {
                *epoch == g.epoch.0.to_bits()
                    && *expected_targets == g.expected_targets.to_bits()
                    && *most_probable_count == g.most_probable_count
                    && *filter == g.filter
                    && *is_stale == g.is_stale
            }
            _ => false,
        }
    }

    /// Write `group`'s label into `text`, and return the key it was written from.
    pub fn write(group: Option<&gungnir_model::DenseGroupView>, text: &mut String) -> Self {
        let Some(g) = group else {
            return Self::None;
        };
        text.push_str(&dense_group_label(g));
        Self::Group {
            epoch: g.epoch.0.to_bits(),
            expected_targets: g.expected_targets.to_bits(),
            most_probable_count: g.most_probable_count,
            filter: g.filter.clone(),
            is_stale: g.is_stale,
        }
    }
}

/// Draw a dense group (GAP-015): a translucent disc two sigma across for each component
/// holding at least [`GROUP_COMPONENT_DRAW_WEIGHT`] of a target, in the unknown-identity
/// colour because a group has no identity, and one label at the group's weighted centre.
/// `label` is the group's words as [`DenseGroupLabelKey::write`] wrote them for this group.
///
/// **No symbol, no identifier, no per-component count.** A disc is mass at a place, and
/// drawing it as a track symbol would claim the identity the filter does not carry.
pub fn draw_dense_group_2d(
    painter: &egui::Painter,
    palette: &theme::Palette,
    rect: egui::Rect,
    view: &TopDownView,
    group: Option<&gungnir_model::DenseGroupView>,
    label: &str,
) {
    let Some(group) = group else {
        return;
    };
    let color = palette.class_unknown_color;
    let stroke = egui::Stroke::new(palette.stroke_emphasis, color);
    let fill = color.gamma_multiply(0.2);
    let (mut weight, mut east, mut north) = (0.0, 0.0, 0.0);
    for c in &group.components {
        if c.expected_targets.is_finite() && c.expected_targets > 0.0 {
            weight += c.expected_targets;
            east += c.expected_targets * c.position_enu[0];
            north += c.expected_targets * c.position_enu[1];
        }
        if c.expected_targets < GROUP_COMPONENT_DRAW_WEIGHT {
            continue;
        }
        let radius_m = 2.0 * c.position_sigma_m[0].max(c.position_sigma_m[1]);
        if !radius_m.is_finite() {
            continue;
        }
        let centre = view.project(c.position_enu, rect);
        let edge = view.project(
            [
                c.position_enu[0] + radius_m,
                c.position_enu[1],
                c.position_enu[2],
            ],
            rect,
        );
        painter.circle(centre, (edge - centre).length().max(3.0), fill, stroke);
    }
    let anchor = if weight > 0.0 {
        view.project([east / weight, north / weight, 0.0], rect)
    } else {
        rect.left_bottom() + egui::vec2(palette.panel_spacing, -3.0 * palette.panel_spacing)
    };
    painter.text(
        anchor,
        egui::Align2::CENTER_BOTTOM,
        label,
        egui::FontId::proportional(palette.small_font_size),
        if group.is_stale {
            palette.track_stale_color
        } else {
            palette.viewport_text_color
        },
    );
}

/// One laydown option's sensor and resource positions, previewed on the map for a
/// visual before-and-after against the current picture -- GAP-087's own remaining item,
/// the "comparison" `gungnir-ui`'s coverage-layer controls named as unavailable until
/// now. `None` when PN-16 has no option selected: no toggle exists for this layer, the
/// same "an empty list draws nothing" shape [`PredictedPath`] uses, because a preview
/// with nothing selected to preview is not a state an operator turns on or off.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LaydownPreview<'a> {
    /// The option's own words for what it is for (`gungnir_model::laydown::Laydown::intent`).
    pub intent: &'a str,
    pub sensors: &'a [[f64; 3]],
    pub resources: &'a [[f64; 3]],
}

/// Draw a laydown preview: a hollow circle per sensor position, a hollow diamond per
/// resource position, both in the one hue no other layer draws on the map, so a
/// proposed placement is never mistaken for the live picture's own sensors and
/// resources. The label is the option's own intent, not its bare identifier, because
/// "cover the sea approach" tells a reader what they are looking at and "west-2" does
/// not. `label` is that line, written when the selection changes (GAP-257).
pub fn draw_laydown_preview_2d(
    painter: &egui::Painter,
    palette: &theme::Palette,
    rect: egui::Rect,
    view: &TopDownView,
    preview: Option<LaydownPreview<'_>>,
    label: &str,
) {
    const MARKER_RADIUS: f32 = 7.0;
    let Some(preview) = preview else {
        return;
    };
    let stroke = egui::Stroke::new(palette.stroke_emphasis, palette.laydown_preview_color);
    for &enu in preview.sensors {
        painter.circle_stroke(view.project(enu, rect), MARKER_RADIUS, stroke);
    }
    for &enu in preview.resources {
        let p = view.project(enu, rect);
        let points = vec![
            p + egui::vec2(0.0, -MARKER_RADIUS),
            p + egui::vec2(MARKER_RADIUS, 0.0),
            p + egui::vec2(0.0, MARKER_RADIUS),
            p + egui::vec2(-MARKER_RADIUS, 0.0),
        ];
        painter.add(egui::Shape::convex_polygon(
            points,
            egui::Color32::TRANSPARENT,
            stroke,
        ));
    }
    if let Some(&label_at) = preview
        .sensors
        .first()
        .or_else(|| preview.resources.first())
    {
        painter.text(
            view.project(label_at, rect) + egui::vec2(0.0, -2.0 * MARKER_RADIUS),
            egui::Align2::CENTER_BOTTOM,
            label,
            egui::FontId::proportional(palette.small_font_size),
            palette.laydown_preview_color,
        );
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
    palette: &theme::Palette,
    rect: egui::Rect,
    view: &TopDownView,
    fences: &[GeofenceOutline<'_>],
) {
    const SEGMENTS: usize = 64;
    for fence in fences {
        let colour = if fence.no_go {
            palette.alert_color
        } else {
            palette.muted_text_color()
        };
        let stroke = egui::Stroke::new(palette.stroke_emphasis, colour);
        let point = |i: usize| {
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
        };
        // Every other arc piece is a dash, each projected as it is drawn (GAP-257): the
        // same 32 dashes the 65-point ring gave when it was collected and paired.
        for i in (0..SEGMENTS).step_by(2) {
            painter.line_segment([point(i), point(i + 1)], stroke);
        }
        painter.text(
            view.project(fence.center_enu, rect),
            egui::Align2::CENTER_CENTER,
            if fence.no_go {
                format!("{} (no-go)", fence.name)
            } else {
                fence.name.to_string()
            },
            egui::FontId::proportional(palette.small_font_size),
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
    palette: &theme::Palette,
    rect: egui::Rect,
    view: &TopDownView,
    hazards: &[HazardOutline<'_>],
) {
    for hazard in hazards {
        let Some(first) = hazard.samples.first().filter(|_| hazard.samples.len() >= 2) else {
            continue;
        };
        let first = view.project(*first, rect);
        let stroke = egui::Stroke::new(palette.stroke_hazard, palette.hazard_color);
        if hazard.blocks_surface {
            // egui owns a line's points, so this is the one vector the layer allocates; it
            // was collected and then cloned for the shape (GAP-257).
            painter.add(egui::Shape::line(
                hazard
                    .samples
                    .iter()
                    .map(|p| view.project(*p, rect))
                    .collect(),
                stroke,
            ));
        } else {
            for pair in hazard.samples.chunks(2) {
                if let [a, b] = pair {
                    painter.line_segment([view.project(*a, rect), view.project(*b, rect)], stroke);
                }
            }
        }
        // The label sits at the outline's first point; for a ring that is its eastmost
        // sample, for a boom one end.
        painter.text(
            first + egui::vec2(palette.panel_spacing, -palette.panel_spacing),
            egui::Align2::LEFT_BOTTOM,
            format!("{} ({})", hazard.name, hazard.kind),
            egui::FontId::proportional(palette.small_font_size),
            palette.hazard_color,
        );
    }
}

fn draw_rings(
    painter: &egui::Painter,
    palette: &theme::Palette,
    rect: egui::Rect,
    view: &TopDownView,
    layer: CoverageLayer<'_>,
    note: &str,
) {
    let (circles, nominal) = match layer {
        CoverageLayer::Circles { circles, nominal } => (circles, nominal),
        CoverageLayer::None(_) => {
            painter.text(
                rect.left_bottom() + egui::vec2(palette.panel_spacing, -palette.panel_spacing),
                egui::Align2::LEFT_BOTTOM,
                note,
                egui::FontId::proportional(palette.small_font_size),
                palette.muted_text_color(),
            );
            return;
        }
    };

    if nominal.is_some() {
        painter.text(
            rect.left_bottom() + egui::vec2(palette.panel_spacing, -palette.panel_spacing),
            egui::Align2::LEFT_BOTTOM,
            note,
            egui::FontId::proportional(palette.small_font_size),
            palette.warning_color,
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
        let stroke = egui::Stroke::new(
            palette.stroke_hairline,
            coverage_color(palette, circle.confidence),
        );
        match sector_outline_points(circle, SECTOR_SEGMENTS) {
            Some(outline) => {
                // Projected straight into the one vector egui's line owns (GAP-257).
                let points: Vec<egui::Pos2> = outline.map(|p| view.project(p, rect)).collect();
                painter.add(egui::Shape::line(points, stroke));
            }
            None => {
                painter.circle_stroke(center, radius_px, stroke);
            }
        }
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
pub fn coverage_color(palette: &theme::Palette, confidence: f32) -> egui::Color32 {
    let base = palette.coverage_color;
    // `clamp` on a NaN returns NaN, and `NaN as u8` is 0 -- nothing drawn, which is the
    // right answer for a confidence nobody can read.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let alpha = (f32::from(palette.coverage_max_alpha) * confidence.clamp(0.0, 1.0)) as u8;
    egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GAP-015: the label says the group is not tracks, gives the count as the mean it
    /// is, names the filter and the epoch, gives a most likely count only where the
    /// filter has one, and says when it has gone stale.
    #[test]
    fn a_dense_group_label_says_not_tracks_and_what_it_knows() {
        let mut group = gungnir_model::DenseGroupView {
            epoch: gungnir_model::MissionTime(412.0),
            expected_targets: 19.62,
            most_probable_count: None,
            count_distribution: Vec::new(),
            components: Vec::new(),
            filter: "gm-phd".into(),
            scan_size: 21,
            is_stale: false,
        };
        let phd = dense_group_label(&group);
        assert!(phd.contains("not tracks") && phd.contains("19.6"), "{phd}");
        assert!(phd.contains("gm-phd") && phd.contains("412"), "{phd}");
        assert!(
            !phd.contains("most likely") && !phd.contains("stale"),
            "{phd}"
        );
        group.most_probable_count = Some(20);
        group.is_stale = true;
        let cphd = dense_group_label(&group);
        assert!(
            cphd.contains("most likely 20") && cphd.ends_with("stale"),
            "{cphd}"
        );
    }

    /// GAP-257: the dead-spot line is written once and kept while what it states holds. It
    /// prints the age in whole seconds, so the key moves once a second and not every frame;
    /// a field going stale or a new revision rewrites it.
    #[test]
    fn the_dead_spot_line_is_rewritten_when_what_it_states_changes() {
        fn layer(f: DeadSpotField<'_>) -> DeadSpotLayer<'_> {
            DeadSpotLayer::Field(f)
        }
        let cells = [DeadSpotCell::Clear; 4];
        let mut f = field(&cells, 2);
        let mut text = String::new();
        let key = DeadSpotLineKey::write(&layer(f), &mut text);
        assert_eq!(text, dead_spot_line(&f));

        f.age_s = 6.4;
        assert!(key.matches(&layer(f)), "6.4 s prints as the same 6 s");
        assert_eq!(dead_spot_line(&f), text, "and the line says the same");
        f.age_s = 7.0;
        assert!(!key.matches(&layer(f)), "a new second is a new line");
        f.age_s = 6.0;
        f.stale = true;
        assert!(!key.matches(&layer(f)), "stale is stated");
        f.stale = false;
        f.revision += 1;
        assert!(!key.matches(&layer(f)), "a new field is a new line");
        assert!(!key.matches(&DeadSpotLayer::Computing));
    }

    /// GAP-257: the coverage note is the sentence the layer called for before it was cached,
    /// and is kept only for a layer that would say the same.
    #[test]
    fn the_coverage_note_is_the_layers_own_sentence() {
        let placed = CoverageLayer::None(NoCoverage::NoOrigin { setting: "origin" });
        let mut text = String::new();
        let key = CoverageNoteKey::write(&placed, &mut text);
        assert_eq!(text, NoCoverage::NoOrigin { setting: "origin" }.sentence());
        assert!(key.matches(&placed));
        assert!(!key.matches(&CoverageLayer::None(NoCoverage::NoSensorsActive)));
        assert!(!key.matches(&CoverageLayer::Circles {
            circles: &[],
            nominal: None
        }));

        let mut nominal_text = String::new();
        let nominal = CoverageLayer::Circles {
            circles: &[],
            nominal: Some("no adapter"),
        };
        let nominal_key = CoverageNoteKey::write(&nominal, &mut nominal_text);
        assert!(
            nominal_text.contains("not what it is covering now")
                && nominal_text.contains("no adapter")
        );
        assert!(nominal_key.matches(&nominal));
        let observed = CoverageLayer::Circles {
            circles: &[],
            nominal: None,
        };
        let mut none_text = String::new();
        assert!(CoverageNoteKey::write(&observed, &mut none_text).matches(&observed));
        assert!(none_text.is_empty(), "observed rings carry no note");
    }

    fn field(cells: &[DeadSpotCell], columns: u32) -> DeadSpotField<'_> {
        #[allow(clippy::cast_possible_truncation)]
        let rows = cells.len() as u32 / columns;
        DeadSpotField {
            revision: 1,
            origin_enu: [0.0, 0.0],
            cell_m: 30.0,
            rows,
            columns,
            cells,
            height_agl_m: 100.0,
            age_s: 6.0,
            stale: false,
            sensor: None,
            linked: false,
            what_if: None,
        }
    }

    /// GAP-249 (D-150): a what-if's two kinds of cell differ from every other kind and from
    /// each other, in pattern as well as colour. "Would clear" keeps the dead spot's dark
    /// fill under a hatch in the healthy colour, because it is still dark until somebody
    /// makes the change; "would darken" is a cross-hatch in the alert colour over nothing.
    #[test]
    fn a_what_ifs_cells_are_told_apart_from_each_other_and_from_the_live_kinds() {
        let palette = theme::Palette::day();
        let side = 8u32;
        let image_of = |kind| {
            let cells = vec![kind; (side * side) as usize];
            dead_spot_image(&field(&cells, side), &palette, 4, 1).pixels
        };
        // Every colour is compared as the image draws it, through the same premultiplied
        // alpha: a palette colour read back unmultiplied is off by a unit in a channel.
        let drawn =
            |c: egui::Color32| egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 230);
        let colours = |pixels: &[egui::Color32]| -> std::collections::HashSet<egui::Color32> {
            pixels
                .iter()
                .filter(|p| **p != egui::Color32::TRANSPARENT)
                .copied()
                .collect()
        };
        let clears = image_of(DeadSpotCell::WouldClear);
        let darkens = image_of(DeadSpotCell::WouldDarken);
        assert!(
            clears.iter().all(|p| *p != egui::Color32::TRANSPARENT),
            "would-clear keeps the dark fill everywhere it is not hatched"
        );
        assert!(colours(&clears).contains(&drawn(palette.healthy_color())));
        let shadow_pixel = image_of(DeadSpotCell::Shadow)[0];
        assert!(
            clears.contains(&shadow_pixel),
            "would-clear keeps the dead spot's fill"
        );
        let hatched = darkens
            .iter()
            .filter(|p| **p != egui::Color32::TRANSPARENT)
            .count();
        assert!(
            hatched > 8 && hatched < 40,
            "would-darken is a cross-hatch: {hatched}"
        );
        assert_eq!(
            colours(&darkens),
            std::iter::once(drawn(palette.alert_color)).collect()
        );
        for live in [
            DeadSpotCell::Shadow,
            DeadSpotCell::Single,
            DeadSpotCell::NoData,
        ] {
            let pixels = image_of(live);
            assert_ne!(pixels, clears, "{live:?} and would-clear look alike");
            assert_ne!(pixels, darkens, "{live:?} and would-darken look alike");
        }
    }

    /// The what-if line leads with what it is not, says what is marked, and carries the
    /// height and age the live line does.
    #[test]
    fn a_what_ifs_line_says_it_is_not_the_live_picture() {
        let cells = [DeadSpotCell::WouldClear];
        let f = DeadSpotField {
            what_if: Some("S2 radar switched on, mast +10 m"),
            ..field(&cells, 1)
        };
        let line = dead_spot_line(&f);
        assert!(line.starts_with("WHAT-IF, not the live picture"), "{line}");
        for want in [
            "S2 radar switched on",
            "green hatching would clear",
            "100 m",
            "6 s ago",
        ] {
            assert!(line.contains(want), "{want}: {line}");
        }
    }

    /// GAP-246: each kind of cell differs in pattern as well as colour. A dead spot is
    /// solid and dark, single-sensor is hatched (some pixels clear), no data is a dot grid
    /// over a wash, and a clear cell draws nothing so the terrain shows through.
    #[test]
    fn a_dead_spot_image_carries_its_meaning_in_pattern_as_well_as_colour() {
        let palette = theme::Palette::day();
        let side = 8u32;
        for (kind, check) in [
            (DeadSpotCell::Clear, "clear"),
            (DeadSpotCell::Shadow, "shadow"),
            (DeadSpotCell::Single, "single"),
            (DeadSpotCell::NoData, "no data"),
        ] {
            let cells = vec![kind; (side * side) as usize];
            let image = dead_spot_image(&field(&cells, side), &palette, 4, 1);
            assert_eq!(image.size, [8, 8]);
            let distinct: std::collections::HashSet<_> = image.pixels.iter().collect();
            let clear = image
                .pixels
                .iter()
                .filter(|p| **p == egui::Color32::TRANSPARENT)
                .count();
            match kind {
                DeadSpotCell::Clear => assert_eq!(clear, 64, "{check}"),
                DeadSpotCell::Shadow => {
                    assert_eq!(distinct.len(), 1, "{check} is solid");
                    let [r, g, b, a] = image.pixels[0].to_srgba_unmultiplied();
                    assert!(
                        u16::from(r) + u16::from(g) + u16::from(b) < 60,
                        "{check} is dark"
                    );
                    assert_eq!(a, palette.shadow_alpha);
                }
                DeadSpotCell::Single => {
                    assert!(
                        clear > 16 && clear < 48,
                        "{check} is a hatch: {clear} clear"
                    );
                }
                DeadSpotCell::NoData => {
                    assert_eq!(clear, 0, "{check} is washed");
                    assert_eq!(distinct.len(), 2, "{check} is dots over a wash");
                }
                // A what-if's own kinds, which the next test checks.
                DeadSpotCell::WouldClear | DeadSpotCell::WouldDarken => {}
            }
        }
    }

    /// A field larger than a texture side is drawn one pixel per `stride` cells, never
    /// truncated to its first rows.
    #[test]
    fn a_large_field_is_decimated_into_the_texture() {
        let palette = theme::Palette::day();
        let cells = vec![DeadSpotCell::Shadow; 100 * 30];
        let image = dead_spot_image(&field(&cells, 100), &palette, 4, 3);
        assert_eq!(image.size, [34, 10]);
    }

    /// The pattern keeps about the same pitch on screen whatever the zoom, and changes only
    /// at a doubling, so zooming does not rebuild the texture every frame.
    #[test]
    fn the_pattern_period_follows_the_zoom_in_doublings() {
        assert_eq!(
            dead_spot_pattern_cells(30.0, 1.0),
            2,
            "zoomed in, the floor"
        );
        assert_eq!(dead_spot_pattern_cells(30.0, 30.0), 8);
        assert_eq!(dead_spot_pattern_cells(30.0, 33.0), 16);
        assert_eq!(
            dead_spot_pattern_cells(30.0, 10_000.0),
            64,
            "zoomed out, the ceiling"
        );
        assert_eq!(dead_spot_pattern_cells(30.0, 0.0), 2);
    }

    /// The line says the height, the age, the key, and geometric line of sight; a stale
    /// field and a linked desktop's say so; and a field that could not be computed says the
    /// dead spots are unknown, not absent.
    #[test]
    fn the_dead_spot_lines_say_what_the_field_is_and_what_it_is_not() {
        let cells = [DeadSpotCell::Shadow];
        let mut f = field(&cells, 1);
        let line = dead_spot_line(&f);
        for want in [
            "100 m above ground",
            "6 s ago",
            "geometric line of sight",
            "dark: hidden",
        ] {
            assert!(line.contains(want), "{want}: {line}");
        }
        assert!(
            !line.contains("recomputing") && !line.contains("node"),
            "{line}"
        );
        f.stale = true;
        f.linked = true;
        f.sensor = Some("S2 radar");
        let line = dead_spot_line(&f);
        assert!(
            line.contains("recomputing") && line.contains("node's sensors"),
            "{line}"
        );
        assert!(line.contains("for S2 radar alone"), "{line}");
        let none = dead_spot_not_computed_line("no DEM is configured");
        assert!(
            none.contains("no DEM is configured") && none.contains("unknown"),
            "{none}"
        );
    }

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
        let palette = theme::Palette::day();
        let low = terrain_color(&palette, 0.0);
        let high = terrain_color(&palette, 1.0);
        assert!(high.r() > low.r(), "higher ground is paler");
        assert_eq!(terrain_color(&palette, f32::NAN), low);
        assert!(low.a() < 255, "translucent, so tracks stay legible over it");
    }

    /// `draw_terrain_2d` as it was before GAP-252, kept as the oracle the surface is held
    /// to (DN-35 §8.1 row a): it folded, decimated and coloured the full grid every frame.
    fn oracle_terrain_mesh(
        palette: &theme::Palette,
        rect: egui::Rect,
        view: &TopDownView,
        terrain: TerrainLayer<'_>,
    ) -> Option<egui::Mesh> {
        let (rows, columns) = (terrain.rows, terrain.columns);
        if rows < 2
            || columns < 2
            || terrain.positions.len() != (rows as usize) * (columns as usize)
        {
            return None;
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
            return None;
        }
        let span = if hi > lo { hi - lo } else { 1.0 };
        let stride = terrain_stride(rows, columns);
        let sampled_rows: Vec<u32> = (0..rows).step_by(stride as usize).collect();
        let sampled_columns: Vec<u32> = (0..columns).step_by(stride as usize).collect();
        let mut mesh = egui::Mesh::default();
        let mut ids: Vec<Option<u32>> =
            Vec::with_capacity(sampled_rows.len() * sampled_columns.len());
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
                mesh.colored_vertex(pos, terrain_color(palette, (p[2] - lo) / span));
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
        (!mesh.indices.is_empty()).then_some(mesh)
    }

    /// GAP-255: an unchanged view does not reproject the terrain. The mesh is projected
    /// when the surface first appears, held through a hundred frames whose surface, view
    /// and rect are the same (the same `Arc`, not an equal copy), reprojected when any one
    /// of the three changes, and dropped when the surface goes. What it holds is what
    /// `terrain_mesh_2d` would project this frame, so caching changes nothing drawn.
    #[test]
    fn the_terrain_mesh_is_projected_only_when_its_inputs_change() {
        let palette = theme::Palette::day();
        let positions = grid(20, 24, rolling, |_, _| false);
        let surface = terrain_surface(&palette, layer_of(&positions, 20, 24)).expect("a surface");
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(800.0, 600.0));
        let view = TopDownView {
            center_e: 120.0,
            center_n: -40.0,
            meters_per_px: 7.5,
        };
        let held = |slot: &Option<TerrainMesh2d>| {
            std::sync::Arc::clone(&slot.as_ref().expect("a mesh is held").mesh)
        };

        let mut slot = None;
        assert!(
            sync_terrain_mesh_2d(&mut slot, Some(&surface), &view, rect),
            "the first frame projects"
        );
        let first = held(&slot);
        assert_eq!(*first, terrain_mesh_2d(rect, &view, &surface));
        for _ in 0..100 {
            assert!(
                !sync_terrain_mesh_2d(&mut slot, Some(&surface), &view, rect),
                "an unchanged frame reprojected"
            );
        }
        assert!(std::sync::Arc::ptr_eq(&first, &held(&slot)));

        let panned = TopDownView {
            center_e: 500.0,
            ..view
        };
        assert!(
            sync_terrain_mesh_2d(&mut slot, Some(&surface), &panned, rect),
            "a pan"
        );
        assert_eq!(*held(&slot), terrain_mesh_2d(rect, &panned, &surface));
        let zoomed = TopDownView {
            meters_per_px: 15.0,
            ..panned
        };
        assert!(
            sync_terrain_mesh_2d(&mut slot, Some(&surface), &zoomed, rect),
            "a zoom"
        );
        let resized = rect.expand(40.0);
        assert!(
            sync_terrain_mesh_2d(&mut slot, Some(&surface), &zoomed, resized),
            "a resize"
        );
        let reloaded = TerrainSurface {
            revision: 4,
            ..surface.clone()
        };
        assert!(
            sync_terrain_mesh_2d(&mut slot, Some(&reloaded), &zoomed, resized),
            "a new load"
        );
        assert_eq!(held(&slot).vertices.len(), reloaded.positions.len());
        assert!(!sync_terrain_mesh_2d(
            &mut slot,
            Some(&reloaded),
            &zoomed,
            resized
        ));
        assert!(
            !sync_terrain_mesh_2d(&mut slot, None, &zoomed, resized),
            "no terrain projects nothing"
        );
        assert!(slot.is_none(), "no terrain drops the mesh");
    }

    /// A grid of `rows` by `columns` 30 m cells, north row first, its heights from
    /// `height(east, north)`, and `NaN` at every cell `hole` names.
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    fn grid(
        rows: u32,
        columns: u32,
        height: impl Fn(f32, f32) -> f32,
        hole: impl Fn(u32, u32) -> bool,
    ) -> Vec<[f32; 3]> {
        let mut positions = Vec::with_capacity((rows * columns) as usize);
        for r in 0..rows {
            for c in 0..columns {
                let (e, n) = (
                    c as f32 * 30.0 - 900.0,
                    (rows - 1 - r) as f32 * 30.0 - 600.0,
                );
                let z = if hole(r, c) { f32::NAN } else { height(e, n) };
                positions.push([e, n, z]);
            }
        }
        positions
    }

    fn rolling(e: f32, n: f32) -> f32 {
        120.0 * (e / 400.0).sin() * (n / 550.0).cos() + 40.0 * (e / 130.0 + n / 170.0).sin() + 200.0
    }

    fn layer_of(positions: &[[f32; 3]], rows: u32, columns: u32) -> TerrainLayer<'_> {
        TerrainLayer {
            positions,
            rows,
            columns,
            revision: 3,
        }
    }

    /// GAP-252, DN-35 §8.1 row a as amendment 1 restates it: over a small grid, a grid past
    /// the vertex budget and a grid with no-data holes, the mesh the projection now builds
    /// from the surface is today's vertex for vertex and index for index, and, over flat
    /// ground, colour for colour. Elsewhere each colour is today's lit by a bounded shade,
    /// with today's alpha.
    #[test]
    fn the_surface_is_the_projections_ground() {
        let palette = theme::Palette::day();
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(800.0, 600.0));
        let view = TopDownView {
            center_e: 120.0,
            center_n: -40.0,
            meters_per_px: 7.5,
        };
        let holes = |r: u32, c: u32| (r == 4 && c == 6) || (r == 10 && (8..11).contains(&c));
        let no_hole = |_: u32, _: u32| false;
        let cases: [(&str, u32, u32, bool); 3] = [
            ("small", 9, 12, false),
            ("past the budget", 150, 140, false),
            ("with holes", 20, 24, true),
        ];
        for (name, rows, columns, with_holes) in cases {
            assert_eq!(
                terrain_stride(rows, columns) > 1,
                name == "past the budget",
                "{name}: the cases must cover a stride of one and above one"
            );
            for flat in [false, true] {
                let height = |e: f32, n: f32| if flat { 150.0 } else { rolling(e, n) };
                let positions = if with_holes {
                    grid(rows, columns, height, holes)
                } else {
                    grid(rows, columns, height, no_hole)
                };
                let layer = layer_of(&positions, rows, columns);
                let oracle = oracle_terrain_mesh(&palette, rect, &view, layer).expect("drawn");
                let surface = terrain_surface(&palette, layer).expect("a surface");
                assert_eq!(surface.revision, 3);
                let mesh = terrain_mesh_2d(rect, &view, &surface);
                assert_eq!(mesh.indices, oracle.indices, "{name}: the triangles");
                assert_eq!(mesh.vertices.len(), oracle.vertices.len(), "{name}");
                for (drawn, today) in mesh.vertices.iter().zip(&oracle.vertices) {
                    assert_eq!(drawn.pos, today.pos, "{name}: a vertex moved");
                    assert_eq!(drawn.uv, today.uv, "{name}");
                    if flat {
                        assert_eq!(
                            drawn.color, today.color,
                            "{name}: flat ground is today's colour"
                        );
                        continue;
                    }
                    let [r, g, b, a] = drawn.color.to_srgba_unmultiplied();
                    let [tr, tg, tb, ta] = today.color.to_srgba_unmultiplied();
                    assert_eq!(a, ta, "{name}: the alpha is terrain_alpha");
                    for (lit, ramp) in [(r, tr), (g, tg), (b, tb)] {
                        let (lit, ramp) = (f64::from(lit), f64::from(ramp));
                        assert!(
                            lit >= ramp * HILLSHADE_DARKEST - 2.0
                                && lit <= ramp * HILLSHADE_BRIGHTEST + 2.0,
                            "{name}: {lit} is not {ramp} lit within the shade's bounds"
                        );
                    }
                }
            }
        }
    }

    /// The surface refuses by name what it cannot draw, before anything reaches a GPU.
    #[test]
    fn a_surface_that_cannot_be_drawn_is_refused_by_name() {
        let palette = theme::Palette::day();
        let positions = grid(4, 5, rolling, |_, _| false);
        assert!(matches!(
            terrain_surface(&palette, layer_of(&positions[..19], 4, 5)),
            Err(crate::ViewportError::MalformedMesh { .. })
        ));
        let empty = grid(4, 5, rolling, |_, _| true);
        assert_eq!(
            terrain_surface(&palette, layer_of(&empty, 4, 5)),
            Err(crate::ViewportError::EmptyMesh)
        );
        let checkered = grid(4, 5, rolling, |r, c| (r + c) % 2 == 0);
        assert_eq!(
            terrain_surface(&palette, layer_of(&checkered, 4, 5)),
            Err(crate::ViewportError::EmptyMesh),
            "no cell has four corners"
        );
        assert_eq!(
            terrain_surface(&palette, layer_of(&positions[..5], 1, 5)),
            Err(crate::ViewportError::EmptyMesh)
        );
    }

    /// GAP-252, DN-35 §8.1 row b: over a hundred frames at one revision the surface is
    /// built once, a new revision rebuilds it once, and no terrain drops it. A refusal is
    /// held too, so a grid that cannot be drawn is not refused again every frame.
    #[test]
    fn the_surface_is_built_once_per_load() {
        let palette = theme::Palette::day();
        let positions = grid(30, 40, rolling, |_, _| false);
        let mut slot = None;
        let mut layer = layer_of(&positions, 30, 40);
        let builds = (0..100)
            .filter(|_| sync_terrain_surface(&palette, &mut slot, Some(layer)))
            .count();
        assert_eq!(builds, 1, "a hundred frames at one revision");
        layer.revision = 4;
        let builds = (0..100)
            .filter(|_| sync_terrain_surface(&palette, &mut slot, Some(layer)))
            .count();
        assert_eq!(builds, 1, "a new revision");
        assert_eq!(slot.as_ref().map(|s| s.revision), Some(4));
        assert!(!sync_terrain_surface(&palette, &mut slot, None));
        assert!(slot.is_none(), "no terrain drops the surface");

        let empty = grid(4, 5, rolling, |_, _| true);
        let refused = layer_of(&empty, 4, 5);
        let builds = (0..100)
            .filter(|_| sync_terrain_surface(&palette, &mut slot, Some(refused)))
            .count();
        assert_eq!(builds, 1, "a refusal is held, not retried every frame");
        assert!(slot.is_some_and(|s| s.surface.is_err()));
    }

    /// GAP-252, D-157 (DN-35 amendment 1 item 1): flat ground shades to exactly one; a
    /// slope toward the north-west light is lighter and one away darker; and the factor
    /// never leaves its bounds however steep the ground.
    #[test]
    #[allow(clippy::float_cmp)]
    fn the_hillshade_is_one_on_flat_ground_and_bounded_on_any_slope() {
        assert_eq!(
            hillshade(0.0, 0.0),
            1.0,
            "flat ground keeps today's colour exactly"
        );
        // Ground rising to the south-east faces the north-west light.
        let toward = hillshade(0.15, -0.15);
        let away = hillshade(-0.15, 0.15);
        assert!(toward > 1.0, "{toward}");
        assert!(away < 1.0, "{away}");
        // A slope along the light's own line is the one most lit; across it, less.
        assert!(hillshade(0.1, -0.1) > hillshade(0.1, 0.1));
        for (de, dn) in [(40.0, -40.0), (-40.0, 40.0), (1e6, 0.0), (f64::NAN, 0.0)] {
            let shade = hillshade(de, dn);
            assert!(
                (HILLSHADE_DARKEST..=HILLSHADE_BRIGHTEST).contains(&shade),
                "({de}, {dn}): {shade}"
            );
        }
        // Most lit where the ground faces the light square on: a 45 degree slope.
        assert_eq!(hillshade(0.7, -0.7), HILLSHADE_BRIGHTEST);
        assert_eq!(hillshade(-0.5, 0.5), HILLSHADE_DARKEST);
        assert_eq!(hillshade(-40.0, 40.0), HILLSHADE_DARKEST);
        const {
            assert!(
                HILLSHADE_DARKEST > 0.0 && HILLSHADE_BRIGHTEST * 176.0 < 255.0,
                "never black or white"
            );
        }
    }

    /// The shade is read from the surface's own geometry: a uniform slope facing the light
    /// is lit uniformly, every vertex alike, and a no-data hole in it changes no other
    /// vertex's colour, because a kept neighbour's one-sided difference finds the same
    /// slope. The light comes from the north-west, so the slope rising to the south-east
    /// is lighter than the flat ramp colour.
    #[test]
    fn a_hole_changes_no_other_vertexs_shade() {
        let palette = theme::Palette::day();
        // Binary fractions, so every height and difference is exact in `f32`.
        let slope = |e: f32, n: f32| 0.25 * e - 0.125 * n + 400.0;
        let whole = grid(12, 14, slope, |_, _| false);
        // The hole is inside the grid, away from the lowest and highest corners, so the
        // ramp's ends do not move with it.
        let holed = grid(12, 14, slope, |r, c| r == 5 && c == 6);
        let colours =
            |positions: &[[f32; 3]]| -> std::collections::HashMap<[u32; 2], egui::Color32> {
                let surface =
                    terrain_surface(&palette, layer_of(positions, 12, 14)).expect("a surface");
                surface
                    .positions
                    .iter()
                    .zip(&surface.colors)
                    .map(|(p, c)| ([p[0].to_bits(), p[1].to_bits()], *c))
                    .collect()
            };
        let (whole, holed) = (colours(&whole), colours(&holed));
        assert_eq!(holed.len(), whole.len() - 1);
        for (at, colour) in &holed {
            assert_eq!(whole.get(at), Some(colour), "the vertex at {at:?} changed");
        }
        let shade = hillshade(0.25, -0.125);
        assert!(shade > 1.0, "{shade}");
        assert!(
            whole
                .values()
                .all(|c| c.to_srgba_unmultiplied()[0] > terrain_rgb(0.0)[0]),
            "every vertex of the slope is lit, the lowest corner included"
        );
    }

    /// GAP-252 (DN-35 amendment 1 item 1), beside `gungnir-ui`'s
    /// `a_dead_spot_darkens_the_terrain_under_it_but_does_not_hide_it`, whose ground is the
    /// unshaded ramp's two ends: over the lightest and darkest shaded ground a dead spot
    /// still at least halves the brightness, and a dead spot over the lightest ground is
    /// darker than the darkest ground with none, so a shadow is never read as a valley.
    #[test]
    fn a_dead_spot_stays_darker_than_any_shaded_ground() {
        let over = |top: [f64; 3], alpha: f64, under: [f64; 3]| {
            [0, 1, 2].map(|i| top[i] * alpha + under[i] * (1.0 - alpha))
        };
        let rgb = |c: egui::Color32| [f64::from(c.r()), f64::from(c.g()), f64::from(c.b())];
        let luma = |c: [f64; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        for palette in [theme::Palette::day(), theme::Palette::night()] {
            let terrain_alpha = f64::from(palette.terrain_alpha) / 255.0;
            let shadow_alpha = f64::from(palette.shadow_alpha) / 255.0;
            let ground = |t: f32, shade: f64| {
                let lit = shaded(&palette, t, shade).to_srgba_unmultiplied();
                over(
                    [f64::from(lit[0]), f64::from(lit[1]), f64::from(lit[2])],
                    terrain_alpha,
                    rgb(palette.viewport_background),
                )
            };
            let darkest = ground(0.0, HILLSHADE_DARKEST);
            let lightest = ground(1.0, HILLSHADE_BRIGHTEST);
            for lit in [darkest, lightest] {
                let dark = over(rgb(palette.shadow_color), shadow_alpha, lit);
                assert!(
                    luma(dark) < 0.5 * luma(lit),
                    "a dead spot must at least halve the ground's brightness: {} to {}",
                    luma(lit),
                    luma(dark)
                );
            }
            let shadow_on_lightest = over(rgb(palette.shadow_color), shadow_alpha, lightest);
            assert!(
                luma(shadow_on_lightest) < luma(darkest),
                "a dead spot over the lightest ground ({}) must be darker than the darkest \
                 ground with none ({})",
                luma(shadow_on_lightest),
                luma(darkest)
            );
        }
    }

    /// A small cloud is drawn whole; a large one is decimated to the budget, never
    /// truncated to whichever points happened to load first.
    #[test]
    fn a_point_cloud_is_decimated_by_stride_to_the_vertex_budget() {
        assert_eq!(point_cloud_stride(100), 1);
        assert_eq!(point_cloud_stride(POINT_CLOUD_VERTEX_BUDGET), 1);
        let count = POINT_CLOUD_VERTEX_BUDGET * 10 + 3;
        let stride = point_cloud_stride(count);
        assert!(stride >= 10, "{stride}");
        let kept = count.div_ceil(stride);
        assert!(kept <= POINT_CLOUD_VERTEX_BUDGET, "{kept}");
        assert_eq!(point_cloud_stride(0), 1);
    }

    /// The source and target of a pair must not be drawn the same colour, or a loaded
    /// pair would look like one undifferentiated cloud rather than two.
    #[test]
    fn source_and_target_are_drawn_in_different_colours() {
        let palette = theme::Palette::day();
        assert_ne!(
            point_cloud_color(&palette, 0),
            point_cloud_color(&palette, 1)
        );
        // Anything past the pair repeats the target's colour rather than panicking or
        // indexing a fixed palette out of bounds.
        assert_eq!(
            point_cloud_color(&palette, 1),
            point_cloud_color(&palette, 2)
        );
    }

    /// [`PointCloudLayer`] borrows straight from a loaded buffer's positions, the same
    /// way [`TerrainLayer`] borrows a mesh's -- constructing one is exercised here since
    /// the crate this borrows from (`gungnir-data`) is a dependency this one already has,
    /// and an actual GL draw cannot be asserted without a window.
    #[test]
    fn a_point_cloud_layer_borrows_a_buffers_positions_without_copying_them() {
        let buffer = gungnir_data::pointcloud::PointBuffer {
            positions: vec![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]],
            ..gungnir_data::pointcloud::PointBuffer::default()
        };
        let layer = PointCloudLayer {
            positions: &buffer.positions,
        };
        assert_eq!(layer.positions.len(), 2);
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(layer.positions[1], [4.0, 5.0, 6.0]);
        }
        assert!(std::ptr::eq(
            layer.positions.as_ptr(),
            buffer.positions.as_ptr()
        ));
    }

    fn circle(sensor: u32, radius_m: f64, confidence: f32) -> CoverageCircle {
        CoverageCircle {
            sensor,
            center: [0.0, 0.0, 0.0],
            radius_m,
            confidence,
            sector: None,
        }
    }

    /// GAP-118: a sectored region is drawn as a wedge from the sensor out to its range,
    /// between its edges and across north when it straddles it; a full circle stays a ring.
    #[test]
    fn a_sector_is_outlined_as_a_wedge_between_its_edges() {
        let mut c = circle(1, 1_000.0, 1.0);
        c.center = [100.0, -50.0, 0.0];
        assert!(sector_outline(&c, 96).is_none(), "no sector is a ring");
        c.sector = Some(
            gungnir_model::AzimuthSector::new(350_f64.to_radians(), 40_f64.to_radians())
                .expect("legal"),
        );
        let outline = sector_outline(&c, 96).expect("a wedge");
        assert_eq!(outline.first(), Some(&c.center));
        assert_eq!(outline.last(), Some(&c.center));
        let arc = &outline[1..outline.len() - 1];
        let bearing = |p: &[f64; 3]| {
            gungnir_model::bearing_rad(p[0] - c.center[0], p[1] - c.center[1])
                .expect("off centre")
                .to_degrees()
        };
        assert!((bearing(&arc[0]) - 330.0).abs() < 1e-9, "starts at 330");
        assert!(
            (bearing(arc.last().expect("an end")) - 10.0).abs() < 1e-9,
            "ends at 10"
        );
        for p in arc {
            let r = ((p[0] - c.center[0]).powi(2) + (p[1] - c.center[1]).powi(2)).sqrt();
            assert!((r - 1_000.0).abs() < 1e-6, "on the range");
            let b = bearing(p);
            assert!(
                !(10.0 + 1e-9..330.0 - 1e-9).contains(&b),
                "{b} is outside the sector"
            );
        }
        c.sector =
            Some(gungnir_model::AzimuthSector::new(0.0, std::f64::consts::TAU).expect("legal"));
        assert!(
            sector_outline(&c, 96).is_none(),
            "a stated full circle is a ring"
        );
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
        let palette = theme::Palette::day();
        let strong = coverage_color(&palette, 1.0);
        let weak = coverage_color(&palette, 0.7);
        let none = coverage_color(&palette, 0.0);
        assert_eq!(strong.a(), palette.coverage_max_alpha);
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
        let palette = theme::Palette::day();
        assert_eq!(
            coverage_color(&palette, 5.0).a(),
            palette.coverage_max_alpha
        );
        assert_eq!(coverage_color(&palette, -1.0).a(), 0);
        assert_eq!(
            coverage_color(&palette, f32::NAN).a(),
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

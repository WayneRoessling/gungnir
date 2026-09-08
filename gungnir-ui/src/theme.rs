// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Colors, spacing, fonts -- centralized, no magic numbers in panels
//! (rust-ui-architecture-coding-standards.md §6). `gungnir-viewport3d::materials`
//! reads the same palette so 2D and 3D views agree.
//!
//! Two kinds of thing live here, and the distinction matters. The **tokens** are
//! [`Palette`]'s fields: every colour, size and stroke width the panels, the viewport
//! and the dock draw with, named by the role the value plays rather than by the widget
//! that first needed it. The **installer**, [`install_egui_theme`], hands egui's own
//! chrome -- panel fills, widget states, selection, text styles -- the same tokens, so
//! the frame around the picture is the same operations-room surface as the picture
//! itself (`docs/ux/design-system.md` DS-07). Before the installer existed the panels
//! drew their colours on egui's stock dark grey, which is why
//! `docs/ux/accessibility.md` now names [`Palette::panel_background`] as the surface
//! text is checked against.
//!
//! # Day and night (D-35, DS-07; GAP-095)
//!
//! [`Palette`] carries two values, [`Palette::day`] and [`Palette::night`]: the night
//! variant scales the chrome's surface, text and interaction hues to 70% of their day
//! relative luminance, the grid darker still, with `alert_color` pinned identical
//! across both (D-35's three clauses, exactly). Everything else DS-01 lists --
//! lifecycle, classification, coverage, hazard, selection, geometry -- is outside the
//! scope either document names and stays identical to day.
//!
//! Which variant is in force is a `ConfigBaseline` setting a deployment picks at
//! start-up (`gungnir_model::ThemeVariant`, resolved through [`Palette::for_variant`]),
//! **never a per-session runtime toggle**: D-35 requires that a shift in the picture's
//! colours never be a mid-session surprise. This module holds no global state for it
//! either way -- `rust-ui-architecture-coding-standards.md` §2 forbids a global
//! mutable static -- so the resolved [`Palette`] is threaded explicitly as a parameter
//! from `gungnir-app`'s start-up down through every function that used to read one of
//! these as a bare constant. There is consequently no code path by which a panel could
//! reach a different [`Palette`] than the one the session started with.
//!
//! What is deliberately *not* here: the word for a status (`gungnir_model::Vocabulary`
//! owns it, GAP-070), the association-confidence margin (`gungnir-policy` owns it and
//! [`frame_is_dashed`] takes it as an argument), and any rule that decides what a track
//! *is*. This crate depends on `gungnir-model` alone (ARCHITECTURE.md §4), so a
//! threshold that is policy cannot live here and a threshold that lives here is
//! presentation: [`Palette::time_remaining_warn_s`] says when a number turns amber, not
//! when a plan expires.

use egui::epaint::Shadow;
use egui::{
    Color32, Context, FontFamily, FontId, Margin, RichText, Rounding, Stroke, Style, TextStyle,
    Vec2, Visuals,
};
use gungnir_model::TrackStatus;

/// Every DS-01 token, for one colour variant (D-35, DS-07; GAP-095).
///
/// Two values exist -- [`Palette::day`] and [`Palette::night`] -- and a deployment
/// picks one through `ConfigBaseline`'s `ui.theme` at start-up, resolved by
/// [`Palette::for_variant`]. See the module documentation for why this is threaded
/// explicitly rather than held globally.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    // -----------------------------------------------------------------------
    // Geometry and typography (DS-01, DS-05, DS-06). Identical in both variants:
    // only colour is a function of day/night.
    // -----------------------------------------------------------------------
    /// Gap between panels and between rows of controls.
    pub panel_spacing: f32,
    /// Vertical gap between items inside a panel: half the panel gap, so a panel
    /// reads as one block with rows in it rather than as a stack of separate blocks.
    pub row_spacing: f32,
    /// Anything a decision is made on (DS-05).
    pub default_font_size: f32,
    /// Provenance, history, footnotes (DS-05).
    pub small_font_size: f32,
    /// A panel's title and nothing else (DS-05).
    pub title_font_size: f32,
    pub track_table_max_height: f32,
    pub status_dot_radius: f32,
    /// Starting width of the docked workspace beside the viewport.
    pub dashboard_default_width: f32,
    /// Height of the dock's tab bar (`gungnir-app/src/dock.rs`).
    pub tab_bar_height: f32,
    /// Grid lines, coverage rings and the uncertainty ellipse are context and stay
    /// hairline.
    pub stroke_hairline: f32,
    /// A heading vector, a classification frame, a fence and a warning are the
    /// picture and get emphasis.
    pub stroke_emphasis: f32,
    /// A hazard is a survey fact drawn heavier than either so it is never mistaken
    /// for a track's frame (DN-14).
    pub stroke_hazard: f32,
    /// A coverage gap that is partly covered (DN-12): the uncovered one below is
    /// drawn heavier because it is the one the operator acts on.
    pub stroke_gap_partial: f32,
    pub stroke_gap_uncovered: f32,

    // -----------------------------------------------------------------------
    // Surfaces, text and interaction: the chrome (DS-07). The tokens the night
    // variant scales to 70% of their day relative luminance.
    // -----------------------------------------------------------------------
    /// The window behind everything: the darkest surface, blue-black rather than
    /// neutral so the cool track, coverage and assignment colours sit in it instead
    /// of on it.
    pub app_background: Color32,
    /// A docked panel, a window, a menu. One step up from the app so a panel has an
    /// edge without a drawn border having to supply it.
    pub panel_background: Color32,
    /// A widget under the pointer.
    pub panel_background_hover: Color32,
    /// A widget being pressed, or a menu that is open.
    pub panel_background_active: Color32,
    /// Hairline borders and separators.
    pub border_subtle: Color32,
    /// Body text on a panel.
    pub text_primary: Color32,
    /// Secondary text: labels, provenance, a value that is present but not the
    /// point.
    pub text_secondary: Color32,
    /// Keyboard focus, hover strokes, the text cursor, the drag preview in the dock.
    /// Chrome only: it is never drawn inside the viewport, which is why it may sit
    /// near the friendly-frame blue without the two competing.
    pub focus_color: Color32,
    /// Opacity of the selection fill behind selected text and rows. An opacity, not
    /// a hue, so the night variant does not scale it.
    pub focus_fill_alpha: u8,

    // -----------------------------------------------------------------------
    // Track lifecycle and health (DS-01, DS-02). Identical in both variants except
    // `alert_color`, which D-35 pins to identical too -- it just says so explicitly.
    // -----------------------------------------------------------------------
    pub track_confirmed_color: Color32,
    pub track_coasting_color: Color32,
    /// A deleted track, which the picture shows for at most one tracker step before
    /// the lifecycle manager drops it (`gungnir-track::lifecycle`); after that it is
    /// history only. Neutral grey, and *not* the stale grey: stale means the track
    /// may still exist and its last report is old; deleted means the tracker has
    /// removed it. Those are different facts and principle 3 (`docs/ux/README.md`)
    /// says they must look different.
    pub track_deleted_color: Color32,
    /// Any stale track, regardless of status; a cool grey so it reads as faded
    /// rather than as a fourth lifecycle state.
    pub track_stale_color: Color32,
    /// Warnings, cautions, a tentative fact: amber (DS-02).
    pub warning_color: Color32,
    /// Critical alerts, a denied verdict, a false health flag, weapons free: red is
    /// reserved for "something needs a person now" (DS-02). **Fixed across both
    /// variants (D-35)**: a critical must read the same regardless of which is in
    /// force.
    pub alert_color: Color32,

    // -----------------------------------------------------------------------
    // The viewport (DS-01, DS-04, DS-07). Identical in both variants except the
    // grid, which the night variant scales darker still than the chrome.
    // -----------------------------------------------------------------------
    /// The map surface. Slightly darker than the panels so the picture is the
    /// lowest thing on screen and every glyph is drawn *up* out of it.
    pub viewport_background: Color32,
    /// Range rings and grid lines: a faint blue cast, there to organise the space
    /// and not to be looked at. **Darker still at night** (DS-07), not merely the
    /// chrome's 70%.
    pub viewport_grid_color: Color32,
    /// Range labels, coordinate readouts and the viewport's status line.
    pub viewport_text_color: Color32,
    /// Assignment lines from a resource to its track, and the pre-delegated verdict
    /// chip (DS-02).
    pub intercept_line_color: Color32,
    /// Affiliation colours (DS-03).
    pub class_hostile_color: Color32,
    pub class_friendly_color: Color32,
    pub class_neutral_color: Color32,
    pub class_unknown_color: Color32,
    /// Sensor coverage rings on the map (GAP-007). Opaque, with the transparency in
    /// [`Palette::coverage_max_alpha`] beside it; see that field's own documentation
    /// for why the base colour stays opaque.
    pub coverage_color: Color32,
    /// Opacity of a full-confidence coverage ring.
    pub coverage_max_alpha: u8,
    /// Opacity of the shaded terrain under the picture (GAP-023).
    pub terrain_alpha: u8,
    /// Static hazards and barriers (DN-14, GAP-017).
    pub hazard_color: Color32,
    /// The selection halo around a glyph: white, per DS-04.
    pub selection_halo_color: Color32,
    /// A laydown option previewed on the map (GAP-087).
    pub laydown_preview_color: Color32,
    /// The source cloud of a registration pair (GAP-098).
    pub point_cloud_source_color: Color32,
    /// The target cloud of the same pair (GAP-098).
    pub point_cloud_target_color: Color32,
    /// A bearing-only detection's ray (DN-27 §7, GAP-096).
    pub bearing_ray_color: Color32,
    /// Seconds of time remaining at which a decision surface warns, and at which it
    /// becomes critical (DS-05).
    pub time_remaining_warn_s: f32,
    pub time_remaining_critical_s: f32,
}

/// D-35, DS-07: the chrome's surface, text and interaction hues are the same hues at
/// 70 percent of their day relative luminance at night.
const NIGHT_CHROME_LUMINANCE_FACTOR: f64 = 0.7;

/// DS-07: the grid is "darker still" than the chrome at night. Rather than invent a
/// second, unrelated number, this applies the same 70 percent factor a second time
/// (49 percent of day), so the grid is a further step down from the chrome instead of
/// an independent choice.
const NIGHT_GRID_LUMINANCE_FACTOR: f64 =
    NIGHT_CHROME_LUMINANCE_FACTOR * NIGHT_CHROME_LUMINANCE_FACTOR;

impl Palette {
    /// The default palette: DS-01's values, unchanged.
    #[must_use]
    pub const fn day() -> Self {
        Self {
            panel_spacing: 8.0,
            row_spacing: 4.0,
            default_font_size: 14.0,
            small_font_size: 12.0,
            title_font_size: 16.0,
            track_table_max_height: 320.0,
            status_dot_radius: 5.0,
            dashboard_default_width: 420.0,
            tab_bar_height: 22.0,
            stroke_hairline: 1.0,
            stroke_emphasis: 1.5,
            stroke_hazard: 2.5,
            stroke_gap_partial: 2.0,
            stroke_gap_uncovered: 3.0,

            app_background: Color32::from_rgb(7, 14, 21),
            panel_background: Color32::from_rgb(11, 23, 33),
            panel_background_hover: Color32::from_rgb(16, 34, 47),
            panel_background_active: Color32::from_rgb(18, 48, 63),
            border_subtle: Color32::from_rgb(28, 51, 66),
            text_primary: Color32::from_rgb(220, 233, 241),
            text_secondary: Color32::from_rgb(145, 169, 183),
            focus_color: Color32::from_rgb(57, 198, 232),
            focus_fill_alpha: 72,

            track_confirmed_color: Color32::from_rgb(60, 200, 90),
            track_coasting_color: Color32::from_rgb(200, 100, 40),
            track_deleted_color: Color32::from_rgb(130, 130, 135),
            track_stale_color: Color32::from_rgb(140, 140, 160),
            warning_color: Color32::from_rgb(220, 190, 60),
            alert_color: Color32::from_rgb(240, 80, 90),

            viewport_background: Color32::from_rgb(8, 16, 24),
            viewport_grid_color: Color32::from_rgb(31, 56, 70),
            viewport_text_color: Color32::from_rgb(205, 222, 232),
            intercept_line_color: Color32::from_rgb(70, 200, 200),
            class_hostile_color: Color32::from_rgb(230, 80, 80),
            class_friendly_color: Color32::from_rgb(90, 170, 240),
            class_neutral_color: Color32::from_rgb(100, 200, 120),
            class_unknown_color: Color32::from_rgb(230, 200, 90),
            coverage_color: Color32::from_rgb(73, 132, 172),
            coverage_max_alpha: 80,
            terrain_alpha: 110,
            hazard_color: Color32::from_rgb(190, 120, 220),
            selection_halo_color: Color32::from_rgb(240, 240, 240),
            laydown_preview_color: Color32::from_rgb(230, 100, 200),
            point_cloud_source_color: Color32::from_rgb(224, 168, 62),
            point_cloud_target_color: Color32::from_rgb(120, 140, 220),
            bearing_ray_color: Color32::from_rgb(210, 150, 40),
            time_remaining_warn_s: 30.0,
            time_remaining_critical_s: 10.0,
        }
    }

    /// The night variant (D-35, DS-07): the chrome's surface, text and interaction
    /// hues at 70% of their day relative luminance, the grid darker still, and
    /// `alert_color` pinned -- D-35's three clauses, exactly. Everything else DS-01
    /// lists (lifecycle, classification, coverage, hazard, selection, geometry) is
    /// outside the scope either document names, so it is copied from day unchanged.
    #[must_use]
    pub fn night() -> Self {
        let day = Self::day();
        Self {
            app_background: scale_luminance(day.app_background, NIGHT_CHROME_LUMINANCE_FACTOR),
            panel_background: scale_luminance(day.panel_background, NIGHT_CHROME_LUMINANCE_FACTOR),
            panel_background_hover: scale_luminance(
                day.panel_background_hover,
                NIGHT_CHROME_LUMINANCE_FACTOR,
            ),
            panel_background_active: scale_luminance(
                day.panel_background_active,
                NIGHT_CHROME_LUMINANCE_FACTOR,
            ),
            border_subtle: scale_luminance(day.border_subtle, NIGHT_CHROME_LUMINANCE_FACTOR),
            text_primary: scale_luminance(day.text_primary, NIGHT_CHROME_LUMINANCE_FACTOR),
            text_secondary: scale_luminance(day.text_secondary, NIGHT_CHROME_LUMINANCE_FACTOR),
            focus_color: scale_luminance(day.focus_color, NIGHT_CHROME_LUMINANCE_FACTOR),
            viewport_grid_color: scale_luminance(
                day.viewport_grid_color,
                NIGHT_GRID_LUMINANCE_FACTOR,
            ),
            ..day
        }
    }

    /// The palette a `ConfigBaseline`'s resolved [`gungnir_model::ThemeVariant`]
    /// selects. The one place this crate needs to know that type exists.
    #[must_use]
    pub fn for_variant(variant: gungnir_model::ThemeVariant) -> Self {
        match variant {
            gungnir_model::ThemeVariant::Day => Self::day(),
            gungnir_model::ThemeVariant::Night => Self::night(),
        }
    }

    /// = [`Palette::text_secondary`]; the older name, kept because a hundred call
    /// sites read it. One value, two names, and the alias is a method deriving from
    /// the one field so they cannot drift.
    #[must_use]
    pub fn muted_text_color(&self) -> Color32 {
        self.text_secondary
    }

    /// = [`Palette::track_confirmed_color`]: the same green as a confirmed track,
    /// because health and lifecycle never share a channel -- health is a dot or a
    /// chip, lifecycle is a glyph fill -- and principle 4 says health has exactly two
    /// states, so a third green would be a lie.
    #[must_use]
    pub fn healthy_color(&self) -> Color32 {
        self.track_confirmed_color
    }

    /// = [`Palette::alert_color`]: a false health flag is the alert red, aliased so
    /// the health channel stays two colours.
    #[must_use]
    pub fn degraded_color(&self) -> Color32 {
        self.alert_color
    }

    /// = [`Palette::warning_color`]: a tentative track shares the warning amber on
    /// purpose (DS-02) -- both mean "not yet settled".
    #[must_use]
    pub fn track_tentative_color(&self) -> Color32 {
        self.warning_color
    }
}

impl Default for Palette {
    /// Day: the palette in force until a `ConfigBaseline` says otherwise.
    fn default() -> Self {
        Self::day()
    }
}

/// Colour for a track by lifecycle status; stale tracks are always drawn muted
/// regardless of status (docs/gungnir-capabilities.md §5.2).
#[must_use]
pub fn track_color(palette: &Palette, status: TrackStatus, stale: bool) -> Color32 {
    if stale {
        return palette.track_stale_color;
    }
    match status {
        TrackStatus::Confirmed => palette.track_confirmed_color,
        TrackStatus::Tentative => palette.track_tentative_color(),
        TrackStatus::Coasting => palette.track_coasting_color,
        TrackStatus::Deleted => palette.track_deleted_color,
    }
}

// The word for a status is not here: `gungnir_model::Vocabulary` owns it since GAP-070,
// because a deployment may rename it and two sources for one label drift apart.

/// A number an operator compares with another: time remaining, a score, a range, a
/// speed. Set in the monospace face at body size, because egui's proportional face has
/// no tabular figures and DS-05 asks that compared numbers line up column under column.
#[must_use]
pub fn numeral(palette: &Palette, text: impl Into<String>) -> RichText {
    RichText::new(text).font(FontId::monospace(palette.default_font_size))
}

// ---------------------------------------------------------------------------
// Classification frames and the decision palette (GAP-073, docs/ux/design-system.md
// DS-03 and "Proposed theme changes").
// ---------------------------------------------------------------------------

/// The frame shape drawn around a track glyph.
///
/// Shape *and* colour together, following the public military symbology convention,
/// so that colour is never the only cue (DS-03 and `docs/ux/accessibility.md`: an
/// operator with a colour-vision deficiency still reads affiliation from the shape).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClassificationFrame {
    Diamond,
    RoundedRect,
    Square,
    Quatrefoil,
}

/// Frame colour for an affiliation. Identical in both variants: classification is
/// outside the scope D-35's night transform names.
#[must_use]
pub fn classification_color(palette: &Palette, c: gungnir_model::Classification) -> Color32 {
    use gungnir_model::Classification;
    match c {
        Classification::Hostile => palette.class_hostile_color,
        Classification::Friendly => palette.class_friendly_color,
        Classification::Neutral => palette.class_neutral_color,
        Classification::Unknown => palette.class_unknown_color,
    }
}

/// The shape's name in words, for the evidence card and for anyone reading the screen
/// aloud. `RoundedRect` is a Rust identifier; "rounded rectangle" is a shape.
#[must_use]
pub fn frame_label(frame: ClassificationFrame) -> &'static str {
    match frame {
        ClassificationFrame::Diamond => "diamond",
        ClassificationFrame::RoundedRect => "rounded rectangle",
        ClassificationFrame::Square => "square",
        ClassificationFrame::Quatrefoil => "quatrefoil",
    }
}

/// Frame shape for an affiliation (DS-03).
#[must_use]
pub fn classification_frame(c: gungnir_model::Classification) -> ClassificationFrame {
    use gungnir_model::Classification;
    match c {
        Classification::Hostile => ClassificationFrame::Diamond,
        Classification::Friendly => ClassificationFrame::RoundedRect,
        Classification::Neutral => ClassificationFrame::Square,
        Classification::Unknown => ClassificationFrame::Quatrefoil,
    }
}

/// Whether the frame is drawn dashed: association confidence below the policy margin.
///
/// DS-03: "Confidence below the policy margin draws the frame dashed." The margin is a
/// policy value, so it is passed in rather than assumed here; `gungnir-policy` owns it.
#[must_use]
pub fn frame_is_dashed(association_confidence: f32, policy_margin: f32) -> bool {
    association_confidence < policy_margin
}

// ---------------------------------------------------------------------------
// The installer: egui's own chrome drawn with the tokens above (DS-06, DS-07)
// ---------------------------------------------------------------------------

/// egui's `Visuals` with every surface, stroke and state colour taken from `palette`.
///
/// Starts from `Visuals::dark()` so anything this does not name keeps a sensible dark
/// default, then replaces what an operator sees: no rounded corners and no shadows,
/// because a console is edges and surfaces rather than cards floating over a page;
/// striped rows, because a twelve-row queue is read across; hover and press in the
/// focus colour so the pointer's target is always the same colour wherever it is.
#[must_use]
pub fn operations_visuals(palette: &Palette) -> Visuals {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = palette.panel_background;
    visuals.window_fill = palette.panel_background;
    visuals.extreme_bg_color = palette.app_background;
    visuals.faint_bg_color = palette.app_background;
    visuals.code_bg_color = palette.app_background;
    visuals.window_stroke = Stroke::new(palette.stroke_hairline, palette.border_subtle);
    visuals.window_rounding = Rounding::ZERO;
    visuals.menu_rounding = Rounding::ZERO;
    visuals.window_shadow = Shadow::NONE;
    visuals.popup_shadow = Shadow::NONE;
    visuals.striped = true;
    visuals.hyperlink_color = palette.focus_color;
    visuals.warn_fg_color = palette.warning_color;
    visuals.error_fg_color = palette.alert_color;
    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(
        palette.focus_color.r(),
        palette.focus_color.g(),
        palette.focus_color.b(),
        palette.focus_fill_alpha,
    );
    visuals.selection.stroke = Stroke::new(palette.stroke_hairline, palette.focus_color);

    let w = &mut visuals.widgets;
    w.noninteractive.bg_fill = palette.panel_background;
    w.noninteractive.weak_bg_fill = palette.panel_background;
    w.noninteractive.bg_stroke = Stroke::new(palette.stroke_hairline, palette.border_subtle);
    w.noninteractive.fg_stroke = Stroke::new(palette.stroke_hairline, palette.text_primary);
    w.inactive.bg_fill = palette.panel_background_hover;
    w.inactive.weak_bg_fill = palette.panel_background;
    w.inactive.bg_stroke = Stroke::new(palette.stroke_hairline, palette.border_subtle);
    w.inactive.fg_stroke = Stroke::new(palette.stroke_hairline, palette.text_primary);
    w.hovered.bg_fill = palette.panel_background_hover;
    w.hovered.weak_bg_fill = palette.panel_background_hover;
    w.hovered.bg_stroke = Stroke::new(palette.stroke_hairline, palette.focus_color);
    w.hovered.fg_stroke = Stroke::new(palette.stroke_hairline, palette.text_primary);
    w.active.bg_fill = palette.panel_background_active;
    w.active.weak_bg_fill = palette.panel_background_active;
    w.active.bg_stroke = Stroke::new(palette.stroke_emphasis, palette.focus_color);
    w.active.fg_stroke = Stroke::new(palette.stroke_hairline, palette.text_primary);
    w.open.bg_fill = palette.panel_background_active;
    w.open.weak_bg_fill = palette.panel_background_active;
    w.open.bg_stroke = Stroke::new(palette.stroke_hairline, palette.border_subtle);
    w.open.fg_stroke = Stroke::new(palette.stroke_hairline, palette.text_primary);
    for state in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        state.rounding = Rounding::ZERO;
        state.expansion = 0.0;
    }
    visuals
}

/// Install the operations theme on a context. Called once, from the eframe creation
/// closure in `gungnir-app/src/main.rs` and from the headless render probe, so the
/// desktop and the tests lay out the same style. `palette` is the value resolved from
/// `ConfigBaseline` at start-up (day or night, D-35); it is not read from anywhere
/// else, and nothing after this call may install a different one for the same session.
///
/// Beyond [`operations_visuals`]: spacing from the tokens, the DS-05 text styles (body
/// and buttons at 14, provenance at 12, a panel title at 16, the monospace face at body
/// size for [`numeral`]), and `animation_time` at zero because DS-06 allows no motion
/// but the strip's counts and the queue's reorder, and egui's default eases collapsing
/// headers and toggles over a tenth of a second.
pub fn install_egui_theme(ctx: &Context, palette: &Palette) {
    let mut style: Style = (*ctx.style()).clone();
    style.visuals = operations_visuals(palette);
    style.animation_time = 0.0;
    style.spacing.item_spacing = Vec2::new(palette.panel_spacing, palette.row_spacing);
    style.spacing.button_padding = Vec2::new(palette.panel_spacing, palette.row_spacing);
    style.spacing.window_margin = Margin::same(palette.panel_spacing);
    style.text_styles = [
        (
            TextStyle::Heading,
            FontId::new(palette.title_font_size, FontFamily::Proportional),
        ),
        (
            TextStyle::Body,
            FontId::new(palette.default_font_size, FontFamily::Proportional),
        ),
        (
            TextStyle::Button,
            FontId::new(palette.default_font_size, FontFamily::Proportional),
        ),
        (
            TextStyle::Small,
            FontId::new(palette.small_font_size, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(palette.default_font_size, FontFamily::Monospace),
        ),
    ]
    .into();
    ctx.set_style(style);
}

// ---------------------------------------------------------------------------
// Contrast (docs/ux/accessibility.md)
// ---------------------------------------------------------------------------

/// sRGB channel (0-255) to linear light (0.0-1.0): the decode half of the pair with
/// [`linear_to_srgb`], per WCAG 2.1's relative luminance definition.
fn srgb_to_linear(v: u8) -> f64 {
    let s = f64::from(v) / 255.0;
    if s <= 0.039_28 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear light (0.0-1.0) back to an sRGB channel (0-255): the encode half of the pair
/// with [`srgb_to_linear`]. Out-of-range input is clamped rather than wrapped.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn linear_to_srgb(l: f64) -> u8 {
    let l = l.clamp(0.0, 1.0);
    let s = if l <= 0.003_130_8 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Relative luminance per WCAG 2.1, for the contrast rule in
/// `docs/ux/accessibility.md`.
#[must_use]
pub fn relative_luminance(c: Color32) -> f64 {
    0.2126 * srgb_to_linear(c.r()) + 0.7152 * srgb_to_linear(c.g()) + 0.0722 * srgb_to_linear(c.b())
}

/// Contrast ratio between two colours, 1.0 to 21.0 (WCAG 2.1).
///
/// `docs/ux/accessibility.md` requires at least 4.5:1 for 14 pt text and 3:1 for the
/// 12 pt provenance text against the surface it is drawn on. The theme's own tests
/// hold the palette to it against every surface, so a colour cannot be changed to
/// something unreadable without a test failing.
#[must_use]
pub fn contrast_ratio(a: Color32, b: Color32) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (lighter, darker) = if la >= lb { (la, lb) } else { (lb, la) };
    (lighter + 0.05) / (darker + 0.05)
}

/// Scales a colour to `factor` times its own WCAG relative luminance while keeping its
/// hue: decode to linear light, scale, re-encode. Relative luminance is a linear
/// combination of the *linear* channels, so scaling every linear channel by the same
/// factor scales the result by exactly that factor -- which is what "70 percent
/// luminance" (DS-07) means precisely, rather than the different, darker-still result
/// naively scaling the encoded sRGB bytes by 0.7 would give.
fn scale_luminance(c: Color32, factor: f64) -> Color32 {
    Color32::from_rgb(
        linear_to_srgb(srgb_to_linear(c.r()) * factor),
        linear_to_srgb(srgb_to_linear(c.g()) * factor),
        linear_to_srgb(srgb_to_linear(c.b()) * factor),
    )
}

#[cfg(test)]
mod classification_tests {
    use super::*;
    use gungnir_model::Classification;

    const ALL: [Classification; 4] = [
        Classification::Hostile,
        Classification::Friendly,
        Classification::Neutral,
        Classification::Unknown,
    ];

    /// Shape and colour must both be distinct, because DS-03's whole point is that
    /// colour is never the only cue. Two affiliations sharing a shape would put the
    /// entire distinction on colour for an operator who cannot see the difference.
    /// Classification is outside the night transform's scope, so day alone proves it.
    #[test]
    fn every_affiliation_has_a_distinct_shape_and_colour() {
        let day = Palette::day();
        let mut shapes = std::collections::HashSet::new();
        let mut colours = std::collections::HashSet::new();
        for c in ALL {
            assert!(
                shapes.insert(classification_frame(c)),
                "{c:?} shares a frame shape with another affiliation"
            );
            let col = classification_color(&day, c);
            assert!(
                colours.insert((col.r(), col.g(), col.b())),
                "{c:?} shares a frame colour with another affiliation"
            );
        }
    }

    /// The accessibility rule, enforced rather than asserted: every classification
    /// colour must reach 4.5:1 against the viewport background it is drawn on, in
    /// both variants (classification colours do not change, but the check is run
    /// against each palette's own values on principle, not assumed to hold).
    #[test]
    fn classification_colours_meet_the_contrast_rule() {
        for palette in [Palette::day(), Palette::night()] {
            for c in ALL {
                let ratio = contrast_ratio(
                    classification_color(&palette, c),
                    palette.viewport_background,
                );
                assert!(
                    ratio >= 4.5,
                    "{c:?} has contrast {ratio:.2}:1 against the viewport background, \
                     below the 4.5:1 that docs/ux/accessibility.md requires"
                );
            }
        }
    }

    /// The warning and selection colours are read as text or as a halo over the same
    /// background and are held to the same rule, in both variants.
    #[test]
    fn warning_and_selection_colours_meet_the_contrast_rule() {
        for palette in [Palette::day(), Palette::night()] {
            for (name, colour) in [
                ("warning_color", palette.warning_color),
                ("selection_halo_color", palette.selection_halo_color),
            ] {
                let ratio = contrast_ratio(colour, palette.viewport_background);
                assert!(
                    ratio >= 4.5,
                    "{name} has contrast {ratio:.2}:1, below 4.5:1"
                );
            }
        }
    }

    /// A known pair, so the contrast function itself is checked rather than trusted:
    /// black on white is the maximum 21:1, and a colour against itself is 1:1.
    #[test]
    fn contrast_ratio_is_calibrated() {
        let white = Color32::from_rgb(255, 255, 255);
        let black = Color32::from_rgb(0, 0, 0);
        assert!((contrast_ratio(white, black) - 21.0).abs() < 0.01);
        assert!((contrast_ratio(white, white) - 1.0).abs() < 1e-9);
    }

    /// The dashed-frame rule reads a policy margin rather than a constant, because the
    /// margin belongs to `gungnir-policy` and differs per deployment.
    #[test]
    fn the_dashed_frame_follows_the_policy_margin() {
        assert!(frame_is_dashed(0.4, 0.7), "below the margin is dashed");
        assert!(!frame_is_dashed(0.9, 0.7), "above the margin is solid");
        assert!(!frame_is_dashed(0.7, 0.7), "at the margin is solid");
    }

    /// The two time-remaining thresholds must be ordered, or a decision surface would
    /// go critical before it warned. Was a `const` compile-time assertion while these
    /// were bare constants; now that they are resolved `Palette` fields (read from
    /// `ConfigBaseline` in principle, though this token in particular never varies by
    /// variant) the same check runs at test time instead, against both values.
    #[test]
    fn time_remaining_thresholds_are_ordered() {
        for palette in [Palette::day(), Palette::night()] {
            assert!(palette.time_remaining_critical_s < palette.time_remaining_warn_s);
            assert!(palette.time_remaining_critical_s > 0.0);
        }
    }
}

#[cfg(test)]
mod surface_tests {
    use super::*;

    /// Every surface a colour can be text on, and every colour that is drawn as
    /// 14 pt text somewhere: the status words in the table, the health chips, the
    /// alert rows, the hazard labels, the verdict chip. Each must reach 4.5:1 on
    /// every surface, because a chip that is readable on the map and not on a panel
    /// is readable in the place it is not drawn. Run for both palettes: night's
    /// darker chrome must not have quietly dropped a pair below the rule.
    #[test]
    fn every_text_colour_meets_the_rule_on_every_surface() {
        for palette in [Palette::day(), Palette::night()] {
            let surfaces = [
                ("viewport_background", palette.viewport_background),
                ("panel_background", palette.panel_background),
                ("app_background", palette.app_background),
            ];
            let text_colours = [
                ("text_primary", palette.text_primary),
                ("text_secondary", palette.text_secondary),
                ("viewport_text_color", palette.viewport_text_color),
                ("alert_color", palette.alert_color),
                ("healthy_color", palette.healthy_color()),
                ("warning_color", palette.warning_color),
                ("hazard_color", palette.hazard_color),
                ("intercept_line_color", palette.intercept_line_color),
                ("track_confirmed_color", palette.track_confirmed_color),
                ("track_tentative_color", palette.track_tentative_color()),
                ("track_coasting_color", palette.track_coasting_color),
                ("track_deleted_color", palette.track_deleted_color),
                ("track_stale_color", palette.track_stale_color),
                ("class_hostile_color", palette.class_hostile_color),
                ("class_friendly_color", palette.class_friendly_color),
            ];
            for (surface_name, surface) in surfaces {
                for (name, colour) in text_colours {
                    let ratio = contrast_ratio(colour, surface);
                    assert!(
                        ratio >= 4.5,
                        "{name} has contrast {ratio:.2}:1 on {surface_name}, below the \
                         4.5:1 that docs/ux/accessibility.md requires for 14 pt text"
                    );
                }
            }
        }
    }

    /// The surfaces are ordered: the map is the lowest thing on screen, panels sit
    /// one step above the window. A panel darker than the map would make the picture
    /// float. Checked for both palettes: `viewport_background` does not scale at
    /// night while the others do, so this is not automatic and is verified rather
    /// than assumed.
    #[test]
    fn surfaces_are_layered_darkest_to_lightest() {
        for palette in [Palette::day(), Palette::night()] {
            let l = relative_luminance;
            assert!(l(palette.app_background) < l(palette.panel_background));
            assert!(l(palette.viewport_background) < l(palette.panel_background));
            assert!(l(palette.panel_background) < l(palette.panel_background_hover));
            assert!(l(palette.panel_background_hover) < l(palette.panel_background_active));
            assert!(
                l(palette.border_subtle) > l(palette.panel_background),
                "a border must be visible"
            );
        }
    }

    /// The five lifecycle colours are five facts. Deleted and stale in particular:
    /// `track_color` once mapped `Deleted` to the stale grey, which told an operator a
    /// removed track might still be there. Lifecycle is outside the night transform's
    /// scope, so day alone proves the set is distinct.
    #[test]
    fn lifecycle_colours_are_distinct() {
        let day = Palette::day();
        let values = [
            ("confirmed", day.track_confirmed_color),
            ("tentative", day.track_tentative_color()),
            ("coasting", day.track_coasting_color),
            ("deleted", day.track_deleted_color),
            ("stale", day.track_stale_color),
        ];
        let mut seen = std::collections::HashSet::new();
        for (name, c) in values {
            assert!(
                seen.insert((c.r(), c.g(), c.b())),
                "{name} shares its colour with another lifecycle state"
            );
        }
        assert_ne!(
            track_color(&day, TrackStatus::Deleted, false),
            track_color(&day, TrackStatus::Confirmed, true),
            "a deleted track must not be drawn as a stale one"
        );
    }

    /// The lines and halos that share the map must be tellable apart at a glance: the
    /// sum of channel differences is a crude distance, but a pair under sixty is a pair
    /// that once existed (the assignment line and the friendly frame were ten apart).
    /// None of this family is in the night transform's scope, so day alone proves it.
    #[test]
    fn viewport_line_colours_are_pairwise_distinct() {
        let day = Palette::day();
        let family = [
            ("intercept_line_color", day.intercept_line_color),
            ("class_friendly_color", day.class_friendly_color),
            ("class_neutral_color", day.class_neutral_color),
            ("coverage_color", day.coverage_color),
            ("selection_halo_color", day.selection_halo_color),
            ("laydown_preview_color", day.laydown_preview_color),
            ("bearing_ray_color", day.bearing_ray_color),
        ];
        for (i, (a_name, a)) in family.iter().enumerate() {
            for (b_name, b) in &family[i + 1..] {
                let distance = i32::from(a.r()).abs_diff(i32::from(b.r()))
                    + i32::from(a.g()).abs_diff(i32::from(b.g()))
                    + i32::from(a.b()).abs_diff(i32::from(b.b()));
                assert!(
                    distance >= 60,
                    "{a_name} and {b_name} are {distance} apart, too close to tell apart on the map"
                );
            }
        }
    }

    /// The aliases are aliases: the design system says health is two colours and a
    /// tentative track is amber, and this test pins the sharing so a later edit that
    /// separates one of them has to say so here. Checked on both palettes, though as
    /// derived methods rather than independent constants they cannot drift by
    /// construction.
    #[test]
    fn deliberate_aliases_hold() {
        for palette in [Palette::day(), Palette::night()] {
            assert_eq!(palette.healthy_color(), palette.track_confirmed_color);
            assert_eq!(palette.degraded_color(), palette.alert_color);
            assert_eq!(palette.track_tentative_color(), palette.warning_color);
            assert_eq!(palette.muted_text_color(), palette.text_secondary);
        }
    }

    /// Installing the theme changes the context's style: the surfaces, the text
    /// styles and the animation time. Read back rather than assumed, because
    /// `set_style` on a cloned `Style` is the kind of call that silently does nothing
    /// if the clone is the one that gets edited.
    #[test]
    fn the_installer_lands_on_the_context() {
        let palette = Palette::day();
        let ctx = Context::default();
        install_egui_theme(&ctx, &palette);
        let style = ctx.style();
        assert_eq!(style.visuals.panel_fill, palette.panel_background);
        assert_eq!(style.visuals.extreme_bg_color, palette.app_background);
        assert_eq!(style.visuals.window_rounding, Rounding::ZERO);
        assert_eq!(style.visuals.window_shadow, Shadow::NONE);
        assert!(
            style.animation_time.abs() < f32::EPSILON,
            "DS-06: no motion"
        );
        assert!(style.visuals.striped);
        let body = &style.text_styles[&TextStyle::Body];
        assert!((body.size - palette.default_font_size).abs() < f32::EPSILON);
        let mono = &style.text_styles[&TextStyle::Monospace];
        assert_eq!(mono.family, FontFamily::Monospace);
        assert!((mono.size - palette.default_font_size).abs() < f32::EPSILON);
        let small = &style.text_styles[&TextStyle::Small];
        assert!((small.size - palette.small_font_size).abs() < f32::EPSILON);
    }

    /// GAP-095's own proof that the wiring works end to end: the installer draws
    /// whichever palette it is handed, not always day. If this ever failed it would
    /// mean a deployment's `ui.theme: "night"` setting had no visible effect.
    #[test]
    fn the_installer_reflects_whichever_palette_it_is_given() {
        let night = Palette::night();
        let ctx = Context::default();
        install_egui_theme(&ctx, &night);
        let style = ctx.style();
        assert_eq!(style.visuals.panel_fill, night.panel_background);
        assert_eq!(style.visuals.extreme_bg_color, night.app_background);
        assert_eq!(style.visuals.error_fg_color, night.alert_color);
        assert_ne!(
            style.visuals.panel_fill,
            Palette::day().panel_background,
            "the installed panel fill should be night's, not day's"
        );
    }

    /// Coverage sits behind the picture: its full-confidence opacity must leave a
    /// track drawn over it clearly on top, in both variants (the alphas do not vary
    /// by theme, but the check is run against each palette's own values).
    #[test]
    fn coverage_is_translucent() {
        for palette in [Palette::day(), Palette::night()] {
            assert!(palette.coverage_max_alpha < 128);
            assert!(palette.terrain_alpha < 160);
        }
    }
}

/// D-35, DS-07, GAP-095: what the night variant actually changes, and what it must
/// leave alone. These are the tests the gap's action names directly: the two variants
/// differ exactly where D-35 says they should, `alert_color` and everything else DS-01
/// lists outside that scope are pinned identical, and `for_variant`/`Default` resolve
/// the way `ConfigBaseline::theme_variant` expects.
#[cfg(test)]
mod palette_variant_tests {
    use super::*;

    /// The eight chrome hues (DS-01's "Surfaces, text and interaction" table) are
    /// each at exactly 70% of their day relative luminance at night -- not merely
    /// darker, but darker by the specific factor DS-07 names.
    #[test]
    fn night_scales_the_chrome_hues_to_70_percent_luminance() {
        let day = Palette::day();
        let night = Palette::night();
        for (name, day_c, night_c) in [
            ("app_background", day.app_background, night.app_background),
            (
                "panel_background",
                day.panel_background,
                night.panel_background,
            ),
            (
                "panel_background_hover",
                day.panel_background_hover,
                night.panel_background_hover,
            ),
            (
                "panel_background_active",
                day.panel_background_active,
                night.panel_background_active,
            ),
            ("border_subtle", day.border_subtle, night.border_subtle),
            ("text_primary", day.text_primary, night.text_primary),
            ("text_secondary", day.text_secondary, night.text_secondary),
            ("focus_color", day.focus_color, night.focus_color),
        ] {
            assert_ne!(day_c, night_c, "{name} should differ between day and night");
            let day_l = relative_luminance(day_c);
            let night_l = relative_luminance(night_c);
            assert!(
                night_l < day_l,
                "{name}: night ({night_l:.4}) should be darker than day ({day_l:.4})"
            );
            assert!(
                (night_l - day_l * NIGHT_CHROME_LUMINANCE_FACTOR).abs() < 0.01,
                "{name}: night luminance {night_l:.4} is not ~70% of day's {day_l:.4}"
            );
        }
    }

    /// DS-07's "grid darker still": the grid differs, is darker than day, and is
    /// darker than the plain 70% the chrome gets -- a further step down, not a second
    /// unrelated number.
    #[test]
    fn night_grid_is_darker_than_the_chrome_scaling() {
        let day = Palette::day();
        let night = Palette::night();
        assert_ne!(day.viewport_grid_color, night.viewport_grid_color);
        let day_l = relative_luminance(day.viewport_grid_color);
        let night_l = relative_luminance(night.viewport_grid_color);
        assert!(night_l < day_l * NIGHT_CHROME_LUMINANCE_FACTOR,
            "the grid ({night_l:.4}) should be darker than the chrome's plain 70% scaling would give ({:.4})",
            day_l * NIGHT_CHROME_LUMINANCE_FACTOR);
        assert!(
            (night_l - day_l * NIGHT_GRID_LUMINANCE_FACTOR).abs() < 0.01,
            "night grid luminance {night_l:.4} is not ~49% of day's {day_l:.4}"
        );
    }

    /// D-35's pin, tested directly: `alert_color` (and its `degraded_color` alias)
    /// must be byte-identical between the two variants. This is the one property the
    /// gap's action calls out by name.
    #[test]
    fn alert_color_never_varies() {
        let day = Palette::day();
        let night = Palette::night();
        assert_eq!(
            day.alert_color, night.alert_color,
            "D-35: ALERT_COLOR must stay fixed across both variants"
        );
        assert_eq!(day.degraded_color(), night.degraded_color());
    }

    /// Everything DS-01 lists outside the chrome-and-grid scope D-35's night variant
    /// touches: lifecycle, classification, coverage, hazard, selection, the laydown
    /// preview, the alphas, the thresholds, and every geometry/typography token.
    /// Pinned identical so a future change to the night constructor cannot silently
    /// widen its own scope without a test noticing.
    #[test]
    // These fields are copied verbatim from `day` to `night` through the `..day`
    // struct update in `Palette::night` (no arithmetic in between), so bit-for-bit
    // equality is the correct check, not an approximation clippy should warn about.
    #[allow(clippy::float_cmp)]
    fn everything_outside_the_named_scope_is_identical() {
        let day = Palette::day();
        let night = Palette::night();

        // Track lifecycle and health, apart from alert_color above.
        assert_eq!(day.track_confirmed_color, night.track_confirmed_color);
        assert_eq!(day.track_coasting_color, night.track_coasting_color);
        assert_eq!(day.track_deleted_color, night.track_deleted_color);
        assert_eq!(day.track_stale_color, night.track_stale_color);
        assert_eq!(day.warning_color, night.warning_color);

        // The viewport, apart from the grid.
        assert_eq!(day.viewport_background, night.viewport_background);
        assert_eq!(day.viewport_text_color, night.viewport_text_color);
        assert_eq!(day.intercept_line_color, night.intercept_line_color);
        assert_eq!(day.class_hostile_color, night.class_hostile_color);
        assert_eq!(day.class_friendly_color, night.class_friendly_color);
        assert_eq!(day.class_neutral_color, night.class_neutral_color);
        assert_eq!(day.class_unknown_color, night.class_unknown_color);
        assert_eq!(day.coverage_color, night.coverage_color);
        assert_eq!(day.coverage_max_alpha, night.coverage_max_alpha);
        assert_eq!(day.terrain_alpha, night.terrain_alpha);
        assert_eq!(day.hazard_color, night.hazard_color);
        assert_eq!(day.selection_halo_color, night.selection_halo_color);
        assert_eq!(day.laydown_preview_color, night.laydown_preview_color);
        assert_eq!(day.point_cloud_source_color, night.point_cloud_source_color);
        assert_eq!(day.point_cloud_target_color, night.point_cloud_target_color);
        assert_eq!(day.bearing_ray_color, night.bearing_ray_color);
        assert_eq!(day.time_remaining_warn_s, night.time_remaining_warn_s);
        assert_eq!(
            day.time_remaining_critical_s,
            night.time_remaining_critical_s
        );

        // An opacity, not a hue.
        assert_eq!(day.focus_fill_alpha, night.focus_fill_alpha);

        // Geometry and typography never vary by theme.
        assert_eq!(day.panel_spacing, night.panel_spacing);
        assert_eq!(day.row_spacing, night.row_spacing);
        assert_eq!(day.default_font_size, night.default_font_size);
        assert_eq!(day.small_font_size, night.small_font_size);
        assert_eq!(day.title_font_size, night.title_font_size);
        assert_eq!(day.track_table_max_height, night.track_table_max_height);
        assert_eq!(day.status_dot_radius, night.status_dot_radius);
        assert_eq!(day.dashboard_default_width, night.dashboard_default_width);
        assert_eq!(day.tab_bar_height, night.tab_bar_height);
        assert_eq!(day.stroke_hairline, night.stroke_hairline);
        assert_eq!(day.stroke_emphasis, night.stroke_emphasis);
        assert_eq!(day.stroke_hazard, night.stroke_hazard);
        assert_eq!(day.stroke_gap_partial, night.stroke_gap_partial);
        assert_eq!(day.stroke_gap_uncovered, night.stroke_gap_uncovered);
    }

    /// `ConfigBaseline::theme_variant` resolves to `gungnir_model::ThemeVariant`;
    /// this is the other half, proving `for_variant` picks the matching `Palette` for
    /// each of the two values that type can hold.
    #[test]
    fn for_variant_selects_the_matching_palette() {
        assert_eq!(
            Palette::for_variant(gungnir_model::ThemeVariant::Day),
            Palette::day()
        );
        assert_eq!(
            Palette::for_variant(gungnir_model::ThemeVariant::Night),
            Palette::night()
        );
    }

    #[test]
    fn the_default_palette_is_day() {
        assert_eq!(Palette::default(), Palette::day());
    }
}

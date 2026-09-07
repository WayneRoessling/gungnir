// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Colors, spacing, fonts -- centralized, no magic numbers in panels
//! (rust-ui-architecture-coding-standards.md §6). `gungnir-viewport3d::materials`
//! reads the same palette so 2D and 3D views agree.
//!
//! Two kinds of thing live here, and the distinction matters. The **tokens** are the
//! constants: every colour, size and stroke width the panels, the viewport and the dock
//! draw with, named by the role the value plays rather than by the widget that first
//! needed it. The **installer**, [`install_egui_theme`], hands egui's own chrome -- panel
//! fills, widget states, selection, text styles -- the same tokens, so the frame around
//! the picture is the same operations-room surface as the picture itself
//! (`docs/ux/design-system.md` DS-07). Before the installer existed the panels drew
//! their colours on egui's stock dark grey, which is why `docs/ux/accessibility.md`
//! now names [`PANEL_BACKGROUND`] as the surface text is checked against.
//!
//! What is deliberately *not* here: the word for a status (`gungnir_model::Vocabulary`
//! owns it, GAP-070), the association-confidence margin (`gungnir-policy` owns it and
//! [`frame_is_dashed`] takes it as an argument), and any rule that decides what a track
//! *is*. This crate depends on `gungnir-model` alone (ARCHITECTURE.md §4), so a
//! threshold that is policy cannot live here and a threshold that lives here is
//! presentation: [`TIME_REMAINING_WARN_S`] says when a number turns amber, not when a
//! plan expires.

use egui::epaint::Shadow;
use egui::{
    Color32, Context, FontFamily, FontId, Margin, RichText, Rounding, Stroke, Style, TextStyle,
    Vec2, Visuals,
};
use gungnir_model::TrackStatus;

// ---------------------------------------------------------------------------
// Geometry and typography (DS-01, DS-05, DS-06)
// ---------------------------------------------------------------------------

/// Gap between panels and between rows of controls.
pub const PANEL_SPACING: f32 = 8.0;
/// Vertical gap between items inside a panel: half the panel gap, so a panel reads as
/// one block with rows in it rather than as a stack of separate blocks.
pub const ROW_SPACING: f32 = 4.0;
/// Anything a decision is made on (DS-05).
pub const DEFAULT_FONT_SIZE: f32 = 14.0;
/// Provenance, history, footnotes (DS-05).
pub const SMALL_FONT_SIZE: f32 = 12.0;
/// A panel's title and nothing else (DS-05). Two points over body is enough to find
/// the panel in a dock of six without making the title the largest thing on a
/// decision surface.
pub const TITLE_FONT_SIZE: f32 = 16.0;
pub const TRACK_TABLE_MAX_HEIGHT: f32 = 320.0;
pub const STATUS_DOT_RADIUS: f32 = 5.0;
/// Starting width of the docked workspace beside the viewport.
pub const DASHBOARD_DEFAULT_WIDTH: f32 = 420.0;
/// Height of the dock's tab bar (`gungnir-app/src/dock.rs`).
pub const TAB_BAR_HEIGHT: f32 = 22.0;

/// Stroke widths, by role. Grid lines, coverage rings and the uncertainty ellipse are
/// context and stay hairline; a heading vector, a classification frame, a fence and a
/// warning are the picture and get emphasis; a hazard is a survey fact drawn heavier
/// than either so it is never mistaken for a track's frame (DN-14).
pub const STROKE_HAIRLINE: f32 = 1.0;
pub const STROKE_EMPHASIS: f32 = 1.5;
pub const STROKE_HAZARD: f32 = 2.5;
/// A coverage gap that is partly covered, and one that is not covered at all (DN-12):
/// the uncovered one is drawn heavier because it is the one the operator acts on.
pub const STROKE_GAP_PARTIAL: f32 = 2.0;
pub const STROKE_GAP_UNCOVERED: f32 = 3.0;

// ---------------------------------------------------------------------------
// Surfaces, text and interaction: the chrome (DS-07)
// ---------------------------------------------------------------------------

/// The window behind everything: the darkest surface, blue-black rather than neutral
/// so the cool track, coverage and assignment colours sit in it instead of on it.
pub const APP_BACKGROUND: Color32 = Color32::from_rgb(7, 14, 21);
/// A docked panel, a window, a menu. One step up from the app so a panel has an edge
/// without a drawn border having to supply it.
pub const PANEL_BACKGROUND: Color32 = Color32::from_rgb(11, 23, 33);
/// A widget under the pointer.
pub const PANEL_BACKGROUND_HOVER: Color32 = Color32::from_rgb(16, 34, 47);
/// A widget being pressed, or a menu that is open.
pub const PANEL_BACKGROUND_ACTIVE: Color32 = Color32::from_rgb(18, 48, 63);
/// Hairline borders and separators.
pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(28, 51, 66);
/// Body text on a panel.
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(220, 233, 241);
/// Secondary text: labels, provenance, a value that is present but not the point.
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(145, 169, 183);
/// The older name for [`TEXT_SECONDARY`], kept because a hundred call sites read it.
/// One value, two names, and the alias is written as one so they cannot drift.
pub const MUTED_TEXT_COLOR: Color32 = TEXT_SECONDARY;
/// Keyboard focus, hover strokes, the text cursor, the drag preview in the dock.
/// Chrome only: it is never drawn inside the viewport, which is why it may sit near the
/// friendly-frame blue without the two competing.
pub const FOCUS_COLOR: Color32 = Color32::from_rgb(57, 198, 232);
/// Opacity of the selection fill behind selected text and rows.
pub const FOCUS_FILL_ALPHA: u8 = 72;

// ---------------------------------------------------------------------------
// Track lifecycle and health (DS-01, DS-02)
// ---------------------------------------------------------------------------

pub const TRACK_CONFIRMED_COLOR: Color32 = Color32::from_rgb(60, 200, 90);
pub const TRACK_COASTING_COLOR: Color32 = Color32::from_rgb(200, 100, 40);
/// A deleted track, which the picture shows for at most one tracker step before the
/// lifecycle manager drops it (`gungnir-track::lifecycle`); after that it is history
/// only. Neutral grey, and *not* the stale grey: stale means the track may still exist
/// and its last report is old; deleted means the tracker has removed it. Those are
/// different facts and principle 3 (`docs/ux/README.md`) says they must look different.
pub const TRACK_DELETED_COLOR: Color32 = Color32::from_rgb(130, 130, 135);
/// Any stale track, regardless of status; a cool grey so it reads as faded rather than
/// as a fourth lifecycle state.
pub const TRACK_STALE_COLOR: Color32 = Color32::from_rgb(140, 140, 160);

/// Warnings, cautions, a tentative fact: amber (DS-02).
pub const WARNING_COLOR: Color32 = Color32::from_rgb(220, 190, 60);
/// A tentative track shares the warning amber on purpose: both mean "not yet settled",
/// the glyph and the table row also carry the status word, and the fill channel never
/// carries a warning (DS-02 keeps the channels apart). Written as an alias so the
/// sharing is a decision on the page rather than two literals that happen to agree.
pub const TRACK_TENTATIVE_COLOR: Color32 = WARNING_COLOR;

/// Critical alerts, a denied verdict, a false health flag, weapons free: red is
/// reserved for "something needs a person now" (DS-02). Bright enough to hold 4.5:1
/// against the panel surface as 14 pt text, which the previous (220, 60, 60) did not.
pub const ALERT_COLOR: Color32 = Color32::from_rgb(240, 80, 90);
/// A true health flag. The same green as a confirmed track because the two never
/// share a channel -- health is a dot or a chip, lifecycle is a glyph fill -- and
/// principle 4 says health has exactly two states, so a third green would be a lie.
pub const HEALTHY_COLOR: Color32 = TRACK_CONFIRMED_COLOR;
/// A false health flag: the alert red, aliased so the health channel stays two colours.
pub const DEGRADED_COLOR: Color32 = ALERT_COLOR;

// ---------------------------------------------------------------------------
// The viewport (DS-01, DS-04, DS-07)
// ---------------------------------------------------------------------------

/// The map surface. Slightly darker than the panels so the picture is the lowest
/// thing on screen and every glyph is drawn *up* out of it.
pub const VIEWPORT_BACKGROUND: Color32 = Color32::from_rgb(8, 16, 24);
/// Range rings and grid lines: a faint blue cast, there to organise the space and not
/// to be looked at.
pub const VIEWPORT_GRID_COLOR: Color32 = Color32::from_rgb(31, 56, 70);
/// Range labels, coordinate readouts and the viewport's status line.
pub const VIEWPORT_TEXT_COLOR: Color32 = Color32::from_rgb(205, 222, 232);
/// Assignment lines from a resource to its track, and the pre-delegated verdict chip
/// (DS-02). Teal rather than blue: the previous (90, 160, 240) was ten units from the
/// friendly frame's (90, 170, 240), and a line and a frame in the same colour on the
/// same map read as one thing.
pub const INTERCEPT_LINE_COLOR: Color32 = Color32::from_rgb(70, 200, 200);

/// Colour for a track by lifecycle status; stale tracks are always drawn muted
/// regardless of status (docs/gungnir-capabilities.md §5.2).
#[must_use]
pub fn track_color(status: TrackStatus, stale: bool) -> Color32 {
    if stale {
        return TRACK_STALE_COLOR;
    }
    match status {
        TrackStatus::Confirmed => TRACK_CONFIRMED_COLOR,
        TrackStatus::Tentative => TRACK_TENTATIVE_COLOR,
        TrackStatus::Coasting => TRACK_COASTING_COLOR,
        TrackStatus::Deleted => TRACK_DELETED_COLOR,
    }
}

// The word for a status is not here: `gungnir_model::Vocabulary` owns it since GAP-070,
// because a deployment may rename it and two sources for one label drift apart.

/// A number an operator compares with another: time remaining, a score, a range, a
/// speed. Set in the monospace face at body size, because egui's proportional face has
/// no tabular figures and DS-05 asks that compared numbers line up column under column.
#[must_use]
pub fn numeral(text: impl Into<String>) -> RichText {
    RichText::new(text).font(FontId::monospace(DEFAULT_FONT_SIZE))
}

// ---------------------------------------------------------------------------
// Classification frames and the decision palette (GAP-073, docs/ux/design-system.md
// DS-03 and "Proposed theme changes").
// ---------------------------------------------------------------------------

/// Affiliation colours. Values are DS-03's proposed constants, unchanged.
pub const CLASS_HOSTILE_COLOR: Color32 = Color32::from_rgb(230, 80, 80);
pub const CLASS_FRIENDLY_COLOR: Color32 = Color32::from_rgb(90, 170, 240);
pub const CLASS_NEUTRAL_COLOR: Color32 = Color32::from_rgb(100, 200, 120);
pub const CLASS_UNKNOWN_COLOR: Color32 = Color32::from_rgb(230, 200, 90);

/// Sensor coverage rings on the map (GAP-007). Cool and low-contrast on purpose:
/// coverage is context behind the picture, and a ring that competed with a track symbol
/// would draw the eye to where the sensors are rather than to what they see.
///
/// Opaque, with the transparency in [`COVERAGE_MAX_ALPHA`] beside it. `Color32` stores
/// its channels premultiplied, so a base colour that carried its own alpha would give
/// `r()`, `g()` and `b()` that are already faded -- and fading them again by confidence
/// would shift the hue rather than the opacity. Keeping the base opaque means the
/// components mean what they say.
pub const COVERAGE_COLOR: Color32 = Color32::from_rgb(73, 132, 172);

/// Opacity of a full-confidence coverage ring. Lowered from 110 on 2026-09-06 so a ring
/// stays informative without competing with the tracks inside it.
pub const COVERAGE_MAX_ALPHA: u8 = 80;

/// Opacity of the shaded terrain under the picture (GAP-023): translucent so tracks and
/// rings stay legible over it.
pub const TERRAIN_ALPHA: u8 = 110;

/// Static hazards and barriers (DN-14, GAP-017): a boom, a net, a wreck. Neither the
/// coverage blue nor the hostile red nor the warning amber, because a hazard is none of
/// those things -- it is a survey fact, and the picture must not read it as a threat or a
/// rule.
pub const HAZARD_COLOR: Color32 = Color32::from_rgb(190, 120, 220);
/// The selection halo around a glyph: white, per DS-04. White is the one colour no
/// other viewport element uses, which is the property a halo needs.
pub const SELECTION_HALO_COLOR: Color32 = Color32::from_rgb(240, 240, 240);

/// Seconds of time remaining at which a decision surface warns, and at which it
/// becomes critical (DS-05). Read by the decision dialog and the approval queue.
pub const TIME_REMAINING_WARN_S: f32 = 30.0;
pub const TIME_REMAINING_CRITICAL_S: f32 = 10.0;

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

/// Frame colour for an affiliation.
#[must_use]
pub fn classification_color(c: gungnir_model::Classification) -> Color32 {
    use gungnir_model::Classification;
    match c {
        Classification::Hostile => CLASS_HOSTILE_COLOR,
        Classification::Friendly => CLASS_FRIENDLY_COLOR,
        Classification::Neutral => CLASS_NEUTRAL_COLOR,
        Classification::Unknown => CLASS_UNKNOWN_COLOR,
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

/// egui's `Visuals` with every surface, stroke and state colour taken from the tokens.
///
/// Starts from `Visuals::dark()` so anything this does not name keeps a sensible dark
/// default, then replaces what an operator sees: no rounded corners and no shadows,
/// because a console is edges and surfaces rather than cards floating over a page;
/// striped rows, because a twelve-row queue is read across; hover and press in the
/// focus colour so the pointer's target is always the same colour wherever it is.
#[must_use]
pub fn operations_visuals() -> Visuals {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = PANEL_BACKGROUND;
    visuals.window_fill = PANEL_BACKGROUND;
    visuals.extreme_bg_color = APP_BACKGROUND;
    visuals.faint_bg_color = APP_BACKGROUND;
    visuals.code_bg_color = APP_BACKGROUND;
    visuals.window_stroke = Stroke::new(STROKE_HAIRLINE, BORDER_SUBTLE);
    visuals.window_rounding = Rounding::ZERO;
    visuals.menu_rounding = Rounding::ZERO;
    visuals.window_shadow = Shadow::NONE;
    visuals.popup_shadow = Shadow::NONE;
    visuals.striped = true;
    visuals.hyperlink_color = FOCUS_COLOR;
    visuals.warn_fg_color = WARNING_COLOR;
    visuals.error_fg_color = ALERT_COLOR;
    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(
        FOCUS_COLOR.r(),
        FOCUS_COLOR.g(),
        FOCUS_COLOR.b(),
        FOCUS_FILL_ALPHA,
    );
    visuals.selection.stroke = Stroke::new(STROKE_HAIRLINE, FOCUS_COLOR);

    let w = &mut visuals.widgets;
    w.noninteractive.bg_fill = PANEL_BACKGROUND;
    w.noninteractive.weak_bg_fill = PANEL_BACKGROUND;
    w.noninteractive.bg_stroke = Stroke::new(STROKE_HAIRLINE, BORDER_SUBTLE);
    w.noninteractive.fg_stroke = Stroke::new(STROKE_HAIRLINE, TEXT_PRIMARY);
    w.inactive.bg_fill = PANEL_BACKGROUND_HOVER;
    w.inactive.weak_bg_fill = PANEL_BACKGROUND;
    w.inactive.bg_stroke = Stroke::new(STROKE_HAIRLINE, BORDER_SUBTLE);
    w.inactive.fg_stroke = Stroke::new(STROKE_HAIRLINE, TEXT_PRIMARY);
    w.hovered.bg_fill = PANEL_BACKGROUND_HOVER;
    w.hovered.weak_bg_fill = PANEL_BACKGROUND_HOVER;
    w.hovered.bg_stroke = Stroke::new(STROKE_HAIRLINE, FOCUS_COLOR);
    w.hovered.fg_stroke = Stroke::new(STROKE_HAIRLINE, TEXT_PRIMARY);
    w.active.bg_fill = PANEL_BACKGROUND_ACTIVE;
    w.active.weak_bg_fill = PANEL_BACKGROUND_ACTIVE;
    w.active.bg_stroke = Stroke::new(STROKE_EMPHASIS, FOCUS_COLOR);
    w.active.fg_stroke = Stroke::new(STROKE_HAIRLINE, TEXT_PRIMARY);
    w.open.bg_fill = PANEL_BACKGROUND_ACTIVE;
    w.open.weak_bg_fill = PANEL_BACKGROUND_ACTIVE;
    w.open.bg_stroke = Stroke::new(STROKE_HAIRLINE, BORDER_SUBTLE);
    w.open.fg_stroke = Stroke::new(STROKE_HAIRLINE, TEXT_PRIMARY);
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
/// desktop and the tests lay out the same style.
///
/// Beyond [`operations_visuals`]: spacing from the tokens, the DS-05 text styles (body
/// and buttons at 14, provenance at 12, a panel title at 16, the monospace face at body
/// size for [`numeral`]), and `animation_time` at zero because DS-06 allows no motion
/// but the strip's counts and the queue's reorder, and egui's default eases collapsing
/// headers and toggles over a tenth of a second.
pub fn install_egui_theme(ctx: &Context) {
    let mut style: Style = (*ctx.style()).clone();
    style.visuals = operations_visuals();
    style.animation_time = 0.0;
    style.spacing.item_spacing = Vec2::new(PANEL_SPACING, ROW_SPACING);
    style.spacing.button_padding = Vec2::new(PANEL_SPACING, ROW_SPACING);
    style.spacing.window_margin = Margin::same(PANEL_SPACING);
    style.text_styles = [
        (
            TextStyle::Heading,
            FontId::new(TITLE_FONT_SIZE, FontFamily::Proportional),
        ),
        (
            TextStyle::Body,
            FontId::new(DEFAULT_FONT_SIZE, FontFamily::Proportional),
        ),
        (
            TextStyle::Button,
            FontId::new(DEFAULT_FONT_SIZE, FontFamily::Proportional),
        ),
        (
            TextStyle::Small,
            FontId::new(SMALL_FONT_SIZE, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(DEFAULT_FONT_SIZE, FontFamily::Monospace),
        ),
    ]
    .into();
    ctx.set_style(style);
}

// ---------------------------------------------------------------------------
// Contrast (docs/ux/accessibility.md)
// ---------------------------------------------------------------------------

/// Relative luminance per WCAG 2.1, for the contrast rule in
/// `docs/ux/accessibility.md`.
#[must_use]
pub fn relative_luminance(c: Color32) -> f64 {
    fn channel(v: u8) -> f64 {
        let s = f64::from(v) / 255.0;
        if s <= 0.039_28 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
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
    #[test]
    fn every_affiliation_has_a_distinct_shape_and_colour() {
        let mut shapes = std::collections::HashSet::new();
        let mut colours = std::collections::HashSet::new();
        for c in ALL {
            assert!(
                shapes.insert(classification_frame(c)),
                "{c:?} shares a frame shape with another affiliation"
            );
            let col = classification_color(c);
            assert!(
                colours.insert((col.r(), col.g(), col.b())),
                "{c:?} shares a frame colour with another affiliation"
            );
        }
    }

    /// The accessibility rule, enforced rather than asserted: every classification
    /// colour must reach 4.5:1 against the viewport background it is drawn on.
    #[test]
    fn classification_colours_meet_the_contrast_rule() {
        for c in ALL {
            let ratio = contrast_ratio(classification_color(c), VIEWPORT_BACKGROUND);
            assert!(
                ratio >= 4.5,
                "{c:?} has contrast {ratio:.2}:1 against the viewport background, \
                 below the 4.5:1 that docs/ux/accessibility.md requires"
            );
        }
    }

    /// The warning and selection colours are read as text or as a halo over the same
    /// background and are held to the same rule.
    #[test]
    fn warning_and_selection_colours_meet_the_contrast_rule() {
        for (name, colour) in [
            ("WARNING_COLOR", WARNING_COLOR),
            ("SELECTION_HALO_COLOR", SELECTION_HALO_COLOR),
        ] {
            let ratio = contrast_ratio(colour, VIEWPORT_BACKGROUND);
            assert!(
                ratio >= 4.5,
                "{name} has contrast {ratio:.2}:1, below 4.5:1"
            );
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
    /// go critical before it warned.
    #[test]
    fn time_remaining_thresholds_are_ordered() {
        const { assert!(TIME_REMAINING_CRITICAL_S < TIME_REMAINING_WARN_S) };
        const { assert!(TIME_REMAINING_CRITICAL_S > 0.0) };
    }
}

#[cfg(test)]
mod surface_tests {
    use super::*;

    /// Every surface a colour can be text on. Before the installer existed the panels
    /// sat on egui's stock grey and nothing checked contrast there; now the surfaces
    /// are tokens and the check runs against all of them.
    const SURFACES: [(&str, Color32); 3] = [
        ("VIEWPORT_BACKGROUND", VIEWPORT_BACKGROUND),
        ("PANEL_BACKGROUND", PANEL_BACKGROUND),
        ("APP_BACKGROUND", APP_BACKGROUND),
    ];

    /// Every colour that is drawn as 14 pt text somewhere: the status words in the
    /// table, the health chips, the alert rows, the hazard labels, the verdict chip.
    /// Each must reach 4.5:1 on every surface, because a chip that is readable on the
    /// map and not on a panel is readable in the place it is not drawn.
    const TEXT_COLOURS: [(&str, Color32); 15] = [
        ("TEXT_PRIMARY", TEXT_PRIMARY),
        ("TEXT_SECONDARY", TEXT_SECONDARY),
        ("VIEWPORT_TEXT_COLOR", VIEWPORT_TEXT_COLOR),
        ("ALERT_COLOR", ALERT_COLOR),
        ("HEALTHY_COLOR", HEALTHY_COLOR),
        ("WARNING_COLOR", WARNING_COLOR),
        ("HAZARD_COLOR", HAZARD_COLOR),
        ("INTERCEPT_LINE_COLOR", INTERCEPT_LINE_COLOR),
        ("TRACK_CONFIRMED_COLOR", TRACK_CONFIRMED_COLOR),
        ("TRACK_TENTATIVE_COLOR", TRACK_TENTATIVE_COLOR),
        ("TRACK_COASTING_COLOR", TRACK_COASTING_COLOR),
        ("TRACK_DELETED_COLOR", TRACK_DELETED_COLOR),
        ("TRACK_STALE_COLOR", TRACK_STALE_COLOR),
        ("CLASS_HOSTILE_COLOR", CLASS_HOSTILE_COLOR),
        ("CLASS_FRIENDLY_COLOR", CLASS_FRIENDLY_COLOR),
    ];

    #[test]
    fn every_text_colour_meets_the_rule_on_every_surface() {
        for (surface_name, surface) in SURFACES {
            for (name, colour) in TEXT_COLOURS {
                let ratio = contrast_ratio(colour, surface);
                assert!(
                    ratio >= 4.5,
                    "{name} has contrast {ratio:.2}:1 on {surface_name}, below the 4.5:1 \
                     that docs/ux/accessibility.md requires for 14 pt text"
                );
            }
        }
    }

    /// The surfaces are ordered: the map is the lowest thing on screen, panels sit one
    /// step above the window. A panel darker than the map would make the picture float.
    #[test]
    fn surfaces_are_layered_darkest_to_lightest() {
        let l = relative_luminance;
        assert!(l(APP_BACKGROUND) < l(PANEL_BACKGROUND));
        assert!(l(VIEWPORT_BACKGROUND) < l(PANEL_BACKGROUND));
        assert!(l(PANEL_BACKGROUND) < l(PANEL_BACKGROUND_HOVER));
        assert!(l(PANEL_BACKGROUND_HOVER) < l(PANEL_BACKGROUND_ACTIVE));
        assert!(
            l(BORDER_SUBTLE) > l(PANEL_BACKGROUND),
            "a border must be visible"
        );
    }

    /// The five lifecycle colours are five facts. Deleted and stale in particular:
    /// `track_color` once mapped `Deleted` to the stale grey, which told an operator a
    /// removed track might still be there.
    #[test]
    fn lifecycle_colours_are_distinct() {
        let values = [
            ("confirmed", TRACK_CONFIRMED_COLOR),
            ("tentative", TRACK_TENTATIVE_COLOR),
            ("coasting", TRACK_COASTING_COLOR),
            ("deleted", TRACK_DELETED_COLOR),
            ("stale", TRACK_STALE_COLOR),
        ];
        let mut seen = std::collections::HashSet::new();
        for (name, c) in values {
            assert!(
                seen.insert((c.r(), c.g(), c.b())),
                "{name} shares its colour with another lifecycle state"
            );
        }
        assert_ne!(
            track_color(TrackStatus::Deleted, false),
            track_color(TrackStatus::Confirmed, true),
            "a deleted track must not be drawn as a stale one"
        );
    }

    /// The lines and halos that share the map must be tellable apart at a glance: the
    /// sum of channel differences is a crude distance, but a pair under sixty is a pair
    /// that once existed (the assignment line and the friendly frame were ten apart).
    #[test]
    fn viewport_line_colours_are_pairwise_distinct() {
        let family = [
            ("INTERCEPT_LINE_COLOR", INTERCEPT_LINE_COLOR),
            ("CLASS_FRIENDLY_COLOR", CLASS_FRIENDLY_COLOR),
            ("CLASS_NEUTRAL_COLOR", CLASS_NEUTRAL_COLOR),
            ("COVERAGE_COLOR", COVERAGE_COLOR),
            ("SELECTION_HALO_COLOR", SELECTION_HALO_COLOR),
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
    /// tentative track is amber, and the test pins the sharing so a later edit that
    /// separates one of them has to say so here.
    #[test]
    fn deliberate_aliases_hold() {
        assert_eq!(HEALTHY_COLOR, TRACK_CONFIRMED_COLOR);
        assert_eq!(DEGRADED_COLOR, ALERT_COLOR);
        assert_eq!(TRACK_TENTATIVE_COLOR, WARNING_COLOR);
        assert_eq!(MUTED_TEXT_COLOR, TEXT_SECONDARY);
    }

    /// Installing the theme changes the context's style: the surfaces, the text styles
    /// and the animation time. Read back rather than assumed, because `set_style` on a
    /// cloned `Style` is the kind of call that silently does nothing if the clone is
    /// the one that gets edited.
    #[test]
    fn the_installer_lands_on_the_context() {
        let ctx = Context::default();
        install_egui_theme(&ctx);
        let style = ctx.style();
        assert_eq!(style.visuals.panel_fill, PANEL_BACKGROUND);
        assert_eq!(style.visuals.extreme_bg_color, APP_BACKGROUND);
        assert_eq!(style.visuals.window_rounding, Rounding::ZERO);
        assert_eq!(style.visuals.window_shadow, Shadow::NONE);
        assert!(
            style.animation_time.abs() < f32::EPSILON,
            "DS-06: no motion"
        );
        assert!(style.visuals.striped);
        let body = &style.text_styles[&TextStyle::Body];
        assert!((body.size - DEFAULT_FONT_SIZE).abs() < f32::EPSILON);
        let mono = &style.text_styles[&TextStyle::Monospace];
        assert_eq!(mono.family, FontFamily::Monospace);
        assert!((mono.size - DEFAULT_FONT_SIZE).abs() < f32::EPSILON);
        let small = &style.text_styles[&TextStyle::Small];
        assert!((small.size - SMALL_FONT_SIZE).abs() < f32::EPSILON);
    }

    /// Coverage sits behind the picture: its full-confidence opacity must leave a
    /// track drawn over it clearly on top.
    #[test]
    fn coverage_is_translucent() {
        const { assert!(COVERAGE_MAX_ALPHA < 128) };
        const { assert!(TERRAIN_ALPHA < 160) };
    }
}

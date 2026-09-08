// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Track/intercept rendering. Depends on `gungnir_model::TrackView` (the service
//! facades' public type), never on any internal tracking crate.
//!
//! Glyphs are CPU-side descriptions (position, one-sigma extent, status) that both
//! the 2D fallback and, once attached, the three-d scene draw from. They are rebuilt
//! only when the track set changes (rust-ui-architecture-coding-standards.md §5).

use crate::interaction::TopDownView;
use gungnir_model::Classification;
use gungnir_model::{InterceptSolutionView, PlanView, TrackId, TrackStatus, TrackView};
use gungnir_ui::theme;
use gungnir_ui::theme::ClassificationFrame;

/// Track symbol + uncertainty extent for one track.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackGlyph {
    pub track_id: TrackId,
    pub status: TrackStatus,
    pub stale: bool,
    /// Affiliation, which decides the frame shape and colour (DS-03). The fill inside
    /// the frame stays the lifecycle colour, so a stale hostile is a diamond with a
    /// grey fill: two independent facts, two independent cues.
    pub classification: Classification,
    /// Association confidence, which decides whether the frame is drawn dashed.
    pub association_confidence: f32,
    /// ENU position, meters.
    pub position: [f64; 3],
    /// ENU velocity, m/s.
    pub velocity: [f64; 3],
    /// One-sigma position uncertainty per axis, meters.
    pub sigma: [f64; 3],
}

/// Rebuild glyphs from the current snapshot.
pub fn update_track_symbols(tracks: &[TrackView]) -> Vec<TrackGlyph> {
    tracks
        .iter()
        .map(|t| TrackGlyph {
            track_id: t.id,
            status: t.status,
            stale: t.quality.is_stale,
            classification: t.classification,
            association_confidence: t.quality.association_confidence,
            position: t.position_enu(),
            velocity: [t.state[3], t.state[4], t.state[5]],
            sigma: t.position_sigma(),
        })
        .collect()
}

/// True when the glyph set no longer matches the snapshot, so the caller rebuilds.
/// Exact float comparison is intended: any change at all means a rebuild.
#[allow(clippy::float_cmp)]
pub fn glyphs_need_rebuild(glyphs: &[TrackGlyph], tracks: &[TrackView]) -> bool {
    glyphs.len() != tracks.len()
        || glyphs.iter().zip(tracks).any(|(g, t)| {
            g.track_id != t.id
                || g.status != t.status
                || g.stale != t.quality.is_stale
                || g.position != t.position_enu()
                || g.sigma != t.position_sigma()
        })
}

/// Seconds of velocity drawn as the heading vector.
const HEADING_VECTOR_SECONDS: f64 = 10.0;
const GLYPH_RADIUS_PX: f32 = 5.0;

/// 2D fallback: circle at the position, one-sigma ellipse, heading vector, id label.
pub fn draw_glyphs_2d(
    painter: &egui::Painter,
    palette: &theme::Palette,
    rect: egui::Rect,
    view: &TopDownView,
    glyphs: &[TrackGlyph],
) {
    for g in glyphs {
        let color = theme::track_color(palette, g.status, g.stale);
        let center = view.project(g.position, rect);
        let ellipse = egui::Rect::from_center_size(
            center,
            egui::vec2(2.0 * view.px(g.sigma[0]), 2.0 * view.px(g.sigma[1])),
        );
        if ellipse.width().is_finite() && ellipse.height().is_finite() {
            painter.rect_stroke(
                ellipse,
                ellipse.width().min(ellipse.height()) / 2.0,
                egui::Stroke::new(palette.stroke_hairline, color.gamma_multiply(0.5)),
            );
        }
        draw_classification_frame(painter, palette, center, g, color);
        let head = view.project(
            [
                g.position[0] + g.velocity[0] * HEADING_VECTOR_SECONDS,
                g.position[1] + g.velocity[1] * HEADING_VECTOR_SECONDS,
                0.0,
            ],
            rect,
        );
        painter.line_segment(
            [center, head],
            egui::Stroke::new(palette.stroke_emphasis, color),
        );
        painter.text(
            center + egui::vec2(GLYPH_RADIUS_PX + 2.0, -GLYPH_RADIUS_PX),
            egui::Align2::LEFT_BOTTOM,
            g.track_id.0.to_string(),
            egui::FontId::monospace(palette.small_font_size),
            color,
        );
    }
}

/// The frame around a glyph: shape and colour from the affiliation, dashed when the
/// association confidence is below the policy margin (DS-03).
///
/// Shape carries the affiliation as well as colour, so an operator who cannot
/// distinguish the colours still reads friend from hostile. That is the reason the
/// frame is a shape at all rather than a coloured circle.
///
/// The fill colour passed in is the *lifecycle* colour, not the affiliation colour:
/// the two encode different things and DS-03 keeps them separate.
fn draw_classification_frame(
    painter: &egui::Painter,
    palette: &theme::Palette,
    center: egui::Pos2,
    glyph: &TrackGlyph,
    lifecycle_color: egui::Color32,
) {
    let frame_color = theme::classification_color(palette, glyph.classification);
    // The policy margin belongs to `gungnir-policy` and is not wired to the viewport
    // yet; until it is, every frame is solid rather than guessing a threshold and
    // drawing some tracks as low-confidence when nobody has said what low means.
    let stroke = egui::Stroke::new(palette.stroke_emphasis, frame_color);
    let r = GLYPH_RADIUS_PX;

    match theme::classification_frame(glyph.classification) {
        ClassificationFrame::Diamond => {
            let points = vec![
                center + egui::vec2(0.0, -r),
                center + egui::vec2(r, 0.0),
                center + egui::vec2(0.0, r),
                center + egui::vec2(-r, 0.0),
            ];
            painter.add(egui::Shape::closed_line(points, stroke));
        }
        ClassificationFrame::RoundedRect => {
            painter.rect_stroke(
                egui::Rect::from_center_size(center, egui::vec2(2.0 * r, 2.0 * r)),
                r * 0.5,
                stroke,
            );
        }
        ClassificationFrame::Square => {
            painter.rect_stroke(
                egui::Rect::from_center_size(center, egui::vec2(2.0 * r, 2.0 * r)),
                0.0,
                stroke,
            );
        }
        ClassificationFrame::Quatrefoil => {
            // Four lobes: four small circles on the axes. Distinct in silhouette from
            // the diamond, the square and the rounded rectangle at glyph size, which
            // is what matters -- an unknown track must not be mistaken for a hostile.
            for offset in [
                egui::vec2(0.0, -r * 0.55),
                egui::vec2(r * 0.55, 0.0),
                egui::vec2(0.0, r * 0.55),
                egui::vec2(-r * 0.55, 0.0),
            ] {
                painter.circle_stroke(center + offset, r * 0.55, stroke);
            }
        }
    }

    // The lifecycle fill sits inside the frame.
    painter.circle_filled(center, r * 0.35, lifecycle_color);
}

/// 2D fallback for the plan: label each assigned track with its resource. Resource
/// positions are geodetic and the intercept geometry is not yet computed, so the
/// pairing is shown as text at the track until `InterceptSolutionView` carries a
/// point to draw a line to.
pub fn draw_plan_2d(
    painter: &egui::Painter,
    palette: &theme::Palette,
    rect: egui::Rect,
    view: &TopDownView,
    glyphs: &[TrackGlyph],
    plan: &PlanView,
) {
    for s in plan.solutions() {
        if let Some(g) = glyphs.iter().find(|g| g.track_id == s.track) {
            let p = view.project(g.position, rect);
            painter.text(
                p + egui::vec2(GLYPH_RADIUS_PX + 2.0, GLYPH_RADIUS_PX),
                egui::Align2::LEFT_TOP,
                format!("R{}", s.resource.0),
                egui::FontId::monospace(palette.small_font_size),
                palette.intercept_line_color,
            );
        }
    }
}

/// three-d geometry per solution, built once the GL context is attached.
/// # Errors
///
/// Always: the geometry is designed and not built (GAP-082).
///
/// **A `Result` rather than an empty `Vec`**, which the scene would have drawn as a plan
/// with no intercepts in it -- indistinguishable from a plan that proposes none.
pub fn update_intercept_geometry(
    _solutions: &[InterceptSolutionView],
) -> Result<Vec<Box<dyn three_d::Object>>, crate::ViewportError> {
    Err(crate::ViewportError::NotImplemented {
        what: "three-d line geometry per intercept solution",
        waiting_on: "GAP-022, the three-d scene",
    })
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use gungnir_model::{Classification, MissionTime, Provenance, Quality};
    use nalgebra::{SMatrix, SVector};

    fn track(id: u64, e: f64) -> TrackView {
        TrackView {
            id: TrackId(id),
            status: TrackStatus::Confirmed,
            state: SVector::<f64, 6>::new(e, 0.0, 0.0, 1.0, 0.0, 0.0),
            covariance: SMatrix::<f64, 6, 6>::identity(),
            classification: Classification::Unknown,
            provenance: Provenance::default(),
            quality: Quality::default(),
            mission_time: MissionTime(0.0),
            releasability: gungnir_model::Releasability::default(),
        }
    }

    #[test]
    fn glyph_carries_position_velocity_and_sigma() {
        let glyphs = update_track_symbols(&[track(3, 7.0)]);
        assert_eq!(glyphs[0].position, [7.0, 0.0, 0.0]);
        assert_eq!(glyphs[0].velocity, [1.0, 0.0, 0.0]);
        assert_eq!(glyphs[0].sigma, [1.0, 1.0, 1.0]);
    }

    #[test]
    fn glyphs_rebuild_only_on_change() {
        let tracks = vec![track(1, 10.0)];
        let glyphs = update_track_symbols(&tracks);
        assert!(!glyphs_need_rebuild(&glyphs, &tracks));
        let moved = vec![track(1, 11.0)];
        assert!(glyphs_need_rebuild(&glyphs, &moved));
        assert!(glyphs_need_rebuild(&glyphs, &[]));
    }
}

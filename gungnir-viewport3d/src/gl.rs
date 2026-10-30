// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Attaching the three-d scene to eframe's OpenGL context (GAP-022).
//!
//! `ARCHITECTURE.md` §4 kept three-d for the viewport and §9 records the consequence:
//! the desktop has two GPU contexts, an OpenGL one for everything the operator sees and
//! a wgpu compute one for point-cloud fusion. This is the OpenGL half being connected.
//!
//! # Why the attachment is possible on the pinned set, and why that is not incidental
//!
//! `eframe` 0.34, `egui_glow` 0.34 and `three-d` 0.19 all depend on `glow` 0.17, so the
//! `Arc<glow::Context>` eframe hands out is the same type `three_d::Context` accepts.
//! Had they differed there would be two `glow`s between them and no way to pass the
//! context across. `cargo tree -i glow@0.17.0` shows the one `glow` all three share, and
//! the version set in §9 is what keeps it that way -- moving any one of them is a
//! decision about this attachment, not only about that crate. (`cargo tree -d` also
//! lists `glow` 0.13: that is `wgpu-hal` 22's, under the compute device, and never meets
//! this context.)
//!
//! # What is testable here and what is not
//!
//! Creating a context, uploading a mesh and issuing a draw call all need a live GL
//! context, which a headless test does not have. So everything that decides *what*
//! is drawn is a pure function over plain data -- [`instances`] and [`camera`] -- and is
//! tested; the GL shell around them is as thin as it can be made.
//!
//! **The drawn result has not been looked at.** No display was available where this was
//! written, and unlike the egui panels there is no headless probe for a GL draw call:
//! `RenderProbe` reads egui's shape list, and three-d does not produce one.
//!
//! This path is nonetheless the **default** (`ui.scene_3d`, on), by the owner's decision
//! of 2026-09-05. Two things carry the weight that a verified renderer would otherwise
//! have carried, and both are deliberate: the viewport's status line is painted by egui
//! and this callback draws after it without clearing anything, so a draw that produces
//! nothing still leaves the line reporting how many tracks exist -- an empty viewport
//! that says twelve tracks is a fault someone can
//! report, an empty viewport that says nothing is an operator concluding the sector is
//! clear; and the operator can switch to the projection at runtime, without editing a
//! baseline and restarting during whatever is happening at the time.

/// The OpenGL context type three-d accepts.
///
/// Re-exported so a caller can name it without depending on three-d: `gungnir-app` uses
/// it to assert at compile time that the `Arc` eframe hands out is this same type, which
/// is the fact this whole attachment rests on.
pub use three_d::context::Context as GlContext;

use crate::interaction::TopDownView;
use crate::tracks::{TrackGlyph, GLYPH_RADIUS_PX};
use gungnir_ui::theme;
use std::sync::Arc;

/// How far above the ENU origin the camera sits, in metres.
///
/// The camera is orthographic, so its height changes nothing on screen; it only has to be
/// above every track and below the far plane's reach under every one, which a track
/// within 100 km of the origin's altitude is.
const CAMERA_ALTITUDE_M: f32 = 100_000.0;

/// Why the three-d scene could not be attached.
#[derive(Debug, thiserror::Error)]
pub enum AttachError {
    /// eframe did not provide an OpenGL context. It only does so with
    /// `Renderer::Glow`; with the wgpu renderer there is nothing to attach to.
    #[error("no OpenGL context: the desktop is not running with Renderer::Glow")]
    NoGlContext,
    /// three-d refused the context.
    #[error("three-d could not use the OpenGL context: {0}")]
    Rejected(String),
}

/// The three-d side of the viewport, once attached.
///
/// Holds the context and the one instanced mesh every glyph is drawn from. Both are
/// created once: `rust-ui-architecture-coding-standards.md` §5 forbids building GPU
/// objects per frame, and an instanced mesh is the shape that lets one buffer serve
/// however many tracks there are.
pub struct SceneRenderer {
    context: three_d::Context,
    glyph_mesh: three_d::Gm<three_d::InstancedMesh, three_d::ColorMaterial>,
    /// The palette resolved from `ConfigBaseline` at start-up (D-35, GAP-095), carried
    /// here because the OpenGL draw path has no `egui::Ui` to read it from the way the
    /// 2D projection's functions do. Set once, in `attach`, and never reassigned.
    palette: theme::Palette,
}

impl SceneRenderer {
    /// Attach to the OpenGL context eframe owns.
    ///
    /// Takes the `Arc` eframe hands out rather than creating a context, because there is
    /// exactly one and it belongs to the window. Returns an error rather than panicking:
    /// a desktop that cannot attach must fall back to the 2D projection and say so, not
    /// die at startup over a rendering feature.
    pub fn attach(
        gl: Option<Arc<three_d::context::Context>>,
        palette: theme::Palette,
    ) -> Result<Self, AttachError> {
        let gl = gl.ok_or(AttachError::NoGlContext)?;
        let context = three_d::Context::from_gl_context(gl)
            .map_err(|e| AttachError::Rejected(e.to_string()))?;

        // A low-detail unit sphere: the glyph is a marker, and a smoother one would cost
        // vertices nobody can see. Each instance scales it to the zoom (`instances`).
        let cpu_mesh = three_d::CpuMesh::sphere(8);
        let glyph_mesh = three_d::Gm::new(
            three_d::InstancedMesh::new(&context, &three_d::Instances::default(), &cpu_mesh),
            three_d::ColorMaterial::default(),
        );

        Ok(Self {
            context,
            glyph_mesh,
            palette,
        })
    }

    /// Draw the glyphs for this frame into the region egui has given the viewport.
    ///
    /// `viewport` and `clip` are in physical pixels with the origin at the bottom left,
    /// which is what OpenGL and three-d use; egui's rect is in points with the origin at
    /// the top left, so the caller converts. Getting that wrong draws the picture in the
    /// wrong half of the window, which is why the conversion is [`viewport_from`] and
    /// tested. `pixels_per_point` is egui's, because the 2D view's scale is in points.
    pub fn paint(
        &mut self,
        glyphs: &[TrackGlyph],
        view: &TopDownView,
        viewport: three_d::Viewport,
        clip: three_d::Viewport,
        pixels_per_point: f32,
    ) {
        let camera = camera(view, viewport, pixels_per_point);
        self.glyph_mesh
            .geometry
            .set_instances(&instances(&self.palette, glyphs, view));
        // `render` would clip to a box at the window's bottom-left corner the size of the
        // viewport, which is the viewport itself only when nothing sits left of or below
        // the picture. A role workspace docks panels around it, so the box is the one
        // egui clips this callback to.
        three_d::RenderTarget::screen(&self.context, viewport.width, viewport.height)
            .render_partially(clip.into(), &camera, [&self.glyph_mesh], &[]);
    }

    /// How many instances the last frame uploaded, for the status line.
    #[must_use]
    pub fn instance_count(&self) -> u32 {
        self.glyph_mesh.geometry.instance_count()
    }
}

/// The instance data for the glyphs: one transform and colour per track.
///
/// Pure, so it can be checked without a GL context. Positions come straight from the
/// track's ENU state, which is the frame the whole picture is in; the colour is the
/// lifecycle colour the 2D projection and the track table use, so the three surfaces
/// cannot disagree about what a stale track looks like. Each sphere's radius is the 2D
/// glyph's at the current zoom: a radius fixed in metres was a few pixels across at
/// sector scale and filled the screen zoomed in.
// The world is in metres and OpenGL is f32. A sector is tens of kilometres across, so
// f32 holds a position to well under a millimetre; the tracking state stays f64 and only
// the drawing is narrowed.
#[allow(clippy::cast_possible_truncation)]
#[must_use]
pub fn instances(
    palette: &theme::Palette,
    glyphs: &[TrackGlyph],
    view: &TopDownView,
) -> three_d::Instances {
    let radius_m = GLYPH_RADIUS_PX * view.meters_per_px as f32;
    let mut transformations = Vec::with_capacity(glyphs.len());
    let mut colors = Vec::with_capacity(glyphs.len());
    for glyph in glyphs {
        let [e, n, u] = glyph.position;
        transformations.push(
            three_d::Mat4::from_translation(three_d::vec3(e as f32, n as f32, u as f32))
                * three_d::Mat4::from_scale(radius_m),
        );
        let c = theme::track_color(palette, glyph.status, glyph.stale);
        colors.push(three_d::Srgba::new(c.r(), c.g(), c.b(), c.a()));
    }
    three_d::Instances {
        transformations,
        colors: Some(colors),
        texture_transformations: None,
    }
}

/// The camera for this frame, framing the same ground the 2D projection shows.
///
/// Pure. The 2D view's centre and zoom are the operator's chosen frame, and the 3D
/// camera follows them rather than having controls of its own: two independent cameras
/// would let the two renderers show different parts of the sector under one label.
///
/// **Orthographic, and sized in egui points**, because everything egui draws over the
/// scene -- the rings, the dead spots, the bearing rays -- is placed by
/// [`TopDownView::project`], which ignores altitude and counts in points. The camera
/// this replaced was a perspective one sized from the viewport's width in physical
/// pixels against a vertical field of view, so it showed `aspect * pixels_per_point`
/// times the ground the overlay did, and a track at altitude drifted outward from the
/// centre besides. `the_camera_puts_a_track_where_the_projection_does` holds it to the
/// projection.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
#[must_use]
pub fn camera(
    view: &TopDownView,
    viewport: three_d::Viewport,
    pixels_per_point: f32,
) -> three_d::Camera {
    let target = three_d::vec3(view.center_e as f32, view.center_n as f32, 0.0);
    let height_points = viewport.height as f32 / pixels_per_point.max(f32::EPSILON);
    let height_m = height_points * view.meters_per_px.max(f64::EPSILON) as f32;
    // three-d multiplies an orthographic height by the camera's distance from its
    // target, so the figure it is given is per metre of that distance.
    three_d::Camera::new_orthographic(
        viewport,
        target + three_d::vec3(0.0, 0.0, CAMERA_ALTITUDE_M),
        target,
        three_d::vec3(0.0, 1.0, 0.0),
        height_m / CAMERA_ALTITUDE_M,
        0.0,
        2.0 * CAMERA_ALTITUDE_M,
    )
}

/// The OpenGL viewport for the region egui gave the callback.
///
/// Takes the plain pixel figures from `PaintCallbackInfo::viewport_in_pixels` rather
/// than the struct, so this crate names no egui rendering type and the conversion stays
/// testable. egui has already done the points-to-pixels and top-to-bottom work --
/// `from_bottom_px` is documented as what `glViewport` wants -- so this does not repeat
/// a conversion egui is the authority on. What it adds is the floor of one pixel: a
/// collapsed region would otherwise divide by zero in the projection, and a panic inside
/// a paint callback takes the window down.
#[must_use]
#[allow(clippy::cast_sign_loss)]
pub fn viewport_from(
    left_px: i32,
    from_bottom_px: i32,
    width_px: i32,
    height_px: i32,
) -> three_d::Viewport {
    three_d::Viewport {
        x: left_px,
        y: from_bottom_px,
        width: width_px.max(1) as u32,
        height: height_px.max(1) as u32,
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use gungnir_model::{Classification, TrackId, TrackStatus};
    use three_d::Viewer;

    fn glyph(id: u64, enu: [f64; 3], status: TrackStatus, stale: bool) -> TrackGlyph {
        TrackGlyph {
            track_id: TrackId(id),
            status,
            stale,
            classification: Classification::Unknown,
            association_confidence: 1.0,
            position: enu,
            velocity: [0.0; 3],
            sigma: [10.0; 3],
        }
    }

    /// One instance per track, at the track's own ENU position. A transform that
    /// dropped or reordered a track would put a symbol where nothing is.
    #[test]
    fn every_track_gets_one_instance_at_its_own_position() {
        let glyphs = [
            glyph(1, [100.0, 200.0, 50.0], TrackStatus::Confirmed, false),
            glyph(2, [-300.0, 0.0, 0.0], TrackStatus::Coasting, false),
        ];
        let view = TopDownView::default();
        let palette = theme::Palette::day();
        let instances = instances(&palette, &glyphs, &view);

        assert_eq!(instances.transformations.len(), 2);
        for (i, expected) in [[100.0_f32, 200.0, 50.0], [-300.0, 0.0, 0.0]]
            .iter()
            .enumerate()
        {
            let t = instances.transformations[i];
            assert_eq!(t.w.x, expected[0]);
            assert_eq!(t.w.y, expected[1]);
            assert_eq!(t.w.z, expected[2]);
        }
    }

    /// The 3D colour is the same lifecycle colour the 2D projection and the track table
    /// use. Three surfaces showing one track must not disagree about whether it is
    /// stale.
    #[test]
    fn the_glyph_colour_is_the_shared_lifecycle_colour() {
        let glyphs = [
            glyph(1, [0.0; 3], TrackStatus::Confirmed, false),
            glyph(2, [0.0; 3], TrackStatus::Confirmed, true),
        ];
        let palette = theme::Palette::day();
        let instances = instances(&palette, &glyphs, &TopDownView::default());
        let colors = instances.colors.expect("colours are set");

        let fresh = theme::track_color(&palette, TrackStatus::Confirmed, false);
        let stale = theme::track_color(&palette, TrackStatus::Confirmed, true);
        assert_eq!(
            (colors[0].r, colors[0].g, colors[0].b),
            (fresh.r(), fresh.g(), fresh.b())
        );
        assert_eq!(
            (colors[1].r, colors[1].g, colors[1].b),
            (stale.r(), stale.g(), stale.b())
        );
        assert_ne!(
            colors[0].r, colors[1].r,
            "stale and fresh must look different"
        );
    }

    /// An empty picture produces an empty instance set rather than one instance at the
    /// origin, which would draw a track that does not exist.
    #[test]
    fn no_tracks_means_no_instances() {
        let instances = instances(&theme::Palette::day(), &[], &TopDownView::default());
        assert!(instances.transformations.is_empty());
        assert_eq!(instances.colors.as_ref().map(Vec::len), Some(0));
        instances
            .validate()
            .expect("an empty instance set is valid");
    }

    /// The camera looks at the operator's chosen centre, from above, and its far plane
    /// is beyond the ground it is looking at -- a near/far pair that clipped the ground
    /// would render an empty view that looks exactly like no tracks.
    #[test]
    fn the_camera_follows_the_two_dimensional_view() {
        let view = TopDownView {
            center_e: 1_500.0,
            center_n: -700.0,
            ..TopDownView::default()
        };
        let viewport = three_d::Viewport::new_at_origo(800, 600);
        let camera = camera(&view, viewport, 1.0);

        assert_eq!(camera.target().x, 1_500.0);
        assert_eq!(camera.target().y, -700.0);
        assert!(
            camera.position().z > 0.0,
            "the camera must be above the ground it is looking at"
        );
        let distance = camera.position().z - camera.target().z;
        assert!(
            camera.z_far() > distance,
            "the far plane clips the ground the camera is aimed at: far {} distance {distance}",
            camera.z_far()
        );
        assert!(camera.z_near() < distance);
    }

    /// Where the camera puts an ENU point, in egui points from the viewport's top left.
    fn on_screen(camera: &three_d::Camera, enu: [f32; 3], size_points: egui::Vec2) -> egui::Pos2 {
        let clip = camera.projection() * camera.view() * three_d::vec4(enu[0], enu[1], enu[2], 1.0);
        let (x, y) = (clip.x / clip.w, clip.y / clip.w);
        egui::pos2(
            f32::midpoint(x, 1.0) * size_points.x,
            f32::midpoint(-y, 1.0) * size_points.y,
        )
    }

    /// **The scene and the overlay put a track in the same place.** egui draws the rings,
    /// the dead spots and the bearing rays over the scene with [`TopDownView::project`],
    /// so a camera that disagreed with it would draw every track off its own context --
    /// which is what the perspective camera did, at every zoom, on any viewport that was
    /// not square, and on any display scaled above 100 %. Checked at two zooms, a wide
    /// and a tall viewport, a 150 % display, and a track at altitude, since the
    /// projection ignores altitude and the camera must too.
    #[test]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "test figures: small positive pixel sizes and metre positions"
    )]
    fn the_camera_puts_a_track_where_the_projection_does() {
        let points: [[f64; 3]; 4] = [
            [1_500.0, -700.0, 0.0],
            [9_000.0, 4_000.0, 300.0],
            [-12_000.0, 2_500.0, 9_000.0],
            [3_000.0, -6_000.0, -40.0],
        ];
        for (meters_per_px, size_points, pixels_per_point) in [
            (10.0, egui::vec2(800.0, 600.0), 1.0),
            (40.0, egui::vec2(1_200.0, 500.0), 1.5),
            (2.5, egui::vec2(400.0, 900.0), 2.0),
        ] {
            let view = TopDownView {
                center_e: 1_500.0,
                center_n: -700.0,
                meters_per_px,
            };
            let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, size_points);
            let viewport = three_d::Viewport::new_at_origo(
                (size_points.x * pixels_per_point) as u32,
                (size_points.y * pixels_per_point) as u32,
            );
            let camera = camera(&view, viewport, pixels_per_point);
            for enu in points {
                let expected = view.project(enu, rect);
                let drawn = on_screen(
                    &camera,
                    [enu[0] as f32, enu[1] as f32, enu[2] as f32],
                    size_points,
                );
                assert!(
                    (drawn - expected).length() < 0.05,
                    "at {meters_per_px} m/pt, {size_points:?} points, {pixels_per_point} px/pt: \
                     {enu:?} drawn at {drawn:?}, the overlay puts it at {expected:?}"
                );
            }
        }
    }

    /// A glyph is the 2D glyph's size on screen at every zoom, not a fixed size in
    /// metres: 120 m was two pixels across at sector scale and filled the view close in.
    #[test]
    #[allow(clippy::cast_possible_truncation)]
    fn a_glyph_is_the_same_size_on_screen_at_every_zoom() {
        let glyphs = [glyph(1, [0.0; 3], TrackStatus::Confirmed, false)];
        for meters_per_px in [0.5, 10.0, 400.0] {
            let view = TopDownView {
                meters_per_px,
                ..TopDownView::default()
            };
            let instances = instances(&theme::Palette::day(), &glyphs, &view);
            let radius_m = instances.transformations[0].x.x;
            let radius_points = radius_m / meters_per_px as f32;
            assert!(
                (radius_points - GLYPH_RADIUS_PX).abs() < 1e-3,
                "at {meters_per_px} m/pt the glyph is {radius_points} points across, \
                 not the 2D glyph's {GLYPH_RADIUS_PX}"
            );
        }
    }

    /// The conversion takes egui's own pixel figures, including the bottom-left origin
    /// it computes for `glViewport`, and changes only the types.
    #[test]
    fn the_viewport_takes_eguis_pixel_figures() {
        let viewport = viewport_from(20, 960, 800, 600);
        assert_eq!(viewport.x, 20);
        assert_eq!(viewport.y, 960, "OpenGL measures from the bottom");
        assert_eq!(viewport.width, 800);
        assert_eq!(viewport.height, 600);
    }

    /// A collapsed region still produces a viewport with a positive size: a zero width
    /// is a division by zero in the projection, and a panic in a paint callback takes
    /// the window down.
    #[test]
    fn a_collapsed_region_still_yields_a_usable_viewport() {
        let viewport = viewport_from(0, 0, 0, 0);
        assert!(viewport.width >= 1 && viewport.height >= 1);

        let view = TopDownView {
            meters_per_px: 0.0,
            ..TopDownView::default()
        };
        let camera = camera(&view, viewport, 0.0);
        assert!(camera.position().z.is_finite());
        assert!(camera.projection().x.x.is_finite());
        assert!(camera.z_far() > camera.z_near());
    }
}

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
//! **The drawn result has been seen once: by the owner, on 2026-10-30** (GAP-226), with
//! every check in `docs/record/2026-10-30/the-owner-s-look-at-the-three-d.md`
//! passed; and again the same day with the track glyphs drawn over it (GAP-251,
//! `docs/record/2026-10-30/track-glyphs-over-the-three-d-scene-looked.md`). No test can see
//! it again. **The ground pass (GAP-252) has not been seen at all**: what the owner looks for
//! is in `docs/record/2026-10-30/gap-252-terrain-in-the-three-d-scene.md`. Unlike the egui
//! panels there is no headless probe
//! for a GL draw call -- `RenderProbe` reads egui's shape list, and three-d does not
//! produce one -- so a change to this draw is unseen until somebody looks, and that
//! record's checks are what to look for.
//!
//! This path is the **default** (`ui.scene_3d`, on), by the owner's decisions of
//! 2026-09-05 and 2026-10-30 (D-153). Two things carry the weight that a test would
//! otherwise have carried, and both are deliberate: the viewport's status line is
//! painted by egui after both GL passes (GAP-252), and neither clears anything, so a
//! draw that produces nothing still leaves the line reporting how many tracks exist --
//! an empty viewport that says twelve tracks is a fault someone can report, an empty
//! viewport that says nothing is an operator concluding the sector is clear; and the
//! operator can switch to the projection at runtime, without editing a baseline and
//! restarting during whatever is happening at the time.

/// The OpenGL context type three-d accepts.
///
/// Re-exported so a caller can name it without depending on three-d: `gungnir-app` uses
/// it to assert at compile time that the `Arc` eframe hands out is this same type, which
/// is the fact this whole attachment rests on.
pub use three_d::context::Context as GlContext;

/// The OpenGL region a paint callback draws into, re-exported for the same reason: the
/// binary converts egui's region once for both GL passes (GAP-252) and names the type.
pub use three_d::Viewport;

use crate::interaction::TopDownView;
use crate::layers::TerrainSurface;
use crate::tracks::{TrackGlyph, GLYPH_RADIUS_PX};
use crate::Ground;
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
/// Holds the context and the one instanced mesh every glyph is drawn from. The context
/// and the mesh's vertex buffers are created once; **its instance buffers are not**:
/// three-d 0.19's `InstancedMesh::set_instances` allocates four fresh `InstanceBuffer`s
/// on every call (three transform rows and the colours), so the glyph pass calls it only
/// when the glyph list or the zoom has changed, keyed by [`GlyphUploadKey`] (GAP-254).
/// `rust-ui-architecture-coding-standards.md` §5 forbids building GPU objects per frame,
/// and until GAP-254 this pass did, on every frame, whether or not any track had moved.
/// An instanced mesh is still the shape that lets one draw call serve however many
/// tracks there are.
pub struct SceneRenderer {
    context: three_d::Context,
    glyph_mesh: three_d::Gm<three_d::InstancedMesh, three_d::ColorMaterial>,
    /// What the instance buffers hold: the glyph revision and the zoom they were built
    /// for, or `None` before the first upload (GAP-254). A frame whose key is this one
    /// draws the buffers as they are.
    uploaded: Option<GlyphUploadKey>,
    /// The terrain's mesh and the surface revision it was built from (GAP-252, DN-35 §6):
    /// built the first time the ground pass is handed a revision it does not hold, and
    /// dropped when handed none, so the upload is once per load.
    ground: Option<(u64, three_d::Gm<three_d::Mesh, three_d::ColorMaterial>)>,
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
            uploaded: None,
            ground: None,
            palette,
        })
    }

    /// The surface revision the ground pass holds, for [`crate::ground_source`]. The
    /// binary reads it under the lock it already takes, before the frame is drawn.
    #[must_use]
    pub fn ground_revision(&self) -> Option<u64> {
        self.ground.as_ref().map(|(revision, _)| *revision)
    }

    /// The ground pass (GAP-252, DN-35 §5.1): bring the uploaded mesh up to date with
    /// `surface`, then draw it if `ground` says the scene draws it this frame and the mesh
    /// held is this surface's. On the frame a surface first arrives the frame was decided
    /// for egui, so this uploads and draws nothing; the next frame draws it.
    ///
    /// The terrain is translucent, as in the projection, so the background and the metre
    /// grid egui painted before this show through. A straight-down grid never overlaps
    /// itself, so nothing needs sorting, and no depth buffer is asked for (DN-35 §4).
    pub fn paint_ground(
        &mut self,
        surface: Option<&TerrainSurface>,
        ground: Ground,
        view: &TopDownView,
        viewport: three_d::Viewport,
        clip: three_d::Viewport,
        pixels_per_point: f32,
    ) {
        match ground_upload(self.ground_revision(), surface.map(|s| s.revision)) {
            GroundUpload::Keep => {}
            GroundUpload::Drop => self.ground = None,
            GroundUpload::Build => {
                if let Some(surface) = surface {
                    let mesh = three_d::Mesh::new(&self.context, &ground_mesh(surface));
                    let material = three_d::ColorMaterial::new_transparent(
                        &self.context,
                        &three_d::CpuMaterial {
                            albedo: three_d::Srgba::WHITE,
                            ..three_d::CpuMaterial::default()
                        },
                    );
                    self.ground = Some((surface.revision, three_d::Gm::new(mesh, material)));
                }
            }
        }
        if ground != Ground::Scene {
            return;
        }
        let Some((held, mesh)) = &self.ground else {
            return;
        };
        if surface.map(|s| s.revision) != Some(*held) {
            return;
        }
        let camera = camera(view, viewport, pixels_per_point);
        three_d::RenderTarget::screen(&self.context, viewport.width, viewport.height)
            .render_partially(clip.into(), &camera, [mesh], &[]);
    }

    /// The glyph pass: draw the glyphs for this frame into the region egui has given the
    /// viewport.
    ///
    /// `glyph_revision` is `ViewportState::glyph_revision`, which changes whenever the
    /// glyph list is rebuilt. The instance buffers are uploaded only when it or the zoom
    /// differs from what they hold (GAP-254; [`glyph_upload`] is the decision, and it is
    /// tested): everything an instance carries is a function of the glyph list, the zoom
    /// and the palette, and the palette is fixed at attach.
    ///
    /// `viewport` and `clip` are in physical pixels with the origin at the bottom left,
    /// which is what OpenGL and three-d use; egui's rect is in points with the origin at
    /// the top left, so the caller converts. Getting that wrong draws the picture in the
    /// wrong half of the window, which is why the conversion is [`viewport_from`] and
    /// tested. `pixels_per_point` is egui's, because the 2D view's scale is in points.
    pub fn paint_glyphs(
        &mut self,
        glyphs: &[TrackGlyph],
        glyph_revision: u64,
        view: &TopDownView,
        viewport: three_d::Viewport,
        clip: three_d::Viewport,
        pixels_per_point: f32,
    ) {
        let camera = camera(view, viewport, pixels_per_point);
        let wanted = GlyphUploadKey::new(glyph_revision, view);
        if glyph_upload(self.uploaded, wanted) {
            self.glyph_mesh
                .geometry
                .set_instances(&instances(&self.palette, glyphs, view));
            self.uploaded = Some(wanted);
        }
        // `render` would clip to a box at the window's bottom-left corner the size of the
        // viewport, which is the viewport itself only when nothing sits left of or below
        // the picture. A role workspace docks panels around it, so the box is the one
        // egui clips this callback to.
        three_d::RenderTarget::screen(&self.context, viewport.width, viewport.height)
            .render_partially(clip.into(), &camera, [&self.glyph_mesh], &[]);
    }

    /// How many instances the buffers hold, for the status line. Since GAP-254 that is the
    /// last upload's count, not necessarily the last frame's; the two differ only on the
    /// frame before a rebuilt glyph list is first drawn.
    #[must_use]
    pub fn instance_count(&self) -> u32 {
        self.glyph_mesh.geometry.instance_count()
    }

    /// The key the instance buffers were last uploaded under, for the binary's status
    /// line and for a test; `None` before the first upload.
    #[must_use]
    pub fn uploaded(&self) -> Option<GlyphUploadKey> {
        self.uploaded
    }
}

/// What a glyph upload depends on (GAP-254): which glyph list, and at what zoom. The
/// zoom is carried as the bits of `TopDownView::meters_per_px`, so the key is `Eq` and a
/// NaN zoom compares equal to itself rather than forcing an upload every frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphUploadKey {
    /// `ViewportState::glyph_revision` when the list was last rebuilt.
    pub revision: u64,
    /// `TopDownView::meters_per_px`, as bits.
    pub meters_per_px_bits: u64,
}

impl GlyphUploadKey {
    /// The key for drawing `revision`'s glyphs at `view`'s zoom.
    #[must_use]
    pub fn new(revision: u64, view: &TopDownView) -> Self {
        Self {
            revision,
            meters_per_px_bits: view.meters_per_px.to_bits(),
        }
    }
}

/// Whether the glyph pass uploads its instance buffers this frame: only when `wanted`
/// differs from the key the buffers were last uploaded under, or nothing has been
/// uploaded yet. Pure, so the decision is tested without a GL context, as
/// [`ground_upload`] is for the ground pass.
#[must_use]
pub fn glyph_upload(held: Option<GlyphUploadKey>, wanted: GlyphUploadKey) -> bool {
    held != Some(wanted)
}

/// What the ground pass does with its mesh, from the revision it holds and the revision
/// the frame hands it (GAP-252, DN-35 §8.1 row b). Pure, so the upload's key is tested
/// without a GL context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroundUpload {
    /// The mesh held is the one wanted, or there is none and none is wanted.
    Keep,
    /// A surface is wanted that the pass does not hold: build its mesh.
    Build,
    /// No surface is wanted: drop the mesh held.
    Drop,
}

/// See [`GroundUpload`].
#[must_use]
pub fn ground_upload(held: Option<u64>, wanted: Option<u64>) -> GroundUpload {
    match (held, wanted) {
        (held, Some(wanted)) if held != Some(wanted) => GroundUpload::Build,
        (Some(_), None) => GroundUpload::Drop,
        _ => GroundUpload::Keep,
    }
}

/// An egui colour as three-d takes it. `egui::Color32` stores premultiplied components and
/// three-d's are not, so a translucent colour crosses unmultiplied, or the scene's ground
/// would come out darker than the projection's by its alpha (DN-35 §6).
#[must_use]
pub fn srgba_of(colour: egui::Color32) -> three_d::Srgba {
    let [r, g, b, a] = colour.to_srgba_unmultiplied();
    three_d::Srgba::new(r, g, b, a)
}

/// The terrain surface as a CPU mesh: the same vertices, colours and triangles the
/// projection draws (DN-35 §6), in ENU metres at their placed up.
#[must_use]
pub fn ground_mesh(surface: &TerrainSurface) -> three_d::CpuMesh {
    three_d::CpuMesh {
        positions: three_d::Positions::F32(
            surface
                .positions
                .iter()
                .map(|p| three_d::vec3(p[0], p[1], p[2]))
                .collect(),
        ),
        indices: three_d::Indices::U32(surface.indices.clone()),
        colors: Some(surface.colors.iter().map(|c| srgba_of(*c)).collect()),
        ..three_d::CpuMesh::default()
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

    /// A placed grid whose up runs from -430 m to 8 850 m, more relief than any DEM on
    /// Earth, as a terrain surface.
    #[allow(clippy::cast_precision_loss)]
    fn tall_surface() -> TerrainSurface {
        let (rows, columns) = (40_u32, 50_u32);
        let mut positions = Vec::new();
        for r in 0..rows {
            for c in 0..columns {
                let (e, n) = (
                    c as f32 * 400.0 - 9_000.0,
                    (rows - 1 - r) as f32 * 400.0 - 7_000.0,
                );
                let t = (r * columns + c) as f32 / (rows * columns - 1) as f32;
                positions.push([e, n, -430.0 + t * 9_280.0]);
            }
        }
        crate::layers::terrain_surface(
            &theme::Palette::day(),
            crate::layers::TerrainLayer {
                positions: &positions,
                rows,
                columns,
                revision: 1,
            },
        )
        .expect("a surface")
    }

    /// GAP-252, DN-35 §8.1 row c: **the scene puts the ground where the projection does.**
    /// Every vertex of a surface from -430 m to 8 850 m lands within 0.05 point of
    /// [`TopDownView::project`], and inside the camera's near and far planes, at the zooms,
    /// viewport shapes and display scales `the_camera_puts_a_track_where_the_projection_does`
    /// uses.
    #[test]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "test figures: small positive pixel sizes"
    )]
    fn the_camera_puts_the_ground_where_the_projection_does() {
        let surface = tall_surface();
        let (lo, hi) = surface
            .positions
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
                (lo.min(p[2]), hi.max(p[2]))
            });
        assert!(
            lo <= -430.0 && hi >= 8_849.0,
            "the fixture's extremes: {lo} to {hi}"
        );
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
            for p in &surface.positions {
                let expected =
                    view.project([f64::from(p[0]), f64::from(p[1]), f64::from(p[2])], rect);
                let drawn = on_screen(&camera, *p, size_points);
                assert!(
                    (drawn - expected).length() < 0.05,
                    "at {meters_per_px} m/pt, {size_points:?} points, {pixels_per_point} px/pt: \
                     {p:?} drawn at {drawn:?}, the projection puts it at {expected:?}"
                );
                let clip =
                    camera.projection() * camera.view() * three_d::vec4(p[0], p[1], p[2], 1.0);
                let depth = clip.z / clip.w;
                assert!(
                    (-1.0..=1.0).contains(&depth),
                    "{p:?} is outside the near and far planes: depth {depth}"
                );
            }
        }
    }

    /// GAP-252, DN-35 §8.1 row f: a translucent terrain colour reaches three-d with its
    /// unmultiplied components and its alpha. Read premultiplied, as the opaque glyphs are,
    /// the scene's ground would be darker than the projection's by the alpha.
    #[test]
    fn a_terrain_colour_crosses_unmultiplied() {
        let palette = theme::Palette::day();
        for t in [0.0, 0.4, 1.0] {
            let colour = crate::layers::terrain_color(&palette, t);
            let crossed = srgba_of(colour);
            let ramp = crate::layers::terrain_rgb(t);
            assert_eq!(crossed.a, palette.terrain_alpha);
            for (got, want) in [
                (crossed.r, ramp[0]),
                (crossed.g, ramp[1]),
                (crossed.b, ramp[2]),
            ] {
                assert!(got.abs_diff(want) <= 1, "{got} is not the ramp's {want}");
            }
            assert!(
                colour.r() < crossed.r,
                "the premultiplied component {} would have darkened it",
                colour.r()
            );
        }
    }

    /// The mesh the ground pass uploads is the surface: the same vertices at their placed
    /// up, the same triangles, and one colour per vertex.
    #[test]
    fn the_ground_mesh_is_the_surface() {
        let surface = tall_surface();
        let mesh = ground_mesh(&surface);
        match &mesh.positions {
            three_d::Positions::F32(positions) => {
                assert_eq!(positions.len(), surface.positions.len());
                for (got, want) in positions.iter().zip(&surface.positions) {
                    assert_eq!([got.x, got.y, got.z], *want);
                }
            }
            three_d::Positions::F64(_) => panic!("the surface is f32"),
        }
        match &mesh.indices {
            three_d::Indices::U32(indices) => assert_eq!(indices, &surface.indices),
            _ => panic!("the surface's indices are u32"),
        }
        assert_eq!(
            mesh.colors.as_ref().map(Vec::len),
            Some(surface.colors.len())
        );
        mesh.validate().expect("a valid mesh");
    }

    /// GAP-252, DN-35 §8.1 row b's GPU half: the ground pass builds its mesh when handed a
    /// revision it does not hold, keeps it while handed the same one, and drops it when
    /// handed none.
    #[test]
    fn the_ground_is_uploaded_once_per_revision() {
        assert_eq!(ground_upload(None, None), GroundUpload::Keep);
        assert_eq!(ground_upload(None, Some(1)), GroundUpload::Build);
        assert_eq!(ground_upload(Some(1), Some(1)), GroundUpload::Keep);
        assert_eq!(ground_upload(Some(1), Some(2)), GroundUpload::Build);
        assert_eq!(ground_upload(Some(1), None), GroundUpload::Drop);
        // A hundred frames at one revision: one build.
        let mut held = None;
        let mut builds = 0;
        for _ in 0..100 {
            if ground_upload(held, Some(9)) == GroundUpload::Build {
                builds += 1;
                held = Some(9);
            }
        }
        assert_eq!(builds, 1);
    }

    /// GAP-254: two paints with the same glyph list and the same zoom upload the instance
    /// buffers once. A hundred frames of a still picture are one upload; a rebuilt glyph
    /// list is another; a zoom is another, because the radius is sized at the zoom; and
    /// panning, which changes neither, is none. Before the first upload every key
    /// uploads, so an empty picture is still drawn as empty rather than as whatever the
    /// buffers held.
    #[test]
    fn the_instances_are_uploaded_once_per_glyph_list_and_zoom() {
        let zoomed = |meters_per_px| TopDownView {
            meters_per_px,
            ..TopDownView::default()
        };
        let first = GlyphUploadKey::new(1, &zoomed(10.0));
        assert!(glyph_upload(None, first), "nothing uploaded yet");
        assert!(
            !glyph_upload(Some(first), first),
            "the same list at the same zoom"
        );
        assert!(
            glyph_upload(Some(first), GlyphUploadKey::new(2, &zoomed(10.0))),
            "a rebuilt glyph list"
        );
        assert!(
            glyph_upload(Some(first), GlyphUploadKey::new(1, &zoomed(20.0))),
            "a zoom changes the radius"
        );
        let panned = TopDownView {
            center_e: 5_000.0,
            center_n: -2_000.0,
            meters_per_px: 10.0,
        };
        assert!(
            !glyph_upload(Some(first), GlyphUploadKey::new(1, &panned)),
            "a pan moves the camera, not the instances"
        );

        // A hundred frames of one picture: one upload.
        let mut held = None;
        let mut uploads = 0;
        for _ in 0..100 {
            let wanted = GlyphUploadKey::new(7, &zoomed(12.5));
            if glyph_upload(held, wanted) {
                uploads += 1;
                held = Some(wanted);
            }
        }
        assert_eq!(uploads, 1);
    }

    /// A NaN zoom is one key, not a different key every frame: `meters_per_px` is held
    /// as bits, so the degenerate view a collapsed region produces does not become an
    /// upload per frame.
    #[test]
    fn a_nan_zoom_does_not_upload_every_frame() {
        let view = TopDownView {
            meters_per_px: f64::NAN,
            ..TopDownView::default()
        };
        let key = GlyphUploadKey::new(1, &view);
        assert!(!glyph_upload(Some(key), GlyphUploadKey::new(1, &view)));
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

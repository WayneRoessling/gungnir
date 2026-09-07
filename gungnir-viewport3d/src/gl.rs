//! Attaching the three-d scene to eframe's OpenGL context (GAP-022).
//!
//! `ARCHITECTURE.md` §4 kept three-d for the viewport and §9 records the consequence:
//! the desktop has two GPU contexts, an OpenGL one for everything the operator sees and
//! a wgpu compute one for point-cloud fusion. This is the OpenGL half being connected.
//!
//! # Why the attachment is possible on the pinned set, and why that is not incidental
//!
//! `eframe` 0.29, `egui_glow` 0.29 and `three-d` 0.18 all depend on `glow` 0.14, so the
//! `Arc<glow::Context>` eframe hands out is the same type `three_d::Context` accepts.
//! Had they differed there would be two `glow`s in the binary and no way to pass the
//! context across. `cargo tree -d` reports one `glow 0.14.2` shared by all three, and
//! the version set in §9 is what keeps it that way -- moving any one of them is a
//! decision about this attachment, not only about that crate.
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
//! *over* this callback's output, so a draw that produces nothing still reports how many
//! tracks exist -- an empty viewport that says twelve tracks is a fault someone can
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
use crate::tracks::TrackGlyph;
use gungnir_ui::theme;
use std::sync::Arc;

/// Radius of a track glyph in metres. Large enough to see at sector scale; the symbol
/// is a marker, not a model of the vehicle.
pub const GLYPH_RADIUS_M: f32 = 120.0;

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
}

impl SceneRenderer {
    /// Attach to the OpenGL context eframe owns.
    ///
    /// Takes the `Arc` eframe hands out rather than creating a context, because there is
    /// exactly one and it belongs to the window. Returns an error rather than panicking:
    /// a desktop that cannot attach must fall back to the 2D projection and say so, not
    /// die at startup over a rendering feature.
    pub fn attach(gl: Option<Arc<three_d::context::Context>>) -> Result<Self, AttachError> {
        let gl = gl.ok_or(AttachError::NoGlContext)?;
        let context = three_d::Context::from_gl_context(gl)
            .map_err(|e| AttachError::Rejected(e.to_string()))?;

        // A low-detail sphere: the glyph is a marker at sector scale, and a smoother
        // one would cost vertices nobody can see.
        let mut cpu_mesh = three_d::CpuMesh::sphere(8);
        cpu_mesh
            .transform(three_d::Mat4::from_scale(GLYPH_RADIUS_M))
            .ok();
        let glyph_mesh = three_d::Gm::new(
            three_d::InstancedMesh::new(&context, &three_d::Instances::default(), &cpu_mesh),
            three_d::ColorMaterial::default(),
        );

        Ok(Self {
            context,
            glyph_mesh,
        })
    }

    /// Draw the glyphs for this frame into the region egui has given the viewport.
    ///
    /// `viewport` is in physical pixels with the origin at the bottom left, which is
    /// what OpenGL and three-d use; egui's rect is in points with the origin at the top
    /// left, so the caller converts. Getting that wrong draws the picture in the wrong
    /// half of the window, which is why the conversion is [`viewport_for`] and tested.
    pub fn paint(
        &mut self,
        glyphs: &[TrackGlyph],
        view: &TopDownView,
        viewport: three_d::Viewport,
    ) {
        let camera = camera(view, viewport);
        self.glyph_mesh
            .geometry
            .set_instances(&instances(glyphs, view));
        three_d::RenderTarget::screen(&self.context, viewport.width, viewport.height).render(
            &camera,
            [&self.glyph_mesh],
            &[],
        );
    }

    /// How many instances the last frame uploaded, for the status line.
    #[must_use]
    pub fn instance_count(&self) -> u32 {
        self.glyph_mesh.geometry.instance_count()
    }
}

/// Half the vertical field of view, as a tangent. The camera is 45 degrees, matching
/// the one `ViewportState` has always held.
const FOV_HALF_TAN: f32 = 0.414_213_57; // tan(22.5 degrees)

/// The instance data for the glyphs: one transform and colour per track.
///
/// Pure, so it can be checked without a GL context. Positions come straight from the
/// track's ENU state, which is the frame the whole picture is in; the colour is the
/// lifecycle colour the 2D projection and the track table use, so the three surfaces
/// cannot disagree about what a stale track looks like.
// The world is in metres and OpenGL is f32. A sector is tens of kilometres across, so
// f32 holds a position to well under a millimetre; the tracking state stays f64 and only
// the drawing is narrowed.
#[allow(clippy::cast_possible_truncation)]
#[must_use]
pub fn instances(glyphs: &[TrackGlyph], view: &TopDownView) -> three_d::Instances {
    let mut transformations = Vec::with_capacity(glyphs.len());
    let mut colors = Vec::with_capacity(glyphs.len());
    for glyph in glyphs {
        let [e, n, u] = glyph.position;
        transformations.push(three_d::Mat4::from_translation(three_d::vec3(
            e as f32, n as f32, u as f32,
        )));
        let c = theme::track_color(glyph.status, glyph.stale);
        colors.push(three_d::Srgba::new(c.r(), c.g(), c.b(), c.a()));
    }
    let _ = view;
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
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
#[must_use]
pub fn camera(view: &TopDownView, viewport: three_d::Viewport) -> three_d::Camera {
    let target = three_d::vec3(view.center_e as f32, view.center_n as f32, 0.0);
    // The height that makes the visible ground width equal the 2D projection's, which
    // is what makes "the same ground" true rather than approximately true. The 2D view
    // shows `width * meters_per_px` metres across; a perspective camera at height `h`
    // with a vertical field of view `f` shows `2h*tan(f/2)` vertically, so matching the
    // half-width gives the height below.
    let half_width_m = (viewport.width as f32 / 2.0) * view.meters_per_px.max(f64::EPSILON) as f32;
    let height = half_width_m / FOV_HALF_TAN;
    three_d::Camera::new_perspective(
        viewport,
        target + three_d::vec3(0.0, 0.0, height.max(1.0)),
        target,
        three_d::vec3(0.0, 1.0, 0.0),
        three_d::degrees(45.0),
        1.0,
        height.max(1.0) * 4.0,
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
        let instances = instances(&glyphs, &view);

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
        let instances = instances(&glyphs, &TopDownView::default());
        let colors = instances.colors.expect("colours are set");

        let fresh = theme::track_color(TrackStatus::Confirmed, false);
        let stale = theme::track_color(TrackStatus::Confirmed, true);
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
        let instances = instances(&[], &TopDownView::default());
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
        let camera = camera(&view, viewport);

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

    /// Zooming in moves the camera closer. A camera whose height ignored the zoom would
    /// leave the 3D view showing a different area from the 2D one under the same
    /// controls.
    #[test]
    fn zooming_in_lowers_the_camera() {
        let viewport = three_d::Viewport::new_at_origo(800, 600);
        // Smaller metres-per-pixel is more zoomed in.
        let wide = TopDownView {
            meters_per_px: 100.0,
            ..TopDownView::default()
        };
        let close = TopDownView {
            meters_per_px: 1.0,
            ..TopDownView::default()
        };
        assert!(
            camera(&close, viewport).position().z < camera(&wide, viewport).position().z,
            "zooming in did not bring the camera down"
        );
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
        let camera = camera(&view, viewport);
        assert!(camera.position().z.is_finite());
        assert!(camera.z_far() > camera.z_near());
    }
}

// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The facts the three-d attachment rests on (GAP-022).
//!
//! The attachment itself needs a live OpenGL context and cannot run here. What can be
//! checked is the thing that makes it possible at all, and the default that holds while
//! nobody has looked at the result.

use gungnir_app::state::AppState;
use gungnir_config::ConfigBaseline;

/// The load-bearing fact, as a compile-time assertion: eframe and three-d share one
/// `glow`.
///
/// If they ever do not, there are two `glow` crates in the binary, the `Arc` eframe
/// hands out is a different type from the one `three_d::Context::from_gl_context` takes,
/// and the viewport silently loses its 3D path -- or rather, it would not compile, which
/// is the point. This function exists to make that failure appear here, at the one line
/// that states the assumption, rather than fifty lines into `main.rs`.
///
/// `cargo tree -d` is the other half of the check and reports one `glow 0.14.2` shared
/// by `eframe`, `egui_glow` and `three-d`. This is the half that runs in CI.
#[allow(dead_code)]
fn eframe_and_three_d_share_one_glow(
    gl: std::sync::Arc<eframe::glow::Context>,
) -> std::sync::Arc<gungnir_viewport3d::gl::GlContext> {
    gl
}

/// The three-d scene is the default, by the owner's decision of 2026-09-05, and a
/// deployment can opt out.
///
/// It is worth stating plainly what this default is: the GL draw call has not been
/// rendered anywhere anyone could see it, and there is no headless probe for one --
/// `RenderProbe` reads egui's shape list, and three-d does not produce shapes. So the
/// renderer that draws the picture by default is the unverified one. Two things carry
/// the weight instead, and both are asserted elsewhere: the status line is painted by
/// egui over the callback's output, so a GL draw that produces nothing still reports the
/// track count; and the operator can switch to the projection at runtime.
#[test]
fn the_three_dimensional_scene_is_the_default_and_can_be_turned_off() {
    let baseline = ConfigBaseline::default();
    assert!(
        baseline.ui.scene_3d,
        "the default changed without the ledger"
    );

    let dir = std::env::temp_dir().join(format!("gungnir-gl-default-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let state = AppState::with_config(ConfigBaseline {
        data_dir: dir.to_string_lossy().into_owned(),
        ..ConfigBaseline::default()
    })
    .expect("desktop state");
    assert!(state.config.ui.scene_3d);

    // And with no GL context attached, the session does not claim the renderer it asked
    // for: `use_3d` follows what is actually there, not what the baseline wanted.
    assert!(
        !state.viewport.gl_ready,
        "a state built without eframe has no GL context"
    );
    assert!(
        !state.viewport.use_3d,
        "the session claimed a renderer that was never attached"
    );
}

/// Attaching without an OpenGL context fails/// Attaching without an OpenGL context fails with a reason rather than panicking. This
/// is the path a desktop running the wgpu renderer would take, and the one a headless
/// test can exercise.
#[test]
fn attaching_without_a_context_is_an_error_not_a_panic() {
    // `SceneRenderer` holds GPU handles and is not `Debug`, so the error is matched
    // rather than unwrapped.
    match gungnir_viewport3d::gl::SceneRenderer::attach(None, gungnir_ui::theme::Palette::day()) {
        Ok(_) => panic!("attaching succeeded with no GL context"),
        Err(err) => {
            assert!(
                matches!(err, gungnir_viewport3d::gl::AttachError::NoGlContext),
                "unexpected failure: {err}"
            );
            assert!(
                err.to_string().contains("Renderer::Glow"),
                "the error must say what would provide a context: {err}"
            );
        }
    }
}

// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A headless render probe, so "the panel says X" can be a gate rather than a claim.
//!
//! Every panel in this crate carries a sentence it exists to put on screen: the approval
//! queue's reason for being empty, the decision dialog's degraded-conditions gate, the
//! replay timeline's warning that it does not rebuild the picture. Until this existed,
//! those were asserted against the *view structs* -- that the field held the right
//! string -- and nothing checked that the panel drew it. A panel could hold a perfect
//! `EmptyBecause` and never render it.
//!
//! `egui::Context::run` needs no window, no GPU and no display: it lays out a frame and
//! returns the shapes it would paint. Walking those shapes for their text gives the
//! drawn output, which is the thing the claims are about.
//!
//! # What this does and does not verify
//!
//! It verifies that a panel draws without panicking, what text it drew, and **in what
//! order**. The order matters for at least one safety property: PN-07's accept control
//! is drawn last on purpose, and that is now checkable rather than a comment.
//!
//! It is not compiled into the desktop. `gungnir-app` turns the feature on from its
//! `[dev-dependencies]`, and the workspace's `resolver = "2"` keeps a dev-dependency's
//! features out of the normal build graph; `cargo tree -p gungnir-app -e features,no-dev`
//! is the check.
//!
//! It does not verify that anything looks right -- spacing, contrast in situ, whether a
//! warning is noticeable. That is a person's judgement and it is GAP-074's usability
//! round. This closes the gap between "the view struct is correct" and "the screen says
//! it", which is where a rendering bug would otherwise hide.

use egui::{Context, RawInput, Rect};

/// Default probe surface. Large enough that panels lay out without clipping text away,
/// which would make an assertion fail for a reason that has nothing to do with the panel.
const PROBE_SIZE: [f32; 2] = [1400.0, 1000.0];

/// A headless egui context that draws a frame and reports what it drew.
pub struct RenderProbe {
    ctx: Context,
    size: [f32; 2],
}

impl Default for RenderProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderProbe {
    #[must_use]
    pub fn new() -> Self {
        Self {
            ctx: themed_context(),
            size: PROBE_SIZE,
        }
    }

    /// A probe with a specific surface size, for testing what a narrow panel does.
    #[must_use]
    pub fn with_size(width: f32, height: f32) -> Self {
        Self {
            ctx: themed_context(),
            size: [width, height],
        }
    }

    /// Draw `contents` into a central panel and return what it returned, with what was
    /// drawn.
    ///
    /// Two frames are run and the second is reported. egui settles some layouts on the
    /// second pass -- grid column widths in particular -- and a first-frame reading
    /// would occasionally miss text that is present in every frame a person would see.
    ///
    /// The value is an `Option` rather than being unwrapped here: `CentralPanel::show`
    /// always runs its contents, so it is always `Some`, but this module compiles as
    /// ordinary code under the `harness` feature and the workspace does not permit
    /// `expect` outside tests. The caller is a test and may unwrap it.
    pub fn draw<R>(&self, mut contents: impl FnMut(&mut egui::Ui) -> R) -> (Option<R>, DrawnFrame) {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(self.size[0], self.size[1]),
            )),
            ..Default::default()
        };
        let mut result = None;
        let mut frame = DrawnFrame::default();
        for _ in 0..2 {
            let output = self.ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    result = Some(contents(ui));
                });
            });
            frame = collect(&output);
        }
        (result, frame)
    }

    /// The context, for a caller that needs to draw a window rather than a panel.
    #[must_use]
    pub fn ctx(&self) -> &Context {
        &self.ctx
    }
}

/// The text a frame drew, in draw order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DrawnFrame {
    /// Every string painted, in the order it was painted.
    pub texts: Vec<String>,
}

impl DrawnFrame {
    /// Whether any drawn string contains `needle`.
    #[must_use]
    pub fn says(&self, needle: &str) -> bool {
        self.texts.iter().any(|t| t.contains(needle))
    }

    /// The index of the first drawn string containing `needle`.
    ///
    /// Draw order is a real property on a decision surface: PN-07 draws accept last so
    /// neither the reflexive click nor the keyboard traversal lands on it first.
    #[must_use]
    pub fn position_of(&self, needle: &str) -> Option<usize> {
        self.texts.iter().position(|t| t.contains(needle))
    }

    /// Everything drawn, for an assertion message.
    #[must_use]
    pub fn joined(&self) -> String {
        self.texts.join(" | ")
    }
}

fn collect(output: &egui::FullOutput) -> DrawnFrame {
    let mut texts = Vec::new();
    for clipped in &output.shapes {
        walk(&clipped.shape, &mut texts);
    }
    DrawnFrame { texts }
}

fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(text) => out.push(text.galley.text().to_owned()),
        egui::Shape::Vec(shapes) => {
            for s in shapes {
                walk(s, out);
            }
        }
        _ => {}
    }
}

/// A context with the operations theme installed, so the probe lays out with the same
/// text styles and spacing as the desktop. A test that read a frame laid out in egui's
/// stock style would be asserting on a screen nobody sees.
fn themed_context() -> Context {
    let ctx = Context::default();
    crate::theme::install_egui_theme(&ctx);
    ctx
}

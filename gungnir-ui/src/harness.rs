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
//! `egui::Context::run_ui` needs no window, no GPU and no display: it lays out a frame and
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

use egui::{Context, Event, Modifiers, PointerButton, RawInput, Rect};

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
    /// The value is an `Option` rather than being unwrapped here: `CentralPanel::show_inside`
    /// always runs its contents, so it is always `Some`, but this module compiles as
    /// ordinary code under the `harness` feature and the workspace does not permit
    /// `expect` outside tests. The caller is a test and may unwrap it.
    pub fn draw<R>(&self, mut contents: impl FnMut(&mut egui::Ui) -> R) -> (Option<R>, DrawnFrame) {
        let mut result = None;
        let mut frame = DrawnFrame::default();
        for _ in 0..2 {
            let (r, f) = self.run_frame(Vec::new(), &mut contents);
            result = r;
            frame = f;
        }
        (result, frame)
    }

    /// Draw `contents`, then click the first drawn string containing `needle`, and report
    /// what the frame that saw the release returned and drew.
    ///
    /// A click in egui is a press and a release over the same widget on consecutive
    /// frames, and a widget is only hit-testable once a previous frame has laid it out,
    /// so this runs the two settling frames of [`draw`](Self::draw), then a frame with the
    /// pointer moved to the text and pressed, then a frame with it released. The result
    /// is the release frame's, which is the one a `clicked()` fires in.
    ///
    /// `Err` carries the settled frame when nothing drawn contains `needle`, so a test
    /// can print what was drawn instead; a click on text that is not there is not a
    /// click on anything.
    pub fn click<R>(
        &self,
        needle: &str,
        mut contents: impl FnMut(&mut egui::Ui) -> R,
    ) -> Result<(Option<R>, DrawnFrame), DrawnFrame> {
        let (_, settled) = self.draw(&mut contents);
        let Some(target) = settled.rect_of(needle) else {
            return Err(settled);
        };
        let at = target.center();
        let press = vec![
            Event::PointerMoved(at),
            Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ];
        self.run_frame(press, &mut contents);
        let release = vec![Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }];
        Ok(self.run_frame(release, &mut contents))
    }

    /// One frame with `events` as its input.
    ///
    /// The same central panel the desktop draws its viewport in (egui 0.34 puts it inside
    /// the frame's root `Ui` rather than on the context).
    fn run_frame<R>(
        &self,
        events: Vec<Event>,
        contents: &mut impl FnMut(&mut egui::Ui) -> R,
    ) -> (Option<R>, DrawnFrame) {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(self.size[0], self.size[1]),
            )),
            events,
            ..Default::default()
        };
        let mut result = None;
        let output = self.ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show_inside(ui, |ui| {
                result = Some(contents(ui));
            });
        });
        (result, collect(&output))
    }

    /// The context, for a caller that needs to draw a window rather than a panel.
    #[must_use]
    pub fn ctx(&self) -> &Context {
        &self.ctx
    }
}

/// The text a frame drew, in draw order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DrawnFrame {
    /// Every string painted, in the order it was painted.
    pub texts: Vec<String>,
    /// Where each string in `texts` was painted, index for index, so a probe can put
    /// the pointer on one.
    pub rects: Vec<Rect>,
}

impl DrawnFrame {
    /// Where the first drawn string containing `needle` was painted.
    #[must_use]
    pub fn rect_of(&self, needle: &str) -> Option<Rect> {
        self.position_of(needle)
            .and_then(|i| self.rects.get(i).copied())
    }

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
    let mut frame = DrawnFrame::default();
    for clipped in &output.shapes {
        walk(&clipped.shape, &mut frame);
    }
    frame
}

fn walk(shape: &egui::Shape, out: &mut DrawnFrame) {
    match shape {
        egui::Shape::Text(text) => {
            out.texts.push(text.galley.text().to_owned());
            out.rects
                .push(Rect::from_min_size(text.pos, text.galley.size()));
        }
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
///
/// Always the day palette (GAP-095): this probe is for testing what a panel draws, not
/// for testing the theme itself -- `gungnir-ui/src/theme.rs`'s own tests cover the
/// night variant -- and a fixed, known palette keeps every test that reads colours off
/// a rendered frame independent of which `ConfigBaseline` setting a real deployment
/// might pick.
fn themed_context() -> Context {
    let ctx = Context::default();
    crate::theme::install_egui_theme(&ctx, &crate::theme::Palette::day());
    ctx
}

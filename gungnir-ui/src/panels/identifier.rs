// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! A whole identifier, drawn with a control that copies it (D-61, GAP-130).
//!
//! Since GAP-130 a decision and a plan are UUID v7 identifiers, 36 characters long. A panel
//! row or an alert shows the short tag, `…9f3a61c2`, which is enough to tell the rows on
//! one screen apart. PN-07 and the handoff record on PN-20 show all of it, because they are
//! where a person quotes a decision to somebody else -- a call to the battery, a question
//! to the node's supervisor, a search of the journal -- and a tag two identifiers can
//! share is not a thing to quote. The control puts the identifier in egui's copied-text
//! output, so nobody has to retype it.

use crate::theme;
use egui::{RichText, Ui};

/// The copy control's label.
pub const COPY_LABEL: &str = "Copy";

/// Draw `label`, the whole identifier in the monospace face, and a control that copies it.
///
/// Returns whether the control was clicked this frame, for a caller that says so.
pub fn draw_full(ui: &mut Ui, palette: &theme::Palette, label: &str, full: &str) -> bool {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(palette.muted_text_color()));
        ui.label(theme::numeral(palette, full));
        let clicked = ui
            .small_button(COPY_LABEL)
            .on_hover_text("Copy the whole identifier")
            .clicked();
        if clicked {
            ui.ctx().copy_text(full.to_owned());
        }
        clicked
    })
    .inner
}

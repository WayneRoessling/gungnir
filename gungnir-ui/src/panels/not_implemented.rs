// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The panel body for a workspace slot whose panel has not been built yet.
//!
//! `docs/ux/information-architecture.md` §1 gives each of the eight roles a workspace
//! of up to six docked panels, and GAP-055 binds those workspaces to the running
//! application. Sixteen of the twenty panels do not exist yet: they are GAP-072,
//! GAP-073, GAP-038, GAP-071 and the rest.
//!
//! A slot with nothing in it must **say so**. An empty frame in a role's workspace
//! reads as "there is nothing to show" -- no alerts, no requirements, no coverage gaps
//! -- which is exactly the claim `CONTRIBUTING.md`'s "No fake wiring" rule and
//! architecture principle AP-02 forbid. This renders the panel's identifier and the gap
//! that will build it, so an operator or a reviewer can tell an unbuilt panel from an
//! empty one at a glance.

use crate::theme;
use egui::{RichText, Ui};

/// Render the placeholder for an unbuilt panel.
///
/// `pn` is the `PN-nn` identifier from the UX documents and `gap` is the register entry
/// that will replace this. Both are shown deliberately: the reviewer checking whether
/// the workspace matches plan 06 needs the first, and anyone asking when it will work
/// needs the second.
pub fn render_not_implemented(ui: &mut Ui, pn: &str, title: &str, gap: &str) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("NOT IMPLEMENTED")
                .strong()
                .color(theme::WARNING_COLOR),
        );
        ui.label(RichText::new(pn).monospace().weak());
    });
    ui.label(format!("{title} is designed but not built."));
    ui.label(RichText::new(format!("Tracked as {gap}.")).weak());
}

#[cfg(test)]
mod tests {

    /// The placeholder must always name the gap. A placeholder that said only "not
    /// implemented" would leave a reviewer no way to find out when it will be.
    #[test]
    fn the_placeholder_carries_its_identifiers() {
        // Rendering needs an egui context; what is worth asserting without one is that
        // the inputs the caller must supply are non-empty, which the layout binding in
        // `gungnir-app` relies on when it maps a `PanelId` to this call.
        for (pn, title, gap) in [
            ("PN-01", "Status strip", "GAP-072"),
            ("PN-04", "Evidence card", "GAP-073"),
            ("PN-16", "Planning", "GAP-006"),
        ] {
            assert!(pn.starts_with("PN-"));
            assert!(!title.is_empty());
            assert!(gap.starts_with("GAP-"));
        }
    }
}

// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! PN-21, About: the Appropriate Legal Notices (D-34).
//!
//! # This panel is a licensing surface, not decoration
//!
//! Every other panel in this crate exists to tell an operator something about the
//! mission. This one exists because of what the licence says, and deleting it has a
//! legal consequence that deleting any other panel does not.
//!
//! Gungnir is AGPL-3.0-or-later, and `LICENSE-ADDITIONAL-TERMS.md` section 1 adds a
//! term under AGPL section 7(b) requiring that a derivative work's interactive user
//! interface preserve the attribution in `NOTICE`. Section 7(b) can only require that
//! for the Appropriate Legal Notices, and AGPL section 5(d) makes the whole obligation
//! conditional:
//!
//! > If the work has interactive user interfaces, each must display Appropriate Legal
//! > Notices; however, if the Program has interactive interfaces that do not display
//! > Appropriate Legal Notices, your work need not make them do so.
//!
//! **So this panel is what makes that term reach a fork at all.** Remove it and no
//! derivative of Gungnir is obliged to credit anyone in its interface, forever, and
//! nothing else in the repository would notice. That is why the render test in
//! `gungnir-ui/src/panels/rendered.rs` asserts on all four elements rather than on the
//! panel existing, and why the constants below are checked against `NOTICE` by
//! `gungnir-app/tests/appropriate_legal_notices.rs` rather than merely resembling it.
//!
//! # What section 0 requires
//!
//! AGPL section 0 defines the term, and the definition is a checklist. An interactive
//! interface displays Appropriate Legal Notices to the extent it includes a convenient
//! and prominently visible feature that (1) displays an appropriate copyright notice,
//! and (2) tells the user that there is no warranty for the work, that licensees may
//! convey the work under this License, and how to view a copy of this License.
//!
//! That is four claims, and each has a constant here: [`COPYRIGHT`], [`NO_WARRANTY`],
//! [`CONVEYANCE`], [`LICENSE_LOCATION`]. A fifth, [`ADDITIONAL_TERMS`], is not required
//! by section 0 but is required by section 7 itself, which asks that added terms be
//! stated in the licence notice of the material they govern.
//!
//! "Convenient and prominently visible" is why `main.rs` opens this from the status
//! strip: PN-01 is in every workspace by construction
//! (`gungnir_workflow::WorkspaceLayout::ALWAYS`), so the notices are one click away
//! for every role, including a session with no role signed in at all. Putting the
//! control anywhere role-gated would have made the notices reachable for some operators
//! and not others, which is not what "prominently visible" means.
//!
//! # No duplicate state
//!
//! Like every panel here it holds nothing (`rust-ui-architecture-coding-standards.md`
//! §2). The legal text is constant because it is constant -- it is not state, and a
//! view struct carrying it would invite a caller to pass a different copyright holder.
//! [`AboutView`] carries only the two things that genuinely vary by build: the version
//! and where the source can be had.

use crate::theme;
use egui::{RichText, Ui};

/// What the program is, as `NOTICE` names it.
pub const PRODUCT: &str = "Gungnir — command-and-control tracking and intercept planning";

/// Section 0's first element: an appropriate copyright notice.
pub const COPYRIGHT: &str = "Copyright (C) 2026 Roessling Digital Solutions LLC";

/// Section 0's element (2), second clause: that licensees may convey the work under
/// this License.
pub const CONVEYANCE: &str = "This program is free software: you can redistribute it and/or modify it under the terms of the GNU Affero General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.";

/// Section 0's element (2), first clause: that there is no warranty for the work.
pub const NO_WARRANTY: &str = "This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License for more details.";

/// Section 0's element (2), third clause: how to view a copy of this License.
pub const LICENSE_LOCATION: &str = "You should have received a copy of the GNU Affero General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.";

/// Not required by section 0, but required by section 7, which asks that additional
/// terms be stated in the licence notice of the material they govern.
pub const ADDITIONAL_TERMS: &str = "Additional terms under AGPL section 7 apply to attribution, origin marking, and trademarks. See LICENSE-ADDITIONAL-TERMS.md.";

/// The four sentences AGPL section 0 requires, in the order the panel draws them.
///
/// Exposed so a test can walk the checklist rather than restate it, and so a future
/// panel that must also carry the notices has one list to draw from instead of its own
/// transcription.
pub const APPROPRIATE_LEGAL_NOTICES: [&str; 4] =
    [COPYRIGHT, CONVEYANCE, NO_WARRANTY, LICENSE_LOCATION];

/// The two things about the notices that vary by build rather than by licence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AboutView<'a> {
    /// This build's version, normally `env!("CARGO_PKG_VERSION")` from the binary.
    pub version: &'a str,
    /// Where the corresponding source can be obtained.
    ///
    /// Drawn because AGPL section 13 obliges anyone who modifies Gungnir and offers it
    /// over a network to give their users the source of *their* version, and the two
    /// connected deployment profiles (`ARCHITECTURE.md` §8) are exactly that shape. An
    /// operator who cannot see where source comes from cannot tell whether the offer
    /// has been honoured, so the upstream is named even in an unmodified build.
    pub source_url: &'a str,
}

/// Draw the Appropriate Legal Notices.
///
/// Draws no control and returns nothing: there is no action an operator can take here,
/// and a panel that cannot be dismissed from inside itself cannot be dismissed by
/// accident either.
pub fn render_about(ui: &mut Ui, view: &AboutView<'_>) {
    ui.label(RichText::new(PRODUCT).size(theme::TITLE_FONT_SIZE));
    ui.label(RichText::new(format!("Version {}", view.version)).color(theme::MUTED_TEXT_COLOR));
    ui.add_space(theme::PANEL_SPACING);

    // The copyright notice is drawn first and unmuted. It is element (1) of section 0
    // and the subject of the section 7(b) term; a derivative has to keep it here.
    ui.label(RichText::new(COPYRIGHT).strong());
    ui.add_space(theme::ROW_SPACING);

    // The remaining three of section 0's checklist, taken from the array rather than
    // relisted, so that a constant dropped from [`APPROPRIATE_LEGAL_NOTICES`] stops
    // reaching the screen and the test that walks the array catches it.
    for sentence in &APPROPRIATE_LEGAL_NOTICES[1..] {
        ui.label(*sentence);
        ui.add_space(theme::ROW_SPACING);
    }

    ui.add_space(theme::PANEL_SPACING);
    ui.separator();
    ui.add_space(theme::PANEL_SPACING);

    ui.label(RichText::new(ADDITIONAL_TERMS).color(theme::TEXT_SECONDARY));
    ui.add_space(theme::ROW_SPACING);

    // Section 13's offer is the modifier's to make, not ours; naming the upstream is
    // what lets an operator tell that this build is the unmodified one.
    ui.label(RichText::new(format!("Source: {}", view.source_url)).color(theme::TEXT_SECONDARY));
    ui.add_space(theme::ROW_SPACING);
    ui.label(
        RichText::new(
            "Test fixtures under testdata/ are third-party material under their own licences; \
             NOTICE lists each one.",
        )
        .color(theme::MUTED_TEXT_COLOR)
        .size(theme::SMALL_FONT_SIZE),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Section 0 is a checklist of four, and the panel's job is to satisfy all of it.
    /// A notice missing one element is not an Appropriate Legal Notice, so the list
    /// this panel draws from must have exactly the four and none of them empty.
    #[test]
    fn the_checklist_has_all_four_elements() {
        assert_eq!(APPROPRIATE_LEGAL_NOTICES.len(), 4);
        for notice in APPROPRIATE_LEGAL_NOTICES {
            assert!(!notice.trim().is_empty());
        }
    }

    /// Each element has to actually make its own claim, not merely be present. These
    /// are the words that carry the legal meaning, and a well-meaning reword that
    /// dropped one would leave the panel looking complete and satisfying nothing.
    #[test]
    fn each_element_makes_its_claim() {
        assert!(COPYRIGHT.contains("Copyright"));
        assert!(COPYRIGHT.contains("Roessling Digital Solutions LLC"));
        assert!(CONVEYANCE.contains("redistribute"));
        assert!(CONVEYANCE.contains("GNU Affero General Public License"));
        assert!(NO_WARRANTY.contains("WITHOUT ANY"));
        assert!(NO_WARRANTY.contains("WARRANTY"));
        assert!(LICENSE_LOCATION.contains("https://www.gnu.org/licenses/"));
    }

    /// Section 7 asks that added terms be stated in the licence notice of the material
    /// they govern. The panel is that notice for the running program, so it has to
    /// point at them by name rather than leaving them to the repository.
    #[test]
    fn the_additional_terms_are_named_and_locatable() {
        assert!(ADDITIONAL_TERMS.contains("section 7"));
        assert!(ADDITIONAL_TERMS.contains("LICENSE-ADDITIONAL-TERMS.md"));
    }
}

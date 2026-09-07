// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! PN-21 against the repository's own licence files (D-34).
//!
//! `gungnir-ui`'s render test proves the panel *paints* the notices. This one proves
//! they are the *right* notices: that the constants in `panels::about` still say what
//! `NOTICE` says, and that every role can reach them.
//!
//! # Why a test and not a review
//!
//! Two documents have to agree and neither compiles: `NOTICE` at the workspace root is
//! what `LICENSE-ADDITIONAL-TERMS.md` section 1 obliges a derivative work to preserve,
//! and `gungnir-ui/src/panels/about.rs` is what the running program actually shows an
//! operator. If they drift, the program displays an attribution that the licence does
//! not require and omits one it does, and nothing anywhere would fail.
//!
//! Embedding `NOTICE` with `include_str!` would have kept them identical by
//! construction, and was rejected: it puts a file outside the package directory into
//! the crate's build, which breaks `cargo package` -- and
//! `docs/gungnir-workspace-structure.md` now records that publication is no longer
//! blocked by the licence. So the coupling is checked here, at test time, over a
//! runtime path.

use std::path::{Path, PathBuf};

use gungnir_security::Role;
use gungnir_ui::panels::about::{
    ADDITIONAL_TERMS, APPROPRIATE_LEGAL_NOTICES, CONVEYANCE, COPYRIGHT, LICENSE_LOCATION,
    NO_WARRANTY, PRODUCT,
};
use gungnir_workflow::{PanelId, WorkspaceLayout};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits in the workspace")
        .to_path_buf()
}

/// `NOTICE` wraps its sentences to fit a text file; the panel lets egui wrap them to
/// fit a window. Comparing them means comparing the words, not the line breaks.
fn normalise(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn notice() -> String {
    let path = workspace().join("NOTICE");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("NOTICE is readable at {}: {err}", path.display()));
    normalise(&raw)
}

/// The panel's text is `NOTICE`'s text.
///
/// Each constant is checked separately so a failure names which sentence drifted rather
/// than reporting that two long strings differ.
#[test]
fn the_panel_says_what_notice_says() {
    let notice = notice();
    for (name, text) in [
        ("PRODUCT", PRODUCT),
        ("COPYRIGHT", COPYRIGHT),
        ("CONVEYANCE", CONVEYANCE),
        ("NO_WARRANTY", NO_WARRANTY),
        ("LICENSE_LOCATION", LICENSE_LOCATION),
        ("ADDITIONAL_TERMS", ADDITIONAL_TERMS),
    ] {
        assert!(
            notice.contains(&normalise(text)),
            "PN-21's {name} is not in NOTICE, so the panel and the file the licence \
             points at have drifted apart.\n\npanel:  {}\n",
            normalise(text)
        );
    }
}

/// The checklist the panel draws from is the four constants, not a separate copy.
///
/// `render_about` draws element (1) from `COPYRIGHT` and the rest by iterating
/// `APPROPRIATE_LEGAL_NOTICES[1..]`, so a constant dropped from that array stops being
/// painted however correct the constant beside it still is. Section 0 requires all
/// four; this asserts the array is still all four and still the same strings, in the
/// order the panel relies on.
#[test]
fn the_checklist_is_the_four_constants() {
    assert_eq!(
        APPROPRIATE_LEGAL_NOTICES,
        [COPYRIGHT, CONVEYANCE, NO_WARRANTY, LICENSE_LOCATION],
        "PN-21 draws from APPROPRIATE_LEGAL_NOTICES; a constant that left the array \
         stops reaching the screen"
    );
}

/// The licence text the notices point at has to actually be in the repository.
///
/// AGPL section 0's fourth element is *how to view a copy of this License*. The panel
/// tells an operator they should have received one with the program; that claim is only
/// true while the file is here and is the AGPL.
#[test]
fn the_license_the_notices_point_at_is_present() {
    let license = std::fs::read_to_string(workspace().join("LICENSE"))
        .expect("LICENSE is readable at the workspace root");
    assert!(
        license.contains("GNU AFFERO GENERAL PUBLIC LICENSE"),
        "LICENSE is not the AGPL"
    );
    assert!(license.contains("Version 3, 19 November 2007"));

    let terms = std::fs::read_to_string(workspace().join("LICENSE-ADDITIONAL-TERMS.md"))
        .expect("LICENSE-ADDITIONAL-TERMS.md is readable at the workspace root");
    assert!(
        terms.contains("7(b)"),
        "the additional terms no longer state the attribution clause PN-21 exists to serve"
    );
}

/// Every role can open PN-21, whatever its layout says.
///
/// This is the property AGPL section 0's "prominently visible" turns into code. A role
/// added later inherits it from `WorkspaceLayout::ALWAYS_AVAILABLE` rather than from
/// somebody remembering to list the panel, and this test is what would fail if that
/// list stopped being consulted.
#[test]
fn every_role_can_open_the_notices() {
    for role in [
        Role::Operator,
        Role::Supervisor,
        Role::Analyst,
        Role::SensorManager,
        Role::Administrator,
        Role::IntelligenceAnalyst,
        Role::Planner,
        Role::Commander,
    ] {
        let layout = WorkspaceLayout::for_role(role);
        assert!(
            layout.may_open(PanelId::About),
            "{role:?} cannot open PN-21, so the notices are not prominently visible for it"
        );
    }
}

/// PN-21 occupies no role's docked layout.
///
/// The counterpart to the test above: reachable from everywhere, docked nowhere. If it
/// ever appears in a role's `panels` list, the reachability has been re-implemented by
/// hand for that role and `ALWAYS_AVAILABLE` has stopped being the single source of it.
#[test]
fn the_notices_are_docked_in_no_layout() {
    for role in [
        Role::Operator,
        Role::Supervisor,
        Role::Analyst,
        Role::SensorManager,
        Role::Administrator,
        Role::IntelligenceAnalyst,
        Role::Planner,
        Role::Commander,
    ] {
        let layout = WorkspaceLayout::for_role(role);
        assert!(
            !layout.panels.contains(&PanelId::About),
            "{role:?} docks PN-21; it is meant to be opened from the status strip"
        );
        assert!(
            !layout.on_demand.contains(&PanelId::About),
            "{role:?} lists PN-21 in on_demand; ALWAYS_AVAILABLE already covers it, and \
             a per-role copy will rot"
        );
    }
}

/// PN-21 is a real panel, not a placeholder.
///
/// `workspace::render_panel` falls through to the not-implemented placeholder for
/// anything it has no arm for. For every other panel that is the honest answer; for
/// this one it would report the panel the licence requires as missing, in a build where
/// it is present.
#[test]
fn the_notices_are_not_reported_as_unbuilt() {
    assert!(gungnir_app::workspace::is_implemented(PanelId::About));
    assert_eq!(PanelId::About.pn(), "PN-21");
    assert!(gungnir_config::validate_panel_id("PN-21"));
}

// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Docking and multi-window (D-17, D-19, GAP-075).
//!
//! `gungnir_workflow::WorkspaceLayout::for_role` says which panels a role has; until now
//! `main.rs` stacked them down one fixed side panel. This renders them as an
//! `egui_tiles` tree the operator can rearrange, and draws the panels D-17 allows to be
//! detached in their own native window.
//!
//! # The arrangement round-trips through the baseline's own vocabulary
//!
//! [`gungnir_model::LayoutNode`] is the persisted description and `egui_tiles::Tree` is
//! the live one, and the conversion goes both ways: the tree is built from the
//! arrangement at launch, and read back out of the tree after the operator has dragged
//! things. Reading it back is what makes "save this arrangement" possible at all, and it
//! is the direction that can silently lose information -- so
//! [`tests::an_arrangement_survives_the_round_trip`] checks the identity rather than
//! trusting the two functions to agree.
//!
//! # What may be detached, and what may not
//!
//! D-17 allows the viewport, the approval queue and the replay timeline in a second
//! window, and says decision dialogs stay with the queue. That rule is enforced in
//! `gungnir-config`'s validation, so a baseline cannot configure a detached decision
//! dialog; here it decides which window PN-07 is drawn in, which is the same rule seen
//! from the other end. A decision separated from the queue it came from is a decision
//! taken without its context.

use crate::state::AppState;
use crate::sustainment::SustainmentState;
use crate::workspace::PanelAction;
use gungnir_model::LayoutNode;
use gungnir_workflow::PanelId;

/// One pane of the dock tree.
///
/// A `PanelId` rather than a rendered closure, so the tree stays data: it can be built
/// from a baseline, compared in a test, and read back out without any of that touching
/// egui.
pub type Pane = PanelId;

/// The panels D-17 allows in a second window, as `PanelId`s.
///
/// The same closed list `gungnir_config::DETACHABLE_PANELS` holds as strings; a test
/// asserts the two agree, because the config crate deliberately keeps no edge to
/// `gungnir-workflow` and two lists that can drift are worth one assertion.
pub const DETACHABLE: [PanelId; 3] = [PanelId::Viewport3d, PanelId::ApprovalQueue, PanelId::Replay];

/// Whether this panel may be shown in its own window.
#[must_use]
pub fn may_detach(panel: PanelId) -> bool {
    DETACHABLE.contains(&panel)
}

/// Resolve a `PN-xx` identifier to a panel.
#[must_use]
pub fn panel_for_pn(pn: &str) -> Option<PanelId> {
    PanelId::ALL.iter().copied().find(|p| p.pn() == pn)
}

/// Build a live tree from a persisted arrangement.
///
/// Unknown panel identifiers are skipped rather than panicking. They should never reach
/// here -- `gungnir_config::validate` refuses a baseline that names one -- but a tree
/// that panicked on a bad name would take the whole desktop down over a typo in a
/// configuration file, which is the wrong failure for a layout.
#[must_use]
pub fn tree_from(arrangement: &LayoutNode) -> egui_tiles::Tree<Pane> {
    let mut tiles = egui_tiles::Tiles::default();
    let root = insert_node(&mut tiles, arrangement)
        .unwrap_or_else(|| tiles.insert_pane(PanelId::TrackTable));
    egui_tiles::Tree::new("workspace_dock", root, tiles)
}

fn insert_node(
    tiles: &mut egui_tiles::Tiles<Pane>,
    node: &LayoutNode,
) -> Option<egui_tiles::TileId> {
    match node {
        LayoutNode::Panel { pn } => panel_for_pn(pn).map(|p| tiles.insert_pane(p)),
        LayoutNode::Tabs { children } => {
            let ids = insert_children(tiles, children)?;
            Some(tiles.insert_tab_tile(ids))
        }
        LayoutNode::Horizontal { children, shares } => {
            let ids = insert_children(tiles, children)?;
            Some(insert_linear(
                tiles,
                egui_tiles::LinearDir::Horizontal,
                &ids,
                shares,
            ))
        }
        LayoutNode::Vertical { children, shares } => {
            let ids = insert_children(tiles, children)?;
            Some(insert_linear(
                tiles,
                egui_tiles::LinearDir::Vertical,
                &ids,
                shares,
            ))
        }
    }
}

fn insert_children(
    tiles: &mut egui_tiles::Tiles<Pane>,
    children: &[LayoutNode],
) -> Option<Vec<egui_tiles::TileId>> {
    let ids: Vec<egui_tiles::TileId> = children
        .iter()
        .filter_map(|c| insert_node(tiles, c))
        .collect();
    (!ids.is_empty()).then_some(ids)
}

fn insert_linear(
    tiles: &mut egui_tiles::Tiles<Pane>,
    dir: egui_tiles::LinearDir,
    ids: &[egui_tiles::TileId],
    shares: &[f32],
) -> egui_tiles::TileId {
    let mut linear = egui_tiles::Linear::new(dir, ids.to_vec());
    if shares.len() == ids.len() {
        for (id, share) in ids.iter().zip(shares) {
            linear.shares.set_share(*id, *share);
        }
    }
    tiles.insert_container(linear)
}

/// Read a live tree back out as a persisted arrangement.
///
/// `None` when the tree has no root, which an empty workspace would produce.
#[must_use]
pub fn arrangement_from(tree: &egui_tiles::Tree<Pane>) -> Option<LayoutNode> {
    node_from(tree, tree.root()?)
}

fn node_from(tree: &egui_tiles::Tree<Pane>, id: egui_tiles::TileId) -> Option<LayoutNode> {
    match tree.tiles.get(id)? {
        egui_tiles::Tile::Pane(panel) => Some(LayoutNode::Panel {
            pn: panel.pn().to_owned(),
        }),
        egui_tiles::Tile::Container(container) => match container {
            egui_tiles::Container::Tabs(tabs) => Some(LayoutNode::Tabs {
                children: children_from(tree, &tabs.children),
            }),
            egui_tiles::Container::Linear(linear) => {
                let children = children_from(tree, &linear.children);
                let shares: Vec<f32> = linear.children.iter().map(|c| linear.shares[*c]).collect();
                match linear.dir {
                    egui_tiles::LinearDir::Horizontal => {
                        Some(LayoutNode::Horizontal { children, shares })
                    }
                    egui_tiles::LinearDir::Vertical => {
                        Some(LayoutNode::Vertical { children, shares })
                    }
                }
            }
            // Never constructed here, so a grid can only appear if a future egui_tiles
            // interaction makes one. Reported as tabs rather than dropped: losing the
            // panels would be worse than losing the geometry.
            egui_tiles::Container::Grid(grid) => Some(LayoutNode::Tabs {
                children: children_from(tree, &grid.children().copied().collect::<Vec<_>>()),
            }),
        },
    }
}

fn children_from(tree: &egui_tiles::Tree<Pane>, ids: &[egui_tiles::TileId]) -> Vec<LayoutNode> {
    ids.iter().filter_map(|c| node_from(tree, *c)).collect()
}

/// The default arrangement for a role: its docked panels stacked in the order
/// `WorkspaceLayout::for_role` gives, minus the viewport, which has the central area.
#[must_use]
pub fn default_arrangement(layout: &gungnir_workflow::WorkspaceLayout) -> LayoutNode {
    let panels: Vec<&str> = layout
        .docked()
        .filter(|p| *p != PanelId::Viewport3d)
        .map(PanelId::pn)
        .collect();
    if panels.is_empty() {
        // Every role has docked panels, so this is unreachable through
        // `WorkspaceLayout`; a layout that drew nothing would be worse than one that
        // drew the track table.
        return LayoutNode::Panel {
            pn: PanelId::TrackTable.pn().to_owned(),
        };
    }
    LayoutNode::stack(&panels)
}

/// Renders one pane of the dock tree.
///
/// Borrows what the panels read rather than owning it, so the tree stays a description
/// of the arrangement and the panels stay the only place that knows how to draw. The
/// clicked action is collected here and applied after the frame, which is the same
/// one-way flow the docked panels had before there was a tree.
pub struct PanelBehavior<'a> {
    pub state: &'a AppState,
    /// Mutable because PN-15 and PN-13 write into their drafts as the operator types.
    /// The other two panels here only read theirs.
    pub sustainment: &'a mut SustainmentState,
    /// What the operator clicked in a pane this frame, if anything.
    pub action: Option<PanelAction>,
}

impl<'a> PanelBehavior<'a> {
    #[must_use]
    pub fn new(state: &'a AppState, sustainment: &'a mut SustainmentState) -> Self {
        Self {
            state,
            sustainment,
            action: None,
        }
    }

    /// Draw one panel, wherever it is: a docked pane or a detached window.
    ///
    /// The three sustainment panels are drawn here rather than through
    /// `workspace::render_panel` because they read state this window owns.
    pub fn draw_panel(&mut self, ui: &mut egui::Ui, panel: PanelId) -> Option<PanelAction> {
        match panel {
            PanelId::Replay => crate::workspace::render_replay(
                ui,
                self.state,
                &self.sustainment.replay,
                &self.sustainment.sessions,
            ),
            // PN-13 writes into the review draft as the reviewer types (GAP-049), the
            // same reason PN-15 takes its draft mutably.
            PanelId::Reports => crate::workspace::render_reports(ui, self.state, self.sustainment),
            PanelId::ConfigEditor => {
                crate::workspace::render_config_editor(ui, self.state, &self.sustainment.config)
            }
            PanelId::Requirements => crate::workspace::render_requirements(
                ui,
                self.state,
                &mut self.sustainment.requirements,
            ),
            // PN-20 edits the sign-in draft in place (GAP-057).
            PanelId::Audit => crate::workspace::render_audit(ui, self.state, self.sustainment),
            other => crate::workspace::render_panel(ui, other, self.state),
        }
    }
}

impl egui_tiles::Behavior<Pane> for PanelBehavior<'_> {
    fn pane_ui(
        &mut self,
        ui: &mut egui::Ui,
        _tile_id: egui_tiles::TileId,
        pane: &mut Pane,
    ) -> egui_tiles::UiResponse {
        let drawn = self.draw_panel(ui, *pane);
        self.action = drawn.or(self.action.take());
        egui_tiles::UiResponse::None
    }

    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        pane.title().into()
    }

    // DS-07: the dock's chrome takes the theme tokens rather than egui_tiles's
    // defaults, so the tab bar is the same surface as the window behind it, an active
    // tab is the panel it opens, and a tab title reads as text rather than as a button.
    fn tab_bar_height(&self, _style: &egui::Style) -> f32 {
        self.state.palette.tab_bar_height
    }

    fn tab_bar_color(&self, _visuals: &egui::Visuals) -> egui::Color32 {
        self.state.palette.app_background
    }

    fn tab_bg_color(
        &self,
        _visuals: &egui::Visuals,
        _tiles: &egui_tiles::Tiles<Pane>,
        _tile_id: egui_tiles::TileId,
        state: &egui_tiles::TabState,
    ) -> egui::Color32 {
        if state.active {
            self.state.palette.panel_background
        } else {
            egui::Color32::TRANSPARENT
        }
    }

    fn tab_outline_stroke(
        &self,
        _visuals: &egui::Visuals,
        _tiles: &egui_tiles::Tiles<Pane>,
        _tile_id: egui_tiles::TileId,
        state: &egui_tiles::TabState,
    ) -> egui::Stroke {
        if state.active {
            egui::Stroke::new(
                self.state.palette.stroke_hairline,
                self.state.palette.border_subtle,
            )
        } else {
            egui::Stroke::NONE
        }
    }

    fn tab_bar_hline_stroke(&self, _visuals: &egui::Visuals) -> egui::Stroke {
        egui::Stroke::new(
            self.state.palette.stroke_hairline,
            self.state.palette.border_subtle,
        )
    }

    fn tab_text_color(
        &self,
        _visuals: &egui::Visuals,
        _tiles: &egui_tiles::Tiles<Pane>,
        _tile_id: egui_tiles::TileId,
        state: &egui_tiles::TabState,
    ) -> egui::Color32 {
        if state.active {
            self.state.palette.text_primary
        } else {
            self.state.palette.text_secondary
        }
    }

    fn drag_preview_stroke(&self, _visuals: &egui::Visuals) -> egui::Stroke {
        egui::Stroke::new(
            self.state.palette.stroke_emphasis,
            self.state.palette.focus_color,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_security::Role;
    use gungnir_workflow::WorkspaceLayout;

    /// The direction that can silently lose information. Building a tree from an
    /// arrangement and reading it back must give the same arrangement, or "save this
    /// layout" would save something the operator is not looking at.
    #[test]
    fn an_arrangement_survives_the_round_trip() {
        let original = LayoutNode::Horizontal {
            children: vec![
                LayoutNode::stack(&["PN-06", "PN-05"]),
                LayoutNode::Tabs {
                    children: vec![
                        LayoutNode::Panel { pn: "PN-03".into() },
                        LayoutNode::Panel { pn: "PN-08".into() },
                    ],
                },
            ],
            shares: vec![0.25, 0.75],
        };
        let tree = tree_from(&original);
        let back = arrangement_from(&tree).expect("a tree has a root");
        assert_eq!(
            back.panels(),
            original.panels(),
            "the round trip lost or reordered panels"
        );
        match (&back, &original) {
            (
                LayoutNode::Horizontal { shares: got, .. },
                LayoutNode::Horizontal { shares: want, .. },
            ) => {
                assert_eq!(got.len(), want.len());
                for (g, w) in got.iter().zip(want) {
                    assert!(
                        (g - w).abs() < 1e-6,
                        "the split geometry changed: {got:?} vs {want:?}"
                    );
                }
            }
            other => panic!("the top-level container changed shape: {other:?}"),
        }
    }

    /// Every role's default arrangement draws every docked panel it has, minus the
    /// viewport, which has the central area. A role whose arrangement dropped a panel
    /// would lose it with nothing on screen to say so.
    #[test]
    fn every_role_default_draws_all_its_docked_panels() {
        for role in Role::ALL {
            let layout = WorkspaceLayout::for_role(*role);
            let arrangement = default_arrangement(&layout);
            let drawn = arrangement.panels();
            for panel in layout.docked() {
                if panel == PanelId::Viewport3d {
                    continue;
                }
                assert!(
                    drawn.contains(&panel.pn()),
                    "{role:?} loses {} from its default arrangement",
                    panel.pn()
                );
            }
            // And it builds into a tree that holds them all.
            let tree = tree_from(&arrangement);
            let back = arrangement_from(&tree).expect("a root");
            assert_eq!(back.panels().len(), drawn.len(), "{role:?}");
        }
    }

    /// The two detachable lists -- this one and `gungnir_config`'s -- are written apart
    /// so the config crate keeps no edge to `gungnir-workflow`. They must agree, or a
    /// baseline could be accepted that this refuses to detach, or the reverse.
    #[test]
    fn the_detachable_lists_agree() {
        let mine: Vec<&str> = DETACHABLE.iter().map(|p| p.pn()).collect();
        let mut mine_sorted = mine.clone();
        mine_sorted.sort_unstable();
        let mut theirs: Vec<&str> = gungnir_config::DETACHABLE_PANELS.to_vec();
        theirs.sort_unstable();
        assert_eq!(mine_sorted, theirs);

        // And the rule that matters: the decision dialog is not among them.
        assert!(!may_detach(PanelId::DecisionDialog));
        assert!(may_detach(PanelId::ApprovalQueue));
    }

    /// `gungnir-config` validates a baseline's arrangement against its own list of
    /// panel identifiers, kept apart so that crate needs no edge to `gungnir-workflow`.
    /// If the two drift, a baseline naming a real new panel would be refused at load
    /// with a message saying it does not exist.
    #[test]
    fn the_config_crate_knows_every_panel() {
        for panel in PanelId::ALL {
            assert!(
                gungnir_config::validate_panel_id(panel.pn()),
                "{} exists but a baseline naming it would be refused",
                panel.pn()
            );
        }
        assert!(!gungnir_config::validate_panel_id("PN-99"));
    }

    /// A detached panel is pruned from the main tree, so it is drawn once rather than
    /// in both windows.
    #[test]
    fn a_detached_panel_leaves_the_main_tree() {
        let layout = WorkspaceLayout::for_role(Role::Operator);
        let arrangement = default_arrangement(&layout);
        assert!(arrangement.panels().contains(&PanelId::ApprovalQueue.pn()));

        let pruned = arrangement
            .without(&[PanelId::ApprovalQueue.pn()])
            .expect("the operator has other panels");
        assert!(!pruned.panels().contains(&PanelId::ApprovalQueue.pn()));

        let tree = tree_from(&pruned);
        let back = arrangement_from(&tree).expect("a root");
        assert!(!back.panels().contains(&PanelId::ApprovalQueue.pn()));
    }

    /// Every `PN-xx` resolves back to the panel it names, so a persisted arrangement
    /// and the running interface cannot disagree about what "PN-06" is.
    #[test]
    fn every_panel_identifier_resolves() {
        for panel in PanelId::ALL {
            assert_eq!(panel_for_pn(panel.pn()), Some(*panel));
        }
        assert_eq!(panel_for_pn("PN-99"), None);
    }

    /// A panel that does not exist is skipped rather than taking the desktop down.
    /// `gungnir_config::validate` refuses such a baseline first, so this is the second
    /// line rather than the first.
    #[test]
    fn an_unknown_panel_does_not_panic_the_tree() {
        let arrangement = LayoutNode::Vertical {
            children: vec![
                LayoutNode::Panel { pn: "PN-99".into() },
                LayoutNode::Panel { pn: "PN-03".into() },
            ],
            shares: Vec::new(),
        };
        let tree = tree_from(&arrangement);
        let back = arrangement_from(&tree).expect("a root");
        assert_eq!(back.panels(), vec!["PN-03"]);
    }
}

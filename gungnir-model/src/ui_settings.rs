// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Per-role window arrangement in the configuration baseline (D-17, GAP-075).
//!
//! D-17 decided that panels may be docked and rearranged within a role's layout, that
//! the viewport, the approval queue and the replay timeline may be detached to a second
//! window, and that **the arrangement is saved per role in the configuration baseline**.
//! This is that schema.
//!
//! # Why this is not the docking crate's own type
//!
//! `egui_tiles` can serialize its `Tree` directly, and using that here would be less
//! code. It is the wrong thing to put in a configuration baseline. The baseline is
//! version-gated, validated, hand-editable and reviewed like any other deployment
//! artefact; a serialized library structure is none of those. It would also make the
//! schema change whenever the crate's internals did, and D-19 pinned `egui_tiles` at
//! 0.10 precisely because upgrades are not free here.
//!
//! [`LayoutNode`] instead describes an arrangement in the vocabulary the design uses --
//! panels, tabs, splits and their shares -- so a person can read a baseline and see the
//! screen it describes, and `gungnir-config` can validate that every panel named is a
//! real one.
//!
//! # Per role, not per operator
//!
//! The arrangement is a deployment decision about what each role's screen looks like,
//! not a personal preference, which is why it lives in the baseline and why changing it
//! durably needs `config.apply`. An operator rearranging panels during a session is a
//! session-local change and is deliberately not written back: a shift's worth of
//! dragging must not silently become the deployment's configuration.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How a role's panels are arranged in a window.
///
/// Recursive so it can describe what the docking tree can: a panel, a stack of tabs, or
/// a split with the share of the space each child takes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LayoutNode {
    /// One panel, named by its `PN-xx` identifier.
    Panel { pn: String },
    /// Panels stacked as tabs, one visible at a time.
    Tabs { children: Vec<LayoutNode> },
    /// A left-to-right split.
    Horizontal {
        children: Vec<LayoutNode>,
        /// Share of the width per child, in the same order. Empty means equal shares.
        #[serde(default)]
        shares: Vec<f32>,
    },
    /// A top-to-bottom split.
    Vertical {
        children: Vec<LayoutNode>,
        #[serde(default)]
        shares: Vec<f32>,
    },
}

impl LayoutNode {
    /// Every panel identifier this arrangement names, in order.
    #[must_use]
    pub fn panels(&self) -> Vec<&str> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            LayoutNode::Panel { pn } => out.push(pn),
            LayoutNode::Tabs { children }
            | LayoutNode::Horizontal { children, .. }
            | LayoutNode::Vertical { children, .. } => {
                for child in children {
                    child.collect(out);
                }
            }
        }
    }

    /// The children of a container, or none for a panel.
    #[must_use]
    pub fn children(&self) -> &[LayoutNode] {
        match self {
            LayoutNode::Panel { .. } => &[],
            LayoutNode::Tabs { children }
            | LayoutNode::Horizontal { children, .. }
            | LayoutNode::Vertical { children, .. } => children,
        }
    }

    /// Shares declared for a split, which must be empty or one per child.
    #[must_use]
    pub fn shares(&self) -> &[f32] {
        match self {
            LayoutNode::Panel { .. } | LayoutNode::Tabs { .. } => &[],
            LayoutNode::Horizontal { shares, .. } | LayoutNode::Vertical { shares, .. } => shares,
        }
    }

    /// This arrangement without the named panels, or `None` if nothing is left.
    ///
    /// Used for the detached panels: they stay in the arrangement so re-attaching knows
    /// where they belong, and are pruned from the main window's tree so they are not
    /// drawn twice. A container that loses all its children disappears with them rather
    /// than remaining as an empty split.
    #[must_use]
    pub fn without(&self, exclude: &[&str]) -> Option<Self> {
        match self {
            LayoutNode::Panel { pn } => (!exclude.contains(&pn.as_str())).then(|| self.clone()),
            LayoutNode::Tabs { children } => {
                let kept = Self::keep(children, exclude)?;
                Some(LayoutNode::Tabs { children: kept.0 })
            }
            LayoutNode::Horizontal { children, shares } => {
                let (kept, keep_shares) = Self::keep_with_shares(children, shares, exclude)?;
                Some(LayoutNode::Horizontal {
                    children: kept,
                    shares: keep_shares,
                })
            }
            LayoutNode::Vertical { children, shares } => {
                let (kept, keep_shares) = Self::keep_with_shares(children, shares, exclude)?;
                Some(LayoutNode::Vertical {
                    children: kept,
                    shares: keep_shares,
                })
            }
        }
    }

    fn keep(children: &[LayoutNode], exclude: &[&str]) -> Option<(Vec<LayoutNode>, Vec<f32>)> {
        let kept: Vec<LayoutNode> = children.iter().filter_map(|c| c.without(exclude)).collect();
        (!kept.is_empty()).then_some((kept, Vec::new()))
    }

    /// Keeps each surviving child's own share, so pruning one panel does not silently
    /// redistribute the space between the others.
    fn keep_with_shares(
        children: &[LayoutNode],
        shares: &[f32],
        exclude: &[&str],
    ) -> Option<(Vec<LayoutNode>, Vec<f32>)> {
        let mut kept = Vec::new();
        let mut kept_shares = Vec::new();
        for (i, child) in children.iter().enumerate() {
            if let Some(node) = child.without(exclude) {
                kept.push(node);
                if let Some(share) = shares.get(i) {
                    kept_shares.push(*share);
                }
            }
        }
        if kept.is_empty() {
            return None;
        }
        if kept_shares.len() != kept.len() {
            kept_shares.clear();
        }
        Some((kept, kept_shares))
    }

    /// A vertical stack of panels, which is the shape a role's default layout takes.
    #[must_use]
    pub fn stack(panels: &[&str]) -> Self {
        LayoutNode::Vertical {
            children: panels
                .iter()
                .map(|pn| LayoutNode::Panel {
                    pn: (*pn).to_owned(),
                })
                .collect(),
            shares: Vec::new(),
        }
    }
}

/// One role's screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoleLayout {
    /// The main window's arrangement.
    pub main: LayoutNode,
    /// Panels shown in their own window, by `PN-xx` identifier.
    ///
    /// D-17 allows only the viewport, the approval queue and the replay timeline to be
    /// detached; `gungnir-config` enforces that, because a detached decision dialog
    /// would separate a decision from the queue it belongs to.
    #[serde(default)]
    pub detached: Vec<String>,
}

/// Per-role arrangements, keyed by the role's canonical name.
///
/// Absent or empty means every role uses the default order
/// `gungnir_workflow::WorkspaceLayout::for_role` produces, which is tested against the
/// plan 06 layout table. An arrangement here overrides the *arrangement*, never the
/// *authorisation*: a panel a role may not open cannot be added by editing this.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiSettings {
    #[serde(default)]
    pub layouts: BTreeMap<String, RoleLayout>,
    /// Draw the viewport with the three-d scene rather than the 2D projection
    /// (GAP-022). **Default on**, by the owner's decision of 2026-09-05.
    ///
    /// The GL draw call has not been rendered anywhere anyone could see it, and there is
    /// no headless probe for one, so this is the setting that decides whether an
    /// unverified renderer or a verified one draws the picture. Two things follow from
    /// it being on, and both are in place: the viewport's status line is drawn by egui
    /// *over* the callback's output, so a GL draw that produces nothing still reports
    /// how many tracks exist; and the operator can switch back to the projection at
    /// runtime without editing a baseline or restarting.
    #[serde(default = "default_scene_3d")]
    pub scene_3d: bool,
    /// Which colour variant the chrome renders (D-35, DS-07; GAP-095): `"day"` or
    /// `"night"`, parsed by [`ThemeVariant::parse`].
    ///
    /// A string here, the same split `AssetConfig::priority` uses (`gungnir-config`):
    /// the schema stays readable and an unrecognised spelling is refused by validation
    /// rather than silently defaulted to day. **Baseline-only, unlike `scene_3d`
    /// above**: D-35 requires that a shift in the picture's colours never be a
    /// mid-session surprise, so there is deliberately no runtime switch and nothing in
    /// the UI may change this while a session is running. Read once at start-up.
    #[serde(default = "default_theme")]
    pub theme: String,
}

/// `true`: see [`UiSettings::scene_3d`].
fn default_scene_3d() -> bool {
    true
}

/// `"day"`: see [`UiSettings::theme`].
fn default_theme() -> String {
    "day".to_owned()
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            layouts: BTreeMap::new(),
            scene_3d: default_scene_3d(),
            theme: default_theme(),
        }
    }
}

/// Which colour variant the chrome renders (D-35, DS-07; GAP-095).
///
/// The night variant is the same hues as day at reduced luminance for low-light or
/// red-light operation, with `ALERT_COLOR` pinned unchanged across both (D-35: a
/// critical alert must read the same regardless of which is in force). Chosen once,
/// from [`UiSettings::theme`], at start-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeVariant {
    #[default]
    Day,
    Night,
}

impl ThemeVariant {
    /// Parses the baseline's spelling. An unknown string is an error rather than a
    /// default: D-35 pins the choice to what the deployment declared, in a
    /// `ConfigBaseline` a person reviews, not to a guess (`gungnir-config` validation
    /// rejects anything else).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "day" => Some(ThemeVariant::Day),
            "night" => Some(ThemeVariant::Night),
            _ => None,
        }
    }
}

impl UiSettings {
    /// The arrangement configured for a role, if any.
    #[must_use]
    pub fn for_role(&self, role: &str) -> Option<&RoleLayout> {
        self.layouts.get(role)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stack_names_its_panels_in_order() {
        let node = LayoutNode::stack(&["PN-06", "PN-05", "PN-03"]);
        assert_eq!(node.panels(), vec!["PN-06", "PN-05", "PN-03"]);
    }

    /// Nesting is what lets a baseline describe a real screen rather than a list, and
    /// the panel order has to survive it: an administrator reading the file should see
    /// the same panels the operator sees.
    #[test]
    fn nested_arrangements_report_every_panel() {
        let node = LayoutNode::Horizontal {
            children: vec![
                LayoutNode::stack(&["PN-06", "PN-05"]),
                LayoutNode::Tabs {
                    children: vec![
                        LayoutNode::Panel { pn: "PN-03".into() },
                        LayoutNode::Panel { pn: "PN-08".into() },
                    ],
                },
            ],
            shares: vec![0.3, 0.7],
        };
        assert_eq!(node.panels(), vec!["PN-06", "PN-05", "PN-03", "PN-08"]);
        assert_eq!(node.shares(), &[0.3, 0.7]);
    }

    /// A detached panel leaves the main tree, and a container that loses everything
    /// disappears rather than becoming an empty split that would draw a gap.
    #[test]
    fn pruning_removes_the_panel_and_any_container_it_emptied() {
        let node = LayoutNode::Horizontal {
            children: vec![
                LayoutNode::stack(&["PN-02"]),
                LayoutNode::stack(&["PN-06", "PN-03"]),
            ],
            shares: vec![0.6, 0.4],
        };
        let pruned = node.without(&["PN-02"]).expect("something is left");
        assert_eq!(pruned.panels(), vec!["PN-06", "PN-03"]);
        assert_eq!(
            pruned.children().len(),
            1,
            "the emptied split should have gone with its child"
        );
        assert_eq!(
            pruned.shares(),
            &[0.4],
            "the surviving child kept its own share rather than being redistributed"
        );

        // Pruning everything leaves nothing, which the caller has to handle rather
        // than being handed an empty container.
        assert!(node.without(&["PN-02", "PN-06", "PN-03"]).is_none());
        // Pruning nothing is the identity.
        assert_eq!(node.without(&[]), Some(node.clone()));
    }

    /// The schema round-trips through JSON, because the baseline is a JSON file that
    /// people edit and review.
    #[test]
    fn the_arrangement_round_trips_through_json() {
        let settings = UiSettings {
            scene_3d: false,
            theme: "night".to_owned(),
            layouts: BTreeMap::from([(
                "Operator".to_owned(),
                RoleLayout {
                    main: LayoutNode::Horizontal {
                        children: vec![
                            LayoutNode::stack(&["PN-06", "PN-05"]),
                            LayoutNode::Panel { pn: "PN-02".into() },
                        ],
                        shares: vec![0.35, 0.65],
                    },
                    detached: vec!["PN-02".to_owned()],
                },
            )]),
        };
        let text = serde_json::to_string(&settings).expect("serialize");
        let back: UiSettings = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(settings, back);
    }

    /// A baseline with no `ui` section is the ordinary case and must mean "use the
    /// defaults", not "no panels".
    ///
    /// Including the three-d default: a `serde` default and a `Default` impl that
    /// disagreed would give a baseline that omits the field a different setting from
    /// one that never had a `ui` section at all.
    #[test]
    fn an_absent_section_configures_nothing() {
        let settings = UiSettings::default();
        assert!(settings.for_role("Operator").is_none());
        let parsed: UiSettings = serde_json::from_str("{}").expect("an empty object parses");
        assert_eq!(parsed, settings);
        assert!(parsed.scene_3d, "the three-d scene is on by default");

        let with_layouts: UiSettings = serde_json::from_str(r#"{"layouts":{}}"#).expect("parses");
        assert!(
            with_layouts.scene_3d,
            "omitting the field gave a different answer from omitting the section"
        );
        let off: UiSettings = serde_json::from_str(r#"{"scene_3d":false}"#).expect("parses");
        assert!(!off.scene_3d, "a deployment can opt out");
        assert_eq!(
            parsed.theme, "day",
            "the theme variant defaults to day (D-35, GAP-095)"
        );
    }

    /// D-35: only "day" and "night" are recognised, case-insensitively and trimmed
    /// like the rest of this file's string-backed settings; anything else is `None`
    /// for `gungnir-config` validation to refuse rather than default.
    #[test]
    fn theme_variant_parses_the_two_recognised_spellings() {
        assert_eq!(ThemeVariant::parse("day"), Some(ThemeVariant::Day));
        assert_eq!(ThemeVariant::parse("night"), Some(ThemeVariant::Night));
        assert_eq!(ThemeVariant::parse(" Night "), Some(ThemeVariant::Night));
        assert_eq!(ThemeVariant::parse("DAY"), Some(ThemeVariant::Day));
        assert_eq!(ThemeVariant::parse("dusk"), None);
        assert_eq!(ThemeVariant::parse(""), None);
    }

    #[test]
    fn theme_variant_defaults_to_day() {
        assert_eq!(ThemeVariant::default(), ThemeVariant::Day);
    }
}

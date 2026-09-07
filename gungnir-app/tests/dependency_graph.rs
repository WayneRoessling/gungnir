// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The dependency graph is acyclic and one-way, checked against the manifests (GAP-081's
//! first check; `ARCHITECTURE.md` §7.1; `docs/agentic-coding-standards.md` §1.1; AP-10).
//!
//! `docs/design/dependency-edges.md` said the acyclicity check ran in a continuous-
//! integration job. It did not: the checks were written for one assessment on 2026-09-04
//! and discarded, and every edge review since rested on prose. This test is the check.
//! It reads every `gungnir-*/Cargo.toml` -- the manifests are the truth for the graph,
//! `CLAUDE.md` -- and refuses a cycle, an upward edge, and a crate the layer table below
//! does not place. **A new crate fails this test until somebody says which layer it is
//! in**, which is the "make the graph machine-readable first" the GAP-081 re-sizing asked
//! for.
//!
//! Normal dependencies only. Dev-dependencies are the verifiers' business and may point
//! anywhere (§1.1: `gungnir-scenario` "feeding test/bench code only").

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Layer {
    /// The tracking core, in the one-way chain §1.1 fixes.
    Core,
    /// Verifiers: depend on whatever they verify, and nothing depends on them.
    Verifier,
    Model,
    /// The service facades. May not depend on productization (AP-10).
    Facade,
    /// 3D data: file I/O and GPU compute, no dependence on anything above the model.
    Data,
    Productization,
    /// Deployment and UI. Nothing below depends on these.
    Deployment,
    Binary,
}

/// `ARCHITECTURE.md` §7.1, transcribed. Every workspace crate must appear here.
const LAYERS: &[(&str, Layer)] = &[
    ("gungnir-core", Layer::Core),
    ("gungnir-coord", Layer::Core),
    ("gungnir-filters", Layer::Core),
    ("gungnir-association", Layer::Core),
    ("gungnir-track", Layer::Core),
    ("gungnir-rfs", Layer::Core),
    ("gungnir-track-fusion", Layer::Core),
    ("gungnir-metrics", Layer::Core),
    ("gungnir-fusion-async", Layer::Core),
    ("gungnir-allocation", Layer::Core),
    ("gungnir-scenario", Layer::Core),
    ("gungnir-testkit", Layer::Verifier),
    ("gungnir-oracle", Layer::Verifier),
    ("gungnir-fuzz", Layer::Verifier),
    ("gungnir-model", Layer::Model),
    ("gungnir-tracking-service", Layer::Facade),
    ("gungnir-intercept-service", Layer::Facade),
    ("gungnir-data", Layer::Data),
    ("gungnir-data-fusion", Layer::Data),
    ("gungnir-eventing", Layer::Productization),
    ("gungnir-store", Layer::Productization),
    ("gungnir-config", Layer::Productization),
    ("gungnir-mission", Layer::Productization),
    ("gungnir-time", Layer::Productization),
    ("gungnir-ingest", Layer::Productization),
    ("gungnir-sensor-management", Layer::Productization),
    ("gungnir-interop", Layer::Productization),
    ("gungnir-identity", Layer::Productization),
    ("gungnir-identification", Layer::Productization),
    ("gungnir-ml", Layer::Productization),
    ("gungnir-geo", Layer::Productization),
    ("gungnir-analytics", Layer::Productization),
    ("gungnir-policy", Layer::Productization),
    ("gungnir-command", Layer::Productization),
    ("gungnir-assessment", Layer::Productization),
    ("gungnir-decision", Layer::Productization),
    ("gungnir-modelops", Layer::Productization),
    ("gungnir-security", Layer::Productization),
    ("gungnir-api", Layer::Productization),
    ("gungnir-observability", Layer::Productization),
    ("gungnir-resilience", Layer::Productization),
    ("gungnir-collab", Layer::Productization),
    ("gungnir-workflow", Layer::Productization),
    ("gungnir-replay", Layer::Productization),
    ("gungnir-reporting", Layer::Productization),
    ("gungnir-remote", Layer::Deployment),
    ("gungnir-ui", Layer::Deployment),
    ("gungnir-viewport3d", Layer::Deployment),
    ("gungnir-render", Layer::Deployment),
    ("gungnir-node", Layer::Binary),
    ("gungnir-app", Layer::Binary),
];

/// §1.1's one-way chain inside the core. A core crate may depend only on core crates
/// earlier in this order.
const CORE_ORDER: &[&str] = &[
    "gungnir-core",
    "gungnir-coord",
    "gungnir-filters",
    "gungnir-association",
    "gungnir-track",
    "gungnir-rfs",
    "gungnir-track-fusion",
    "gungnir-metrics",
    "gungnir-fusion-async",
    "gungnir-allocation",
    "gungnir-scenario",
];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits in the workspace")
        .to_path_buf()
}

/// Normal `gungnir-*` dependencies of one manifest: `[dependencies]`,
/// `[target.<cfg>.dependencies]`, and `[dependencies.gungnir-x]` tables. Not
/// `[dev-dependencies]` or `[build-dependencies]`.
fn normal_deps(manifest: &str) -> BTreeSet<String> {
    let mut deps = BTreeSet::new();
    let mut in_normal = false;
    for raw in manifest.lines() {
        let line = raw.trim();
        if let Some(section) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            let section = section.trim();
            if let Some(name) = section.strip_prefix("dependencies.") {
                if name.starts_with("gungnir-") {
                    deps.insert(name.trim_matches('"').to_string());
                }
                in_normal = false;
                continue;
            }
            in_normal = section == "dependencies"
                || (section.ends_with(".dependencies") && !section.contains("dev-"));
            continue;
        }
        if !in_normal || !line.starts_with("gungnir-") {
            continue;
        }
        let name: String = line
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        deps.insert(name);
    }
    deps
}

fn graph() -> BTreeMap<String, BTreeSet<String>> {
    let root = workspace();
    let mut graph = BTreeMap::new();
    for entry in std::fs::read_dir(&root)
        .expect("workspace listing")
        .flatten()
    {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with("gungnir-") || !path.is_dir() {
            continue;
        }
        let manifest = path.join("Cargo.toml");
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            continue;
        };
        graph.insert(name.to_string(), normal_deps(&text));
    }
    assert!(
        graph.len() > 40,
        "only {} crates found under {}",
        graph.len(),
        root.display()
    );
    graph
}

fn layer(name: &str) -> Layer {
    LAYERS.iter().find(|(n, _)| *n == name).map_or_else(
        || {
            panic!(
                "{name} is not placed in ARCHITECTURE.md §7.1's layer table in this test; \
                     a new crate is placed before it is depended on"
            )
        },
        |(_, l)| *l,
    )
}

/// Every crate the manifests name is placed, and every placed crate exists.
#[test]
fn every_crate_is_placed_in_a_layer() {
    let graph = graph();
    for name in graph.keys() {
        let _ = layer(name);
    }
    for (name, _) in LAYERS {
        assert!(
            graph.contains_key(*name),
            "{name} is placed but has no manifest"
        );
    }
    for (from, deps) in &graph {
        for to in deps {
            assert!(
                graph.contains_key(to),
                "{from} depends on {to}, which is not a workspace crate directory"
            );
        }
    }
}

/// **No cycle**, by three-colour depth-first search over the normal edges.
#[test]
fn the_dependency_graph_is_acyclic() {
    #[derive(Clone, Copy, PartialEq)]
    enum Colour {
        White,
        Grey,
        Black,
    }
    fn visit(
        node: &str,
        graph: &BTreeMap<String, BTreeSet<String>>,
        colour: &mut BTreeMap<String, Colour>,
        stack: &mut Vec<String>,
    ) {
        match colour.get(node).copied().unwrap_or(Colour::White) {
            Colour::Black => return,
            Colour::Grey => {
                stack.push(node.to_string());
                panic!("dependency cycle: {}", stack.join(" -> "));
            }
            Colour::White => {}
        }
        colour.insert(node.to_string(), Colour::Grey);
        stack.push(node.to_string());
        if let Some(deps) = graph.get(node) {
            for dep in deps {
                visit(dep, graph, colour, stack);
            }
        }
        stack.pop();
        colour.insert(node.to_string(), Colour::Black);
    }
    let graph = graph();
    let mut colour = BTreeMap::new();
    for node in graph.keys() {
        visit(node, &graph, &mut colour, &mut Vec::new());
    }
}

/// **One-way, by layer** (§1.1, AP-10): nothing below depends on anything above.
///
/// - the core chain is ordered, and a core crate depends only on earlier core crates;
/// - the model depends on the core only;
/// - a facade depends on the core and the model, never on productization (AP-10 -- the
///   edge DN-06 refused);
/// - 3D data depends on nothing above the model;
/// - productization depends on nothing in deployment or the binaries;
/// - nothing depends on a verifier, and only a verifier depends on `gungnir-scenario`;
/// - nothing depends on a binary.
#[test]
fn every_edge_points_downward() {
    let graph = graph();
    let mut violations = Vec::new();
    for (from, deps) in &graph {
        let from_layer = layer(from);
        for to in deps {
            let to_layer = layer(to);
            let bad = match from_layer {
                Layer::Core => {
                    let earlier = CORE_ORDER.iter().position(|c| c == from);
                    let dep = CORE_ORDER.iter().position(|c| c == to);
                    match (earlier, dep) {
                        (Some(f), Some(t)) => t >= f,
                        _ => true,
                    }
                }
                Layer::Verifier => !matches!(
                    to_layer,
                    Layer::Core | Layer::Model | Layer::Productization | Layer::Facade
                ),
                Layer::Model => to_layer != Layer::Core,
                Layer::Facade => !matches!(to_layer, Layer::Core | Layer::Model),
                Layer::Data => !matches!(to_layer, Layer::Data | Layer::Core | Layer::Model),
                Layer::Productization => !matches!(
                    to_layer,
                    Layer::Core
                        | Layer::Model
                        | Layer::Facade
                        | Layer::Productization
                        | Layer::Data
                ),
                Layer::Deployment => matches!(to_layer, Layer::Binary | Layer::Verifier),
                Layer::Binary => matches!(to_layer, Layer::Binary | Layer::Verifier),
            };
            let scenario_misuse = to == "gungnir-scenario" && from_layer != Layer::Verifier;
            if bad || scenario_misuse {
                violations.push(format!("{from} ({from_layer:?}) -> {to} ({to_layer:?})"));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "upward or forbidden edges:\n  {}",
        violations.join("\n  ")
    );
}

/// The edges §7.1 records as added on 2026-09-05 and 2026-09-06 are the edges the
/// manifests have: the review record and the truth agree.
#[test]
fn the_recorded_edges_are_in_the_manifests() {
    let graph = graph();
    let has = |from: &str, to: &str| graph.get(from).is_some_and(|d| d.contains(to));
    for (from, to, label) in [
        ("gungnir-app", "gungnir-modelops", "(h)"),
        ("gungnir-node", "gungnir-modelops", "(h)"),
        ("gungnir-modelops", "gungnir-model", "(h)"),
        ("gungnir-app", "gungnir-decision", "(i)"),
        ("gungnir-ingest", "gungnir-interop", "(j)"),
        ("gungnir-node", "gungnir-policy", "(k)"),
        ("gungnir-node", "gungnir-geo", "(l)"),
        ("gungnir-app", "gungnir-resilience", "(m)"),
        ("gungnir-app", "gungnir-identification", "(n)"),
        ("gungnir-app", "gungnir-identity", "(o)"),
        ("gungnir-ml", "gungnir-interop", "(q)"),
        ("gungnir-ml", "gungnir-model", "(q)"),
        ("gungnir-node", "gungnir-analytics", "(g)"),
        ("gungnir-node", "gungnir-identity", "(s)"),
        ("gungnir-analytics", "gungnir-sensor-management", "(c)"),
    ] {
        assert!(
            has(from, to),
            "{label} {from} -> {to} is recorded in §7.1 and not in the manifest"
        );
    }
}

// ---------------------------------------------------------------------------------
// The manifests against `ARCHITECTURE.md` itself (edge (t), `dependency-edges.md` §14)
// ---------------------------------------------------------------------------------
//
// Every test above this line reads the manifests and checks a *property* of the graph
// they describe -- placed, acyclic, one-way by layer. None of them checks the graph
// against the document that is supposed to describe it, so an edge could be added to a
// `Cargo.toml`, point in a legal direction, and never appear in `ARCHITECTURE.md`.
//
// That is not hypothetical. `gungnir-remote` to `gungnir-security` did exactly that: a
// real `[dependencies]` edge from 2026-09-06 that §7's table did not list, found on
// 2026-09-07 only because the UAF generator derives its own views from the manifests and
// its committed output disagreed. `docs/design/dependency-edges.md` §14 records the
// finding and says the next one would be found the same accidental way, or not at all.
// This closes it.

/// The arrow `ARCHITECTURE.md` §7.1 draws an edge with.
const ARROW: &str = "──►";

/// Drop the box-drawing characters §7.1's tree is drawn with.
fn strip_box_drawing(line: &str) -> String {
    line.chars()
        .filter(|c| {
            !matches!(
                c,
                '\u{2502}'
                    | '\u{251c}'
                    | '\u{2514}'
                    | '\u{2500}'
                    | '\u{25ba}'
                    | '\u{252c}'
                    | '\u{2510}'
                    | '\u{2518}'
                    | '\u{250c}'
                    | '\u{2524}'
            )
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Remove every balanced bracketed group, and everything from an unbalanced one onwards.
///
/// The documents put four things in brackets and none of them is a dependency: an edge
/// letter, a dev-only edge (`dev: policy`), prose (`theme only`,
/// `no workspace dependencies`), and a paragraph that runs past the end of the line
/// (`gungnir-model`'s `Foundational; ..`). Some sit *inside* the list -- §7.1's workflow
/// row reads `sensor-management (b), assessment (e)` -- so this cannot simply cut at the
/// first bracket, and the unbalanced case is why it cannot simply drop balanced pairs.
fn strip_parentheticals(text: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in text.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// Every backtick-quoted token in a line, in order.
fn backticked(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('`') else { break };
        out.push(after[..close].to_string());
        rest = &after[close + 1..];
    }
    out
}

/// `core` and `gungnir-core` are the same crate; the documents use the short form.
fn full_crate_name(short: &str) -> Option<String> {
    let s = short.trim().trim_matches('`').trim();
    if s.is_empty() {
        return None;
    }
    Some(if s.starts_with("gungnir-") {
        s.to_string()
    } else {
        format!("gungnir-{s}")
    })
}

/// §7's table, one row per crate or group of crates.
fn documented_by_section_7(md: &str) -> BTreeMap<String, BTreeSet<String>> {
    let mut out = BTreeMap::new();
    for raw in md.lines() {
        let line = raw.trim();
        if !line.starts_with("| `gungnir-") {
            continue;
        }
        let cols: Vec<&str> = line.trim_matches('|').split('|').collect();
        if cols.len() != 2 {
            continue;
        }
        let crates: Vec<String> = backticked(cols[0])
            .into_iter()
            .filter(|c| c.starts_with("gungnir-"))
            .collect();
        if crates.is_empty() {
            continue;
        }
        let mut deps: BTreeSet<String> = BTreeSet::new();
        // Same rule as the graph below: a bracketed group is an edge letter, a note, or
        // a dev-only edge, never a production dependency. Dropping them here is what
        // keeps `scenario (dev: testkit)` from reading as a dependency on testkit.
        let listed = strip_parentheticals(cols[1]);
        if !cols[1].contains("No workspace crates") {
            // "Both facades" is prose for the two service facades, and is the only
            // dependency in either document not written as a crate name.
            if cols[1].contains("Both facades") {
                deps.insert("gungnir-tracking-service".to_string());
                deps.insert("gungnir-intercept-service".to_string());
            }
            deps.extend(
                backticked(&listed)
                    .iter()
                    .filter_map(|d| full_crate_name(d)),
            );
        }
        for c in crates {
            out.insert(c, deps.clone());
        }
    }
    out
}

/// §7.1's graph. A dependency list wrapped onto the next line ends with a comma, and the
/// continuation carries no arrow of its own.
fn documented_by_section_7_1(md: &str) -> BTreeMap<String, BTreeSet<String>> {
    let block: Vec<&str> = md
        .lines()
        .skip_while(|l| !l.starts_with("### §7.1"))
        .skip_while(|l| !l.trim_start().starts_with("```"))
        .skip(1)
        .take_while(|l| !l.trim_start().starts_with("```"))
        .collect();
    assert!(
        !block.is_empty(),
        "§7.1's fenced graph was not found in ARCHITECTURE.md; this parser is reading the \
         wrong thing and would otherwise pass by checking nothing"
    );

    let mut out = BTreeMap::new();
    let mut i = 0;
    while i < block.len() {
        // Split on the arrow *before* stripping the tree, because the arrow is itself
        // drawn from the box characters this strips.
        let Some((name, rest)) = block[i].split_once(ARROW) else {
            i += 1;
            continue;
        };
        let Some(crate_name) = full_crate_name(&strip_box_drawing(name)) else {
            i += 1;
            continue;
        };
        let mut text = strip_parentheticals(rest).trim().to_string();
        while text.ends_with(',') && i + 1 < block.len() {
            if block[i + 1].contains(ARROW) {
                break;
            }
            let next = strip_box_drawing(block[i + 1]);
            let cont = strip_parentheticals(&next).trim().to_string();
            if cont.is_empty() {
                break;
            }
            text.push(' ');
            text.push_str(&cont);
            i += 1;
        }
        let deps: BTreeSet<String> = text
            .split(',')
            .filter_map(full_crate_name)
            .filter(|d| d != "gungnir-")
            .collect();
        out.insert(crate_name, deps);
        i += 1;
    }
    out
}

/// **The manifests and `ARCHITECTURE.md` describe the same graph.**
///
/// Compared in both directions, because the two failures are different mistakes. An edge
/// in a manifest that the document does not list is an undeclared dependency -- the rule
/// `CLAUDE.md` states outright, and the one that went unnoticed. An edge the document
/// lists that no manifest carries is a stale document, which is how a reader comes to
/// believe in a dependency that was removed.
///
/// Only production `[dependencies]` are compared. Both documents mark dev-only edges
/// separately, and [`normal_deps`] already excludes `[dev-dependencies]`, so the two
/// agree about what is being described.
#[test]
fn the_manifests_and_architecture_md_agree_on_every_edge() {
    let md = std::fs::read_to_string(workspace().join("ARCHITECTURE.md"))
        .expect("ARCHITECTURE.md is readable at the workspace root");

    let mut documented = documented_by_section_7(&md);
    for (name, deps) in documented_by_section_7_1(&md) {
        if let Some(existing) = documented.get(&name) {
            assert_eq!(
                *existing, deps,
                "{name} is described by both §7's table and §7.1's graph and they \
                 disagree; one of them is wrong"
            );
        }
        documented.insert(name, deps);
    }
    assert!(
        documented.len() > 40,
        "only {} crates parsed out of ARCHITECTURE.md; the parser is broken and would \
         otherwise pass by checking almost nothing",
        documented.len()
    );

    let actual = graph();
    let mut problems: Vec<String> = Vec::new();

    for (name, real) in &actual {
        let Some(doc) = documented.get(name) else {
            problems.push(format!(
                "{name}: in the workspace but in neither §7's table nor §7.1's graph, so \
                 nothing documents what it may depend on"
            ));
            continue;
        };
        for dep in real.difference(doc) {
            problems.push(format!(
                "{name} -> {dep}: in {name}/Cargo.toml but not in ARCHITECTURE.md. Either \
                 the edge is wrong and comes out, or the table gains it and \
                 docs/design/dependency-edges.md gains its entry (§5)"
            ));
        }
        for dep in doc.difference(real) {
            problems.push(format!(
                "{name} -> {dep}: in ARCHITECTURE.md but not in {name}/Cargo.toml. The \
                 manifest is the truth for the graph, so the document is stale"
            ));
        }
    }

    assert!(
        problems.is_empty(),
        "the manifests and ARCHITECTURE.md disagree about {} edge(s):\n  {}",
        problems.len(),
        problems.join("\n  ")
    );
}

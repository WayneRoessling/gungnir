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

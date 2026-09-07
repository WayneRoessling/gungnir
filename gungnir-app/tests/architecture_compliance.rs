//! The four remaining mechanical architecture checks (GAP-081; plan 10 finding CA-F1;
//! `docs/architecture/togaf/phase-g-implementation-governance/architecture-contracts.md`
//! C-07, C-14, AP-12/C-03 part one, and C-05's document integrity). The fifth, dependency
//! direction (C-11), is `dependency_graph.rs`.
//!
//! Each was checked on 2026-09-04 by a script written for that run and discarded, so a
//! principle could erode between assessments. These read the tree on every `cargo test`.
//!
//! - **One owning crate per shared type** (§1.2 of the standards, AP-06): each shared
//!   primitive is defined exactly once, in the crate that owns it.
//! - **The unwrap policy** (CONTRIBUTING, "No `unwrap()`/`expect()`"): none outside test
//!   modules, `fn main`, benches, tests, fuzz targets and the verifier crates.
//! - **Citations resolve**: every `<document>.md §N` in a source comment names a file
//!   that exists once under the workspace and a heading in it numbered `N`.
//! - **The recorded stack** (§2.9, AP-14): every crate in `[workspace.dependencies]` is
//!   named in the standards documents.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits in the workspace")
        .to_path_buf()
}

/// Every `.rs` file under `gungnir-*/`, with its workspace-relative path.
fn rust_sources() -> Vec<(String, String)> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n == "target") {
                    continue;
                }
                walk(&path, root, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    let rel = path
                        .strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    out.push((rel, text));
                }
            }
        }
    }
    let root = workspace();
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&root)
        .expect("workspace listing")
        .flatten()
    {
        let path = entry.path();
        if path.is_dir()
            && path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("gungnir-"))
        {
            walk(&path, &root, &mut out);
        }
    }
    assert!(out.len() > 100, "only {} sources found", out.len());
    out
}

/// The text of a source with `#[cfg(test)]`-gated modules removed (brace-matched).
fn without_test_modules(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("#[cfg(test)]") {
        out.push_str(&rest[..i]);
        let after = &rest[i..];
        // The gated item starts at the next `{` (a `mod tests {`) and ends at its match.
        let Some(open) = after.find('{') else {
            out.push_str(after);
            rest = "";
            break;
        };
        let mut depth = 0usize;
        let mut end = None;
        for (j, c) in after[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(open + j + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        match end {
            Some(e) => rest = &after[e..],
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// True for a path the unwrap policy exempts: tests, benches, examples, fuzz targets,
/// and the verifier crates (which depend on what they verify and nothing depends on).
fn exempt_from_unwrap_policy(rel: &str) -> bool {
    rel.contains("/tests/")
        || rel.contains("/benches/")
        || rel.contains("/examples/")
        || rel.contains("/fuzz_targets/")
        || rel.starts_with("gungnir-testkit/")
        || rel.starts_with("gungnir-oracle/")
        || rel.starts_with("gungnir-fuzz/")
}

/// **One owning crate per shared type** (C-07). The primitives every layer names,
/// each defined once; a second `pub struct TrackId` anywhere is the failure.
#[test]
fn every_shared_type_has_exactly_one_definition() {
    const SHARED: &[&str] = &[
        "TrackId",
        "SensorId",
        "ResourceId",
        "PlanId",
        "DecisionId",
        "SessionId",
        "MissionTime",
        "Geodetic",
        "DetectionView",
        "TrackView",
        "PlanView",
        "ResourceView",
        "Classification",
        "Releasability",
        "EffectorLayer",
        "AlgorithmBaselineId",
        "GlobalEntityId",
        "SystemHealth",
    ];
    let sources = rust_sources();
    let mut definitions: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (rel, text) in &sources {
        if rel.contains("/tests/") || rel.contains("/benches/") {
            continue;
        }
        let body = without_test_modules(text);
        for name in SHARED {
            for kind in ["struct", "enum", "type"] {
                let needle = format!("pub {kind} {name}");
                for (i, line) in body.lines().enumerate() {
                    let trimmed = line.trim_start();
                    if trimmed.starts_with(&needle)
                        && trimmed[needle.len()..]
                            .chars()
                            .next()
                            .is_none_or(|c| !c.is_alphanumeric() && c != '_')
                    {
                        definitions
                            .entry(name)
                            .or_default()
                            .push(format!("{rel}:{}", i + 1));
                    }
                }
            }
        }
    }
    let mut problems = Vec::new();
    for name in SHARED {
        match definitions.get(name).map(Vec::as_slice) {
            Some([_one]) => {}
            Some(many) => problems.push(format!(
                "{name} is defined {} times: {}",
                many.len(),
                many.join(", ")
            )),
            None => problems.push(format!(
                "{name} is defined nowhere (renamed? update the list)"
            )),
        }
    }
    assert!(
        problems.is_empty(),
        "shared types:\n  {}",
        problems.join("\n  ")
    );
}

/// **The unwrap policy** (AP-12, C-03 part one): no `unwrap()` or `expect(` outside
/// test modules, `fn main`, and the exempt paths. A debug-only invariant check is
/// written as `debug_assert!`, not as an unwrap.
#[test]
fn no_unwrap_or_expect_outside_tests_and_main() {
    let mut hits = Vec::new();
    for (rel, text) in rust_sources() {
        if exempt_from_unwrap_policy(&rel) {
            continue;
        }
        let body = without_test_modules(&text);
        let mut in_main = false;
        let mut main_depth = 0usize;
        for (i, line) in body.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("fn main(") {
                in_main = true;
                main_depth = 0;
            }
            if in_main {
                main_depth += line.matches('{').count();
                main_depth = main_depth.saturating_sub(line.matches('}').count());
                if main_depth == 0 && line.contains('}') {
                    in_main = false;
                }
                continue;
            }
            if trimmed.starts_with("//") {
                continue;
            }
            // Strip string literals crudely so a message mentioning `unwrap()` does
            // not count.
            let code: String = {
                let mut s = String::new();
                let mut in_str = false;
                let mut prev = '\0';
                for c in line.chars() {
                    if c == '"' && prev != '\\' {
                        in_str = !in_str;
                    } else if !in_str {
                        s.push(c);
                    }
                    prev = c;
                }
                s
            };
            if code.contains(".unwrap()") || code.contains(".expect(") {
                hits.push(format!("{rel}:{}: {}", i + 1, line.trim()));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "unwrap()/expect() outside tests and main:\n  {}",
        hits.join("\n  ")
    );
}

/// Every Markdown file under the workspace root and `docs/`, by basename, with the
/// paths that carry it.
fn markdown_index(root: &Path) -> BTreeMap<String, Vec<PathBuf>> {
    fn walk(dir: &Path, out: &mut BTreeMap<String, Vec<PathBuf>>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name == "target" || name.starts_with('.') || name.starts_with("gungnir-") {
                    continue;
                }
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "md") {
                let base = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                out.entry(base).or_default().push(path);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, &mut out);
    out
}

/// True when `text` has a heading numbered `section` (`## §7.1`, `### 1.1 …`,
/// `## 7. …`, `## 9. Amendment …`).
fn has_section(text: &str, section: &str) -> bool {
    text.lines().any(|line| {
        let Some(rest) = line.strip_prefix('#') else {
            return false;
        };
        let title = rest.trim_start_matches('#').trim_start();
        let title = title.trim_start_matches('§').trim_start();
        title.starts_with(section)
            && title[section.len()..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_ascii_digit())
    })
}

/// **Citations resolve** (C-05, document integrity): every `<file>.md §N` in a source
/// comment names exactly one document under the workspace and a heading in it.
#[test]
fn every_document_citation_resolves() {
    let root = workspace();
    let index = markdown_index(&root);
    let mut problems = BTreeSet::new();
    let mut count = 0usize;
    for (rel, text) in rust_sources() {
        for (i, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if !(trimmed.starts_with("//")
                || trimmed.starts_with("///")
                || trimmed.starts_with("//!"))
            {
                continue;
            }
            let mut rest = line;
            while let Some(pos) = rest.find(".md") {
                // The document name runs back to the last non-name character.
                let before = &rest[..pos];
                let start = before
                    .rfind(|c: char| !(c.is_ascii_alphanumeric() || "-_./".contains(c)))
                    .map_or(0, |p| p + 1);
                let doc = &before[start..];
                let after = &rest[pos + 3..];
                let after_trim = after.trim_start_matches('`').trim_start();
                if let Some(sec) = after_trim.strip_prefix('§') {
                    let section: String = sec
                        .trim_start()
                        .chars()
                        .take_while(|c| c.is_ascii_digit() || *c == '.')
                        .collect();
                    let section = section.trim_end_matches('.').to_string();
                    if !doc.is_empty() && !section.is_empty() {
                        count += 1;
                        let base = doc.rsplit('/').next().unwrap_or(doc);
                        let file = format!("{base}.md");
                        match index.get(&file).map(Vec::as_slice) {
                            None => {
                                problems
                                    .insert(format!("{rel}:{}: {doc}.md does not exist", i + 1));
                            }
                            Some(paths) => {
                                let chosen = if paths.len() == 1 {
                                    Some(&paths[0])
                                } else {
                                    paths.iter().find(|p| {
                                        p.to_string_lossy()
                                            .replace('\\', "/")
                                            .ends_with(&format!("{doc}.md"))
                                    })
                                };
                                match chosen {
                                    None => problems.insert(format!(
                                        "{rel}:{}: {doc}.md is ambiguous ({} files named {file})",
                                        i + 1,
                                        paths.len()
                                    )),
                                    Some(p) => {
                                        let body = std::fs::read_to_string(p).unwrap_or_default();
                                        if has_section(&body, &section) {
                                            false
                                        } else {
                                            problems.insert(format!(
                                                "{rel}:{}: {doc}.md has no section {section}",
                                                i + 1
                                            ))
                                        }
                                    }
                                };
                            }
                        }
                    }
                }
                rest = after;
            }
        }
    }
    assert!(
        count > 100,
        "only {count} citations found; the scanner is broken"
    );
    assert!(
        problems.is_empty(),
        "{} unresolved citations of {count}:\n  {}",
        problems.len(),
        problems.iter().cloned().collect::<Vec<_>>().join("\n  ")
    );
}

/// **The recorded stack** (C-14, AP-14): every crate in `[workspace.dependencies]` is
/// named in `docs/agentic-coding-standards.md` §2 or in the UI standards document.
#[test]
fn every_workspace_dependency_is_recorded() {
    let root = workspace();
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).expect("root manifest");
    let mut in_deps = false;
    let mut deps = Vec::new();
    for raw in manifest.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            in_deps = line == "[workspace.dependencies]";
            continue;
        }
        if in_deps && !line.is_empty() && !line.starts_with('#') {
            let name: String = line
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            if !name.is_empty() {
                deps.push(name);
            }
        }
    }
    assert!(
        deps.len() > 20,
        "only {} workspace dependencies parsed",
        deps.len()
    );
    let recorded = [
        "docs/agentic-coding-standards.md",
        "docs/rust-ui-architecture-coding-standards.md",
    ]
    .iter()
    .map(|p| std::fs::read_to_string(root.join(p)).expect("standards document"))
    .collect::<Vec<_>>()
    .join("\n");
    let mentioned: BTreeSet<String> = recorded
        .split('`')
        .skip(1)
        .step_by(2)
        .map(|s| s.trim().to_string())
        .collect();
    let names = |dep: &str| -> Vec<String> {
        let mut v = vec![
            dep.to_string(),
            dep.replace('_', "-"),
            dep.replace('-', "_"),
        ];
        if dep == "arrow" {
            v.push("arrow-rs".into());
        }
        v
    };
    let missing: Vec<&String> = deps
        .iter()
        .filter(|d| {
            !names(d).iter().any(|n| {
                mentioned.contains(n)
                    || mentioned.iter().any(|m| {
                        m.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
                            .any(|tok| tok == n)
                    })
            })
        })
        .collect();
    assert!(
        missing.is_empty(),
        "workspace dependencies not recorded in the standards documents: {missing:?}"
    );
}

/// The names a method would plausibly be given if it handed a private key to a caller.
///
/// Deliberately generous: this is a tripwire, and a false positive costs somebody a
/// sentence of explanation while a false negative costs the invariant.
const PRIVATE_KEY_EXPORT_NAMES: &[&str] = &[
    "private_key",
    "secret_key",
    "signing_key_bytes",
    "key_bytes",
    "to_pkcs8",
    "to_sec1_der",
    "export_key",
    "private_der",
    "private_pem",
];

/// `docs/agentic-coding-standards.md` §2.9 point 2: **no code path may build a
/// certificate over private key material that has left a `KeyProvider`.**
///
/// # Why this test exists even though the rule holds by construction
///
/// It holds today because `gungnir-security` has no way to hand a private key to a
/// caller at all: `KeyProvider` exposes `active`, `state`, `seal`, `unseal`, `rotate` and
/// `sign`, the concrete providers add `generate`, `public_key_der` and `public_key_sec1`,
/// and none of them returns private material. `rcgen` and `rustls` are given a
/// `SigningKey` that calls back into the provider, so the bytes never move.
///
/// **An invariant that holds because nobody has yet added a convenience getter is one
/// afternoon from not holding.** The day somebody adds a private-key exporter -- for a
/// backup feature, or to move a key between machines -- the rule dies and every
/// certificate path stays green, because nothing downstream would change. This test makes
/// that addition fail here, so it becomes a decision rather than a commit.
///
/// It checks the surface rather than the callers on purpose. Scanning for uses of `rcgen`
/// would find where certificates are made and say nothing about where keys come from,
/// which is the half that matters.
#[test]
fn no_path_exports_private_key_material() {
    let mut offenders = Vec::new();
    for (rel, text) in rust_sources() {
        if !rel.starts_with("gungnir-security/src/") {
            continue;
        }
        let body = without_test_modules(&text);
        for (line, source) in body.lines().enumerate() {
            let trimmed = source.trim_start();
            if !trimmed.starts_with("pub fn") && !trimmed.starts_with("fn ") {
                continue;
            }
            // Only a function that hands something back can hand back a key.
            if !source.contains("->") {
                continue;
            }
            for name in PRIVATE_KEY_EXPORT_NAMES {
                if trimmed.contains(name) {
                    offenders.push(format!("{rel}:{}: {}", line + 1, trimmed.trim_end()));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "a `gungnir-security` function appears to return private key material, which \
         would break docs/agentic-coding-standards.md §2.9 point 2 -- no code path may \
         build a certificate over private key material that has left a `KeyProvider`. \
         The whole certificate path (`gungnir-remote::identity`) depends on there being \
         no way to obtain the private half. If this is deliberate, it is an owner \
         decision and an amendment to that rule, not a test to relax:\n{}",
        offenders.join("\n")
    );
}

/// The other half of the same rule: the provider trait's own surface.
///
/// A private-key exporter added as a **trait method** would be named something this
/// tripwire does not guess, so the trait's method set is pinned by name. Adding a method
/// fails here and forces the question "does this hand out a key?" to be asked out loud.
#[test]
fn the_key_provider_surface_is_the_one_the_certificate_path_relies_on() {
    let (_, keys) = rust_sources()
        .into_iter()
        .find(|(rel, _)| rel == "gungnir-security/src/keys.rs")
        .expect("gungnir-security/src/keys.rs");
    let trait_body = keys
        .split_once("pub trait KeyProvider")
        .map(|(_, rest)| rest)
        .expect("the KeyProvider trait");
    let end = trait_body.find("\n}").unwrap_or(trait_body.len());
    let methods: std::collections::BTreeSet<String> = trait_body[..end]
        .lines()
        .filter_map(|l| {
            let l = l.trim_start();
            l.strip_prefix("fn ")
                .and_then(|r| r.split(['(', '<']).next())
                .map(str::to_owned)
        })
        .collect();
    let expected: std::collections::BTreeSet<String> =
        ["active", "rotate", "seal", "sign", "state", "unseal"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
    assert_eq!(
        methods, expected,
        "`KeyProvider`'s method set changed. None of the six it had returns private key \
         material, and the certificate path in `gungnir-remote::identity` is safe only \
         because of that. If a method was added, say here whether it hands a caller a \
         private key; if it does, docs/agentic-coding-standards.md §2.9 point 2 has to \
         change first, and that is the owner's decision"
    );
}

//! No `todo!()` survives anywhere in the workspace (GAP-082, plan 10 finding CA-F2).
//!
//! `agentic-coding-standards.md` §3.1 reserves `todo!()` for functions nothing can call
//! and requires a named error variant on any reachable path. Eighteen `todo!()` bodies
//! existed and **every one of them was the body of a public function**, so every one was
//! reachable by any caller: a public function is not a function nothing can call, however
//! few callers it has today. The claim that none was reachable rested on inspection.
//!
//! They are now named errors. This test is what replaces the inspection.
//!
//! # Why the rule is absolute rather than "reachable ones only"
//!
//! **A `todo!()` panics in front of an operator**, which is the loudest possible breach of
//! the honest-status rule this system is built on — and reachability is not a stable
//! property. A private helper nothing calls today is one refactor away from being called,
//! and the refactor that calls it will not think to check. Scanning for the macro is a
//! check that cannot go stale; arguing about call graphs is a check that already did.

use std::path::{Path, PathBuf};

/// Every `.rs` file in the workspace, excluding build output and this file.
fn sources(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            // `target/` is build output and `.git` is not source. `gungnir-fuzz` is
            // excluded from the workspace and carries its own target directory.
            if name == "target" || name.starts_with('.') {
                continue;
            }
            out.extend(sources(&path));
        } else if name.ends_with(".rs") && name != "no_reachable_todo.rs" {
            out.push(path);
        }
    }
    out
}

/// True when the line has a `todo!` outside a comment.
///
/// Deliberately crude: it strips a trailing line comment and looks at what is left. A doc
/// comment *about* `todo!()` — of which this file and two others have several — is prose,
/// and prose is not what panics.
fn has_todo_in_code(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") {
        return false;
    }
    let code = match trimmed.find("//") {
        Some(i) => &trimmed[..i],
        None => trimmed,
    };
    code.contains("todo!(")
}

/// **The check that replaces the inspection.**
#[test]
fn no_todo_macro_survives_in_any_crate() {
    // The test runs with the crate as its working directory.
    let root = Path::new("..");
    let files = sources(root);
    assert!(
        files.len() > 100,
        "only {} source files were found; the walk is not reaching the workspace",
        files.len()
    );

    let mut offenders = Vec::new();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            if has_todo_in_code(line) {
                offenders.push(format!("{}:{}", file.display(), n + 1));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "todo!() panics in front of an operator; use a named error variant instead. \
         Found at: {offenders:?}"
    );
}

/// The scan has to be able to fail, or it is decoration. Checked against text rather than
/// a real file so the workspace never contains the thing being detected.
#[test]
fn the_scan_detects_a_todo_and_ignores_prose_about_one() {
    assert!(has_todo_in_code("    todo!()"));
    assert!(has_todo_in_code("    let x = todo!();"));
    assert!(!has_todo_in_code("    foo(); // no todo here"));
    // The four this scan caught that a `todo!()` grep did not: they take a message.
    assert!(has_todo_in_code(
        r#"    todo!("RTS smoother, gated against filterpy")"#
    ));

    assert!(!has_todo_in_code(
        "/// Named rather than a todo!() (GAP-082)."
    ));
    assert!(!has_todo_in_code("//! every one of these was a todo!()"));
    assert!(!has_todo_in_code("    // convert this todo!() one day"));
}

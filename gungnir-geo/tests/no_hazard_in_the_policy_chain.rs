//! **No hazard ever contributes to a policy verdict** (DN-14 §8, the verification row for
//! CAP-2.5; `docs/design/verification-rows.md` names this the negative test that matters).
//!
//! `gungnir-policy` depends on this crate for geofences, so "no edge" cannot be the
//! guard. The guard is that nothing in the policy or command sources so much as names a
//! hazard: the day somebody wires a boom into the authority chain, this fails.

use std::path::Path;

fn sources(crate_dir: &Path) -> Vec<(std::path::PathBuf, String)> {
    fn walk(dir: &Path, out: &mut Vec<(std::path::PathBuf, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    out.push((path, text));
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(&crate_dir.join("src"), &mut out);
    out
}

#[test]
fn the_policy_and_command_crates_never_name_a_hazard() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits in the workspace");
    for krate in ["gungnir-policy", "gungnir-command"] {
        let files = sources(&workspace.join(krate));
        assert!(!files.is_empty(), "{krate} has no sources to scan");
        for (path, text) in files {
            for (i, line) in text.lines().enumerate() {
                assert!(
                    !line.to_ascii_lowercase().contains("hazard"),
                    "{}:{}: the authority chain names a hazard: {line}",
                    path.display(),
                    i + 1
                );
            }
        }
    }
}

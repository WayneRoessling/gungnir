// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Where a desktop with no baseline file keeps its journal (GAP-215, DN-22 §17).
//!
//! The rule is asked of directly, with the environment it would read handed in, so the
//! test neither depends on this machine's profile nor writes into it.

use gungnir_app::state::{per_user_data_dir, LEGACY_DATA_DIR};
use std::ffi::OsString;

fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
    move |name| {
        pairs
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| OsString::from(*v))
    }
}

#[test]
fn a_desktop_with_no_baseline_keeps_its_journal_in_the_users_profile() {
    let windows = cfg!(windows);
    let dir = per_user_data_dir(
        false,
        &env(&[
            ("LOCALAPPDATA", "C:\\Users\\op\\AppData\\Local"),
            ("XDG_DATA_HOME", "/home/op/.data"),
            ("HOME", "/home/op"),
        ]),
    );
    let expected = if windows {
        std::path::Path::new("C:\\Users\\op\\AppData\\Local")
            .join("Gungnir")
            .join("journal")
    } else {
        std::path::Path::new("/home/op/.data")
            .join("gungnir")
            .join("journal")
    };
    assert_eq!(dir, expected.to_string_lossy());

    if !windows {
        let dir = per_user_data_dir(false, &env(&[("HOME", "/home/op")]));
        assert_eq!(
            dir,
            std::path::Path::new("/home/op/.local/share/gungnir/journal").to_string_lossy()
        );
    }
}

/// A journal already under the working directory is kept there, so moving the default
/// does not look like a lost record; and a host with no profile keeps the old place too.
#[test]
fn an_existing_journal_and_a_missing_profile_keep_the_old_directory() {
    let profile = env(&[
        ("LOCALAPPDATA", "C:\\Users\\op\\AppData\\Local"),
        ("HOME", "/home/op"),
    ]);
    assert_eq!(per_user_data_dir(true, &profile), LEGACY_DATA_DIR);
    assert_eq!(per_user_data_dir(false, &env(&[])), LEGACY_DATA_DIR);
    assert_eq!(
        per_user_data_dir(false, &env(&[("LOCALAPPDATA", ""), ("HOME", "")])),
        LEGACY_DATA_DIR
    );
}

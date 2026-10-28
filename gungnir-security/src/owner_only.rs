// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Files only their owner can open (GAP-215; DN-22 §17).
//!
//! **Human-owned** (`gungnir-security`).
//!
//! The sealed keystore, the sealed account store, the cloud-wrapped secret and the account
//! file of password hashes were written with `std::fs::write`, so each took whatever its
//! directory allowed. Every one is written through [`write`] now:
//!
//! - **the bytes go to a file beside the target**, which is restricted and then renamed
//!   over it, so the target is never readable by anyone else even for the moment between
//!   writing and restricting, and a crash never leaves half a file;
//! - **on Unix** the restriction is mode 0600;
//! - **on Windows** it is an access list naming the current user alone, with inheritance
//!   from the directory removed. It is set by the system's own `icacls.exe`, run by its
//!   path under `%SystemRoot%`, with the user's SID from `whoami.exe`. Calling the Win32
//!   security API directly would need `unsafe` code, which GAP-238 is moving the workspace
//!   to forbid, and a crate for it would be a new §2.9 row.
//!
//! **A restriction that fails is said, not hidden.** The file is still written, because a
//! keystore that could not be saved is worse than one whose permissions are wider than
//! asked; [`Restricted::NotApplied`] carries why, and the caller logs it.

use std::path::{Path, PathBuf};

/// What was done about a file's permissions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Restricted {
    /// Mode 0600.
    Unix,
    /// An access list naming the current user alone, inheritance removed.
    WindowsOwner,
    /// Nothing could be applied, and why. The file kept its directory's permissions.
    NotApplied(String),
}

impl Restricted {
    /// One sentence for a log line or a command's output.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Unix => "owner-only (0600)".to_owned(),
            Self::WindowsOwner => "owner-only (an access list naming this user alone)".to_owned(),
            Self::NotApplied(why) => {
                format!("NOT restricted, and inherits its directory's permissions: {why}")
            }
        }
    }
}

/// Write `bytes` to `path`, readable by its owner alone where the platform allows.
///
/// # Errors
///
/// When the file cannot be written or put in place. A restriction that cannot be applied
/// is not an error: it is the [`Restricted::NotApplied`] returned.
pub fn write(path: &Path, bytes: &[u8]) -> std::io::Result<Restricted> {
    let tmp = beside(path);
    std::fs::write(&tmp, bytes)?;
    let restricted = restrict(&tmp);
    if let Err(err) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err);
    }
    if let Restricted::NotApplied(why) = &restricted {
        tracing::warn!(path = %path.display(), %why, "a sealed file could not be restricted to its owner");
    }
    Ok(restricted)
}

/// The file `path` is written to before it is renamed into place.
fn beside(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".tmp");
    PathBuf::from(name)
}

/// Restrict an existing file to its owner.
#[must_use]
pub fn restrict(path: &Path) -> Restricted {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
            Ok(()) => Restricted::Unix,
            Err(err) => Restricted::NotApplied(err.to_string()),
        }
    }
    #[cfg(windows)]
    {
        windows::restrict(path)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Restricted::NotApplied("this platform has no owner-only mechanism here".to_owned())
    }
}

/// Whether `path` is readable by its owner alone, for a test of a writer elsewhere in the
/// crate: on Windows one access entry with full control and nothing inherited, on Unix
/// mode 0600.
#[cfg(test)]
pub(crate) fn is_owner_only(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o777 == 0o600)
    }
    #[cfg(windows)]
    {
        let entries = windows::access_list(path);
        entries.len() == 1 && entries[0].ends_with("(F)") && !entries[0].contains("(I)")
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        false
    }
}

#[cfg(windows)]
mod windows {
    use super::Restricted;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// A tool under `%SystemRoot%\System32`, by its full path, so a same-named program
    /// earlier on `PATH` is never the one run.
    fn system_tool(name: &str) -> PathBuf {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        PathBuf::from(root).join("System32").join(name)
    }

    /// The current user's SID, from `whoami /user`.
    pub(super) fn current_user_sid() -> Result<String, String> {
        let output = Command::new(system_tool("whoami.exe"))
            .args(["/user", "/fo", "csv", "/nh"])
            .output()
            .map_err(|e| format!("whoami.exe did not run: {e}"))?;
        if !output.status.success() {
            return Err(format!("whoami.exe failed: {}", output.status));
        }
        // `"host\user","S-1-5-21-..."`: the SID is the last quoted field.
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .rsplit(',')
            .next()
            .map(|field| field.trim().trim_matches('"').to_owned())
            .filter(|sid| sid.starts_with("S-1-"))
            .ok_or_else(|| "whoami.exe printed no SID".to_owned())
    }

    pub(super) fn restrict(path: &Path) -> Restricted {
        let sid = match current_user_sid() {
            Ok(sid) => sid,
            Err(why) => return Restricted::NotApplied(why),
        };
        let output = Command::new(system_tool("icacls.exe"))
            .arg(path)
            .args(["/inheritance:r", "/grant:r", &format!("*{sid}:F")])
            .output();
        match output {
            Ok(out) if out.status.success() => Restricted::WindowsOwner,
            Ok(out) => Restricted::NotApplied(format!(
                "icacls.exe failed ({}): {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            )),
            Err(err) => Restricted::NotApplied(format!("icacls.exe did not run: {err}")),
        }
    }

    /// The access list `icacls` reports for `path`, one entry per line, for a test to read.
    #[cfg(test)]
    pub(super) fn access_list(path: &Path) -> Vec<String> {
        Command::new(system_tool("icacls.exe"))
            .arg(path)
            .output()
            .map(|out| {
                String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .map(|line| line.replace(&path.display().to_string(), ""))
                    .map(|line| line.trim().to_owned())
                    .filter(|line| line.contains(':') && line.contains('('))
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gungnir-owner-only-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    /// A written file holds its bytes, leaves nothing beside it, and is restricted to its
    /// owner: on Windows, one access entry, not inherited, naming this user with full
    /// control; on Unix, mode 0600.
    #[test]
    fn a_written_file_is_its_owners_alone() {
        let dir = scratch("write");
        let path = dir.join("keystore.sealed");
        let restricted = write(&path, b"sealed").expect("written");
        assert_eq!(std::fs::read(&path).expect("read"), b"sealed");
        assert!(
            !beside(&path).exists(),
            "the file beside it was left behind"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(restricted, Restricted::Unix);
            let mode = std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "{mode:o}");
        }
        #[cfg(windows)]
        {
            assert_eq!(
                restricted,
                Restricted::WindowsOwner,
                "{}",
                restricted.describe()
            );
            let entries = windows::access_list(&path);
            assert_eq!(entries.len(), 1, "{entries:?}");
            assert!(entries[0].ends_with("(F)"), "{entries:?}");
            assert!(
                !entries[0].contains("(I)"),
                "an inherited entry remains: {entries:?}"
            );
        }
        // Written again, it is replaced and stays restricted.
        let again = write(&path, b"resealed").expect("rewritten");
        assert_eq!(again, restricted);
        assert_eq!(std::fs::read(&path).expect("read"), b"resealed");
        let _ = std::fs::remove_dir_all(dir);
    }
}

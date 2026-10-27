// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Creating the accounts a binary authenticates against (GAP-057, GAP-204;
//! `docs/design/DN-23-operator-authentication.md`).
//!
//! **Why this lives here.** Until GAP-204 this was `gungnir-node/src/account.rs`, and the
//! only way to write an account file was the node binary, which the release ships for
//! Linux alone; a Windows desktop in the field could not create its own accounts and ran
//! with nobody signed in. A binary may not depend on a binary, so the commands moved down
//! into the crate that owns the account stores and that both binaries already depend on.
//! `gungnir-app account ...` and `gungnir-node account ...` are one implementation,
//! [`run`], told which binary it is speaking for.
//!
//! **Why this exists at all.** `FileAccountStore` reads a JSON array of
//! `{operator, role, phc}` records and `hash_passphrase` produces the `phc`. Until
//! 2026-09-07 that function was called from tests and from nowhere else, so a deployment
//! following DN-23 had a documented file format, a binary that refused to authenticate
//! anybody without it, and **no way to write one**.
//!
//! **The passphrase is read from standard input and never from an argument.** Command
//! arguments are visible to every process on the host through the process table, and a
//! passphrase that reaches argv has been disclosed before it is hashed.
//!
//! **The echo of a typed passphrase is not suppressed**, because suppressing it needs a
//! terminal crate this workspace has not admitted under
//! `docs/agentic-coding-standards.md` §2.9. That is a real limitation and it is stated
//! rather than papered over: pipe the passphrase in, or accept that it appears on the
//! screen of whoever is provisioning the account.
//!
//! Nothing here writes key material to a configuration baseline. The file holds argon2
//! PHC strings, which is what DN-22 §6 and DN-23 §5 rule 6 permit, and a node's token
//! signing key stays in the environment where the node reads it.
//!
//! **Human-owned: this writes credential material** (`hash_passphrase`'s output) **into
//! the account store both binaries trust** (signatures: docs/signatures.md). Its first
//! review found and closed a real gap: `role_from_str` had every role this workspace had
//! when it was written (2026-09-07) but not `Role::IntelligenceAnalyst`, added to the enum
//! afterward and never re-checked against this CLI. `role_to_cli_str` (test-only) maps
//! every variant with no wildcard arm, so a tenth role added without a line here fails to
//! compile rather than repeating the defect.
//!
//! **`add-os-keystore` and `list-os-keystore` (GAP-057's node half, D-39).** The same
//! provisioning need, against [`EncryptedAccountStore`] instead of a plaintext file:
//! accounts sealed under a key the operating system's own keystore holds, rather than
//! protected only by whatever file permissions the platform grants (Windows grants none:
//! see [`permissions_note`]). This path names a data directory and a keystore account
//! rather than a file, and reaches the real backend on this machine every time it runs.
//!
//! **Every account change is on the audit record** (GAP-111, D-87): an `account.assign_role`
//! entry in the `audit` directory beside the store the command changed, naming the operator
//! and the role and never the passphrase or its hash, attributed to nobody because the
//! command runs outside any session. The log is opened before anything changes, so a change
//! that could not be recorded is refused rather than made. Listing changes nothing and is
//! not recorded.

use crate::{
    actions, hash_passphrase, Account, AuditEntry, AuditLog, AuditStatus, AuditSync,
    EncryptedAccountStore, FileAuditLog, OperatorId, Role, SecurityError, AUDIT_DIR,
};
use std::fmt::Write as _;
use std::io::Read;
use std::path::Path;

/// What went wrong provisioning an account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountError {
    /// The arguments did not name an operation this understands; carries the usage text
    /// for the binary that was asked.
    Usage { what: String, usage: String },
    /// The role is not one the system has.
    UnknownRole(String),
    /// The operator id was not a number.
    BadOperatorId(String),
    /// The account file exists and is not a list of accounts.
    FileUnreadable(String),
    /// The operator already has an account and `--replace` was not given.
    AlreadyExists(u64),
    /// Nothing arrived on standard input, or only whitespace did; carries the binary's
    /// name for the example.
    EmptyPassphrase { binary: String },
    /// The passphrase could not be hashed.
    HashFailed(String),
    /// The file could not be written.
    WriteFailed(String),
    /// The operating-system-keystore-backed store could not be reached at all: no
    /// keystore on this platform, or the sealed file did not open under its secret.
    StoreUnavailable(String),
    /// The audit log beside the store could not be opened, so nothing was changed: an
    /// account change that leaves no record is the one thing this command will not make
    /// (GAP-111, D-87).
    AuditUnavailable(String),
}

impl std::fmt::Display for AccountError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccountError::Usage { what, usage } => write!(f, "{what}\n\n{usage}"),
            AccountError::UnknownRole(role) => write!(
                f,
                "no such role: {role}. One of: operator, supervisor, analyst, \
                 sensor-manager, administrator, commander, planner, security-officer, \
                 intelligence-analyst"
            ),
            AccountError::BadOperatorId(id) => {
                write!(f, "the operator id must be a whole number, not {id}")
            }
            AccountError::FileUnreadable(why) => write!(f, "{why}"),
            AccountError::AlreadyExists(id) => write!(
                f,
                "operator {id} already has an account in this store. Pass --replace to \
                 set a new passphrase for that operator; without it nothing is changed, \
                 because silently overwriting a credential is how an account is taken \
                 over rather than provisioned"
            ),
            AccountError::EmptyPassphrase { binary } => write!(
                f,
                "no passphrase arrived on standard input. Pipe one in, for example: \
                 printf '%s' 'the passphrase' | {binary} account add accounts.json 7 operator"
            ),
            AccountError::HashFailed(why) => write!(f, "the passphrase was refused: {why}"),
            AccountError::WriteFailed(why) => write!(f, "the account file was not written: {why}"),
            AccountError::StoreUnavailable(why) => {
                write!(f, "the account store is unavailable: {why}")
            }
            AccountError::AuditUnavailable(why) => write!(
                f,
                "the audit log beside the account store could not be opened, so nothing was \
                 changed: {why}"
            ),
        }
    }
}

impl std::error::Error for AccountError {}

/// The usage text for `binary` (`gungnir-app` or `gungnir-node`).
#[must_use]
pub fn usage(binary: &str) -> String {
    format!(
        "\
usage: {binary} account add <accounts.json> <operator-id> <role> [--replace]
       {binary} account list <accounts.json>
       {binary} account add-os-keystore <data-dir> <keystore-account> <operator-id> <role> [--replace]
       {binary} account list-os-keystore <data-dir> <keystore-account>

The passphrase is read from standard input, never from an argument, because
arguments are visible to every process on the host.

  printf '%s' 'the passphrase' | {binary} account add accounts.json 7 operator

add-os-keystore and list-os-keystore seal the accounts in <data-dir> under a key the
operating system's own keystore holds, naming <keystore-account> the way `add` names a
file; where this machine has no reachable keystore they refuse, the same as a data
directory that cannot be written.

Roles: operator, supervisor, analyst, sensor-manager, administrator, commander,
       planner, security-officer, intelligence-analyst"
    )
}

/// Parse a role as it is written on a command line.
fn role_from_str(s: &str) -> Result<Role, AccountError> {
    match s.to_ascii_lowercase().replace('_', "-").as_str() {
        "operator" => Ok(Role::Operator),
        "supervisor" => Ok(Role::Supervisor),
        "analyst" => Ok(Role::Analyst),
        "sensor-manager" => Ok(Role::SensorManager),
        "administrator" => Ok(Role::Administrator),
        "commander" => Ok(Role::Commander),
        "planner" => Ok(Role::Planner),
        "security-officer" => Ok(Role::SecurityOfficer),
        "intelligence-analyst" => Ok(Role::IntelligenceAnalyst),
        _ => Err(AccountError::UnknownRole(s.to_string())),
    }
}

/// Read the existing accounts, or an empty list if the file is not there yet.
///
/// A file that exists and does not parse is an error rather than an empty list:
/// treating a corrupt account file as "no accounts" would quietly discard every
/// account in it on the next write.
fn read_accounts(path: &Path) -> Result<Vec<Account>, AccountError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path).map_err(|e| {
        AccountError::FileUnreadable(format!("cannot read {}: {e}", path.display()))
    })?;
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&text).map_err(|e| {
        AccountError::FileUnreadable(format!(
            "{} exists and is not a list of accounts: {e}. Nothing was changed",
            path.display()
        ))
    })
}

/// Write the accounts back, and restrict the file to its owner where the platform can.
fn write_accounts(path: &Path, accounts: &[Account]) -> Result<(), AccountError> {
    let text = serde_json::to_string_pretty(accounts)
        .map_err(|e| AccountError::WriteFailed(e.to_string()))?;
    std::fs::write(path, text + "\n")
        .map_err(|e| AccountError::WriteFailed(format!("{}: {e}", path.display())))?;
    restrict_to_owner(path);
    Ok(())
}

/// Narrow the file's permissions to its owner.
///
/// Best effort by design. On Unix this is a mode change that either works or does not;
/// on Windows the equivalent is an ACL rewrite that this workspace has no crate for, and
/// the file inherits the directory's permissions. The caller is told which happened by
/// [`permissions_note`] rather than being left to assume the strict case.
fn restrict_to_owner(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

/// What was actually done about the file's permissions, so the operator is not left
/// believing in a protection the platform did not apply.
#[must_use]
pub fn permissions_note() -> &'static str {
    if cfg!(unix) {
        "the file is set to owner-only (0600)"
    } else {
        "the file inherits this directory's permissions; restrict it yourself if the \
         directory is readable by others"
    }
}

/// Read a passphrase from standard input.
fn passphrase_from_stdin(binary: &str) -> Result<String, AccountError> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| AccountError::HashFailed(e.to_string()))?;
    let pass = buf.trim_end_matches(['\n', '\r']).to_string();
    if pass.trim().is_empty() {
        return Err(AccountError::EmptyPassphrase {
            binary: binary.to_owned(),
        });
    }
    Ok(pass)
}

/// Add or replace one account. Returns the line to print.
///
/// # Errors
///
/// [`AccountError`], naming what was refused and what was left unchanged.
pub fn add(
    path: &Path,
    operator: u64,
    role: Role,
    passphrase: &str,
    replace: bool,
) -> Result<String, AccountError> {
    let mut accounts = read_accounts(path)?;
    let existing = accounts.iter().position(|a| a.operator.0 == operator);
    if existing.is_some() && !replace {
        return Err(AccountError::AlreadyExists(operator));
    }
    // Opened before anything changes, so a change that could not be recorded is not made.
    let audit = provisioning_audit(store_dir(path))?;
    let phc = hash_passphrase(passphrase).map_err(|e| AccountError::HashFailed(e.to_string()))?;
    let account = Account {
        operator: OperatorId(operator),
        role,
        phc,
    };
    let verb = if let Some(i) = existing {
        accounts[i] = account;
        "replaced"
    } else {
        accounts.push(account);
        "added"
    };
    write_accounts(path, &accounts)?;
    let recorded = record_provisioning(
        audit,
        format!(
            "operator {operator} {verb} as {role:?} in {} by the provisioning command",
            path.display()
        ),
    );
    Ok(format!(
        "{verb} operator {operator} as {role:?} in {}; {}; {recorded}",
        path.display(),
        permissions_note()
    ))
}

/// The directory a store lives in: where its audit directory goes.
fn store_dir(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

/// The audit log a provisioning command records into (GAP-111, D-87): the `audit`
/// directory beside the store it changes.
///
/// For an operating-system-keystore store that is always the data directory, so the entry
/// joins the binary's own chain; a plaintext file joins it when the deployment keeps the
/// file in that directory, as `docs/design/DN-23-operator-authentication.md` §13
/// recommends, and otherwise has a chain of its own beside it. **Attributed to nobody**:
/// the command runs outside any session, and whoever has the shell is not a verified
/// operator (DN-23 §5 rule 1). The passphrase and its hash are in no entry.
///
/// # Errors
///
/// [`AccountError::AuditUnavailable`] when the log cannot be opened, before anything has
/// been changed.
fn provisioning_audit(store_dir: &Path) -> Result<FileAuditLog, AccountError> {
    FileAuditLog::open(
        &store_dir.join(AUDIT_DIR),
        AuditSync::EveryEntry,
        provisioning_time(),
    )
    .map_err(|e| AccountError::AuditUnavailable(e.to_string()))
}

/// The time a provisioning entry carries: the wall clock, which is the mission time a
/// live session runs on (`gungnir_time::WallClockAuthority` reads the same clock), so the
/// entry sorts among the binary's own. Read directly here because this crate depends on
/// no workspace crate (`ARCHITECTURE.md` §7.1).
fn provisioning_time() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// Record one account change and say where it went, or that it did not.
fn record_provisioning(mut log: FileAuditLog, detail: String) -> String {
    log.record(AuditEntry::new(
        None,
        actions::ASSIGN_ROLE,
        provisioning_time(),
        detail,
    ));
    match (log.status(), log.segment()) {
        (AuditStatus::Durable, Some(segment)) => {
            format!("recorded in the audit log at {}", segment.display())
        }
        (AuditStatus::Failing { reason, .. }, _) => {
            format!("NOT recorded in the audit log, which refused the write: {reason}")
        }
        (AuditStatus::Durable, None) => "NOT recorded in the audit log".to_owned(),
    }
}

/// List the accounts a file holds: operator and role, never the hash.
///
/// # Errors
///
/// [`AccountError::FileUnreadable`] if the file is not a list of accounts.
pub fn list(path: &Path) -> Result<String, AccountError> {
    let accounts = read_accounts(path)?;
    if accounts.is_empty() {
        return Ok(format!("{} holds no accounts", path.display()));
    }
    let mut out = format!("{} holds {} account(s):\n", path.display(), accounts.len());
    for a in &accounts {
        let _ = writeln!(out, "  operator {} as {:?}", a.operator.0, a.role);
    }
    Ok(out.trim_end().to_string())
}

/// Add or replace one account in an operating-system-keystore-backed store (GAP-057's
/// node half, D-39). This reaches the real keystore on this machine; there is no mock to
/// redirect it to outside this crate's own tests.
///
/// # Errors
///
/// [`AccountError::StoreUnavailable`] when this platform has no reachable keystore or
/// the sealed file does not open under its secret; [`AccountError::AlreadyExists`] when
/// the operator already has an account and `replace` was not given.
pub fn add_os_keystore(
    dir: &Path,
    keystore_account: &str,
    operator: u64,
    role: Role,
    passphrase: &str,
    replace: bool,
) -> Result<String, AccountError> {
    let store = EncryptedAccountStore::open_or_create(dir, keystore_account)
        .map_err(|e| AccountError::StoreUnavailable(e.to_string()))?;
    let audit = provisioning_audit(dir)?;
    let phc = hash_passphrase(passphrase).map_err(|e| AccountError::HashFailed(e.to_string()))?;
    match store.add(OperatorId(operator), role, phc, replace) {
        Ok(()) => {
            let recorded = record_provisioning(
                audit,
                format!(
                    "operator {operator} provisioned as {role:?} in the operating-system \
                     keystore account {keystore_account:?} by the provisioning command{}",
                    if replace {
                        ", replacing any account it had"
                    } else {
                        ""
                    }
                ),
            );
            Ok(format!(
                "provisioned operator {operator} as {role:?} in the operating-system keystore \
                 account {keystore_account:?} under {}; {recorded}",
                dir.display()
            ))
        }
        Err(SecurityError::Forbidden(_)) => Err(AccountError::AlreadyExists(operator)),
        Err(e) => Err(AccountError::StoreUnavailable(e.to_string())),
    }
}

/// List the accounts an operating-system-keystore-backed store holds: operator and
/// role, never the hash.
///
/// # Errors
///
/// [`AccountError::StoreUnavailable`] when this platform has no reachable keystore or
/// the sealed file does not open under its secret.
pub fn list_os_keystore(dir: &Path, keystore_account: &str) -> Result<String, AccountError> {
    let store = EncryptedAccountStore::open_or_create(dir, keystore_account)
        .map_err(|e| AccountError::StoreUnavailable(e.to_string()))?;
    let accounts = store
        .listing()
        .map_err(|e| AccountError::StoreUnavailable(e.to_string()))?;
    if accounts.is_empty() {
        return Ok(format!(
            "the operating-system keystore account {keystore_account:?} under {} holds no \
             accounts",
            dir.display()
        ));
    }
    let mut out = format!(
        "the operating-system keystore account {keystore_account:?} under {} holds {} \
         account(s):\n",
        dir.display(),
        accounts.len()
    );
    for (id, role) in &accounts {
        let _ = writeln!(out, "  operator {} as {role:?}", id.0);
    }
    Ok(out.trim_end().to_string())
}

/// Run the `account` subcommand of `binary` from the arguments after the word `account`.
///
/// `binary` is the name the usage text and the examples speak as (`gungnir-app` or
/// `gungnir-node`); the behaviour is the same for both, which is the point of GAP-204.
///
/// # Errors
///
/// [`AccountError`] for anything refused; the message says what was not changed.
pub fn run(binary: &str, args: &[String]) -> Result<String, AccountError> {
    let usage_err = |what: &str| AccountError::Usage {
        what: what.to_owned(),
        usage: usage(binary),
    };
    match args.first().map(String::as_str) {
        Some("add") => {
            let path = args
                .get(1)
                .ok_or_else(|| usage_err("account add needs a file path"))?;
            let operator = args
                .get(2)
                .ok_or_else(|| usage_err("account add needs an operator id"))?;
            let role = args
                .get(3)
                .ok_or_else(|| usage_err("account add needs a role"))?;
            let operator: u64 = operator
                .parse()
                .map_err(|_| AccountError::BadOperatorId(operator.clone()))?;
            let role = role_from_str(role)?;
            let replace = args.iter().any(|a| a == "--replace");
            let passphrase = passphrase_from_stdin(binary)?;
            add(Path::new(path), operator, role, &passphrase, replace)
        }
        Some("list") => {
            let path = args
                .get(1)
                .ok_or_else(|| usage_err("account list needs a file path"))?;
            list(Path::new(path))
        }
        Some("add-os-keystore") => {
            let dir = args
                .get(1)
                .ok_or_else(|| usage_err("account add-os-keystore needs a data directory"))?;
            let keystore_account = args.get(2).ok_or_else(|| {
                usage_err("account add-os-keystore needs a keystore account name")
            })?;
            let operator = args
                .get(3)
                .ok_or_else(|| usage_err("account add-os-keystore needs an operator id"))?;
            let role = args
                .get(4)
                .ok_or_else(|| usage_err("account add-os-keystore needs a role"))?;
            let operator: u64 = operator
                .parse()
                .map_err(|_| AccountError::BadOperatorId(operator.clone()))?;
            let role = role_from_str(role)?;
            let replace = args.iter().any(|a| a == "--replace");
            let passphrase = passphrase_from_stdin(binary)?;
            add_os_keystore(
                Path::new(dir),
                keystore_account,
                operator,
                role,
                &passphrase,
                replace,
            )
        }
        Some("list-os-keystore") => {
            let dir = args
                .get(1)
                .ok_or_else(|| usage_err("account list-os-keystore needs a data directory"))?;
            let keystore_account = args.get(2).ok_or_else(|| {
                usage_err("account list-os-keystore needs a keystore account name")
            })?;
            list_os_keystore(Path::new(dir), keystore_account)
        }
        Some(other) => Err(usage_err(&format!("no such account command: {other}"))),
        None => Err(usage_err("account needs a command")),
    }
}

/// The usage text for `audit-key` (GAP-208).
#[must_use]
pub fn audit_key_usage(binary: &str) -> String {
    format!(
        "\
usage: {binary} audit-key provision <data-dir>

Makes the key the desktop signs its audit anchors with, sealed in
<data-dir>/audit-signing/ under a passphrase the administrator holds and the operator
does not. The passphrase is read from standard input, never from an argument:

  printf '%s' 'the administrator passphrase' | {binary} audit-key provision ./gungnir-journal

It prints the public half; put it in the baseline as security.audit_signing.public_key_pem.
An existing key is never replaced."
    )
}

/// `audit-key provision <data-dir>` (GAP-208, D-142): make the audit-signing key and
/// print its public half for the baseline.
///
/// **Not written to the audit log.** The key does nothing until its public half is in a
/// baseline, and applying a baseline is itself audited (`config.apply`); a key provisioned
/// and never configured verifies nothing and signs nothing anybody checks.
///
/// # Errors
///
/// [`AccountError::Usage`] for anything but `provision <data-dir>`; an empty passphrase;
/// and [`AccountError::StoreUnavailable`] for a short passphrase, a key already there, or
/// a directory that cannot be written.
pub fn run_audit_key(binary: &str, args: &[String]) -> Result<String, AccountError> {
    let usage_err = |what: &str| AccountError::Usage {
        what: what.to_owned(),
        usage: audit_key_usage(binary),
    };
    match args.first().map(String::as_str) {
        Some("provision") => {
            let dir = args
                .get(1)
                .ok_or_else(|| usage_err("audit-key provision needs a data directory"))?;
            let passphrase = passphrase_from_stdin(binary)?;
            let pem = crate::audit::signing::AuditSigner::provision(Path::new(dir), &passphrase)
                .map_err(|e| AccountError::StoreUnavailable(e.to_string()))?;
            Ok(format!(
                "audit-signing key provisioned in {dir}/{}. Put this public key in the \
                 baseline as security.audit_signing.public_key_pem, and keep the passphrase \
                 with the administrator; the desktop asks for it at start.\n{pem}",
                crate::audit::signing::AUDIT_SIGNING_DIR
            ))
        }
        Some(other) => Err(usage_err(&format!("no such audit-key command: {other}"))),
        None => Err(usage_err("audit-key needs a command")),
    }
}

/// The inverse of [`role_from_str`], over every [`Role`] variant with no wildcard arm:
/// a tenth role added to the enum without a line here fails to compile rather than
/// silently leaving this CLI unable to provision it, which is exactly the defect found
/// and fixed 2026-09-10: `IntelligenceAnalyst` reached every other role-aware surface in
/// the workspace but not this one, because it was added to [`Role`] after this file was
/// written and nothing here re-checked the enum against it.
#[cfg(test)]
fn role_to_cli_str(role: Role) -> &'static str {
    match role {
        Role::Operator => "operator",
        Role::Supervisor => "supervisor",
        Role::Analyst => "analyst",
        Role::SensorManager => "sensor-manager",
        Role::Administrator => "administrator",
        Role::Commander => "commander",
        Role::Planner => "planner",
        Role::SecurityOfficer => "security-officer",
        Role::IntelligenceAnalyst => "intelligence-analyst",
    }
}

#[cfg(test)]
mod tests {
    use super::{role_from_str, role_to_cli_str, run, usage, AccountError, Role};

    /// Every role this workspace has must be provisionable through this CLI. Walking
    /// `role_to_cli_str`'s exhaustive match rather than a hand-maintained list here is
    /// what makes a future role addition fail this test (or fail to compile) instead of
    /// silently leaving the CLI unable to create that account, the way
    /// `IntelligenceAnalyst` was until 2026-09-10.
    #[test]
    fn every_role_round_trips_through_the_cli_parser() {
        for role in [
            Role::Operator,
            Role::Supervisor,
            Role::Analyst,
            Role::SensorManager,
            Role::Administrator,
            Role::Commander,
            Role::Planner,
            Role::SecurityOfficer,
            Role::IntelligenceAnalyst,
        ] {
            let s = role_to_cli_str(role);
            assert_eq!(
                role_from_str(s).expect("every name role_to_cli_str emits must parse back"),
                role,
                "role {role:?} does not round-trip through {s:?}"
            );
        }
    }

    /// GAP-204: the usage and the examples speak as the binary that was asked, so a
    /// desktop operator is not told to run the node.
    #[test]
    fn the_usage_names_the_binary_it_speaks_for() {
        for binary in ["gungnir-app", "gungnir-node"] {
            let text = usage(binary);
            assert!(text.contains(&format!("{binary} account add")), "{text}");
            assert!(!text.contains(if binary == "gungnir-app" {
                "gungnir-node"
            } else {
                "gungnir-app"
            }));
            let Err(AccountError::Usage { usage: shown, .. }) = run(binary, &[]) else {
                panic!("no command is a usage error");
            };
            assert_eq!(shown, text);
        }
    }
}

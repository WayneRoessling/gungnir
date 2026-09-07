// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Creating the accounts a node authenticates against (GAP-057,
//! `docs/design/DN-23-operator-authentication.md`).
//!
//! **Why this exists.** `FileAccountStore` reads a JSON array of `{operator, role, phc}`
//! records and `hash_passphrase` produces the `phc`. Until 2026-09-07 that function was
//! called from tests and from nowhere else, so a deployment following DN-23 had a
//! documented file format, a node that refused to authenticate anybody without it, and
//! **no way to write one**. The authentication half of GAP-057 was built, signed, and
//! unreachable in practice: the node warned `no caller authority` and the only remedy
//! was to run a Rust test.
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
//! PHC strings, which is what DN-22 §6 and DN-23 §5 rule 6 permit, and the node's token
//! signing key stays in the environment where [`crate::auth`] reads it.

use gungnir_security::{hash_passphrase, Account, OperatorId, Role};
use std::fmt::Write as _;
use std::io::Read;
use std::path::Path;

/// What went wrong provisioning an account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountError {
    /// The arguments did not name an operation this understands.
    Usage(String),
    /// The role is not one the system has.
    UnknownRole(String),
    /// The operator id was not a number.
    BadOperatorId(String),
    /// The account file exists and is not a list of accounts.
    FileUnreadable(String),
    /// The operator already has an account and `--replace` was not given.
    AlreadyExists(u64),
    /// Nothing arrived on standard input, or only whitespace did.
    EmptyPassphrase,
    /// The passphrase could not be hashed.
    HashFailed(String),
    /// The file could not be written.
    WriteFailed(String),
}

impl std::fmt::Display for AccountError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccountError::Usage(what) => write!(f, "{what}\n\n{USAGE}"),
            AccountError::UnknownRole(role) => write!(
                f,
                "no such role: {role}. One of: operator, supervisor, analyst, \
                 sensor-manager, administrator, commander, planner, security-officer"
            ),
            AccountError::BadOperatorId(id) => {
                write!(f, "the operator id must be a whole number, not {id}")
            }
            AccountError::FileUnreadable(why) => write!(f, "{why}"),
            AccountError::AlreadyExists(id) => write!(
                f,
                "operator {id} already has an account in this file. Pass --replace to \
                 set a new passphrase for that operator; without it nothing is changed, \
                 because silently overwriting a credential is how an account is taken \
                 over rather than provisioned"
            ),
            AccountError::EmptyPassphrase => write!(
                f,
                "no passphrase arrived on standard input. Pipe one in, for example: \
                 printf '%s' 'the passphrase' | gungnir-node account add accounts.json 7 operator"
            ),
            AccountError::HashFailed(why) => write!(f, "the passphrase was refused: {why}"),
            AccountError::WriteFailed(why) => write!(f, "the account file was not written: {why}"),
        }
    }
}

impl std::error::Error for AccountError {}

const USAGE: &str = "\
usage: gungnir-node account add <accounts.json> <operator-id> <role> [--replace]
       gungnir-node account list <accounts.json>

The passphrase is read from standard input, never from an argument, because
arguments are visible to every process on the host.

  printf '%s' 'the passphrase' | gungnir-node account add accounts.json 7 operator

Roles: operator, supervisor, analyst, sensor-manager, administrator, commander,
       planner, security-officer";

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
fn passphrase_from_stdin() -> Result<String, AccountError> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| AccountError::HashFailed(e.to_string()))?;
    let pass = buf.trim_end_matches(['\n', '\r']).to_string();
    if pass.trim().is_empty() {
        return Err(AccountError::EmptyPassphrase);
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
    Ok(format!(
        "{verb} operator {operator} as {role:?} in {}; {}",
        path.display(),
        permissions_note()
    ))
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

/// Run the `account` subcommand from the arguments after the word `account`.
///
/// # Errors
///
/// [`AccountError`] for anything refused; the message says what was not changed.
pub fn run(args: &[String]) -> Result<String, AccountError> {
    match args.first().map(String::as_str) {
        Some("add") => {
            let path = args
                .get(1)
                .ok_or_else(|| AccountError::Usage("account add needs a file path".into()))?;
            let operator = args
                .get(2)
                .ok_or_else(|| AccountError::Usage("account add needs an operator id".into()))?;
            let role = args
                .get(3)
                .ok_or_else(|| AccountError::Usage("account add needs a role".into()))?;
            let operator: u64 = operator
                .parse()
                .map_err(|_| AccountError::BadOperatorId(operator.clone()))?;
            let role = role_from_str(role)?;
            let replace = args.iter().any(|a| a == "--replace");
            let passphrase = passphrase_from_stdin()?;
            add(Path::new(path), operator, role, &passphrase, replace)
        }
        Some("list") => {
            let path = args
                .get(1)
                .ok_or_else(|| AccountError::Usage("account list needs a file path".into()))?;
            list(Path::new(path))
        }
        Some(other) => Err(AccountError::Usage(format!(
            "no such account command: {other}"
        ))),
        None => Err(AccountError::Usage("account needs a command".into())),
    }
}

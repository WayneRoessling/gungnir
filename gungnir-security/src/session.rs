//! Operator sessions: who is signed in, and why nobody is (GAP-057).
//!
//! Design: `docs/design/DN-23-operator-authentication.md`, signed off 2026-09-05.
//! Mechanism: D-02. Crates: D-20 (`argon2`, `hmac`/`sha2`, `subtle`).
//!
//! # The rule the whole module exists for
//!
//! **Authentication never invents attribution.** Before this crate could verify anybody,
//! three built things had to say they did not know who acted: a `DecisionRecord` with no
//! operator, a `Concurrence::UnattributedRole`, and two v2 write paths returning `501`.
//! Those types stay, and stay correct, for a deployment that cannot authenticate. What
//! changes is that a deployment that *can* now fills them in with somebody who was
//! actually checked.
//!
//! Nothing here produces an [`OperatorSession`] except a successful verification. There is
//! no constructor taking a bare [`OperatorId`], for the reason `Handoff` has none taking a
//! bare plan (DN-07): a type that can be built from an identifier alone is a type that
//! will be, somewhere, by someone in a hurry.

use crate::{AuditEntry, AuditLog, OperatorId, Role, SecurityError};

/// Mission time in seconds, as `AuditEntry` already carries it.
///
/// `gungnir-security` does not depend on `gungnir-model`, so this is the same plain
/// `f64` the audit log uses rather than a `MissionTime`.
pub type MissionTimeSeconds = f64;

/// A verified operator, and how long the system will keep believing it.
///
/// Held only after a credential was checked. Constructed inside this module and nowhere
/// else -- see the module documentation.
#[derive(Debug, Clone, PartialEq)]
pub struct OperatorSession {
    pub operator: OperatorId,
    pub role: Role,
    pub established: MissionTimeSeconds,
    /// Absent means a session that does not expire, which **only** the disconnected
    /// desktop's local-account profile may issue (DN-23 §5). A node-issued session
    /// always expires.
    pub expires: Option<MissionTimeSeconds>,
}

impl OperatorSession {
    #[must_use]
    pub fn is_valid_at(&self, now: MissionTimeSeconds) -> bool {
        self.expires.is_none_or(|expires| now < expires)
    }

    /// Seconds until expiry, or `None` for a session that does not expire.
    ///
    /// `None` here means "no expiry configured", not "unknown" -- the same distinction
    /// `TimeRemaining` draws for the approval queue.
    #[must_use]
    pub fn remaining(&self, now: MissionTimeSeconds) -> Option<MissionTimeSeconds> {
        self.expires.map(|expires| expires - now)
    }
}

/// Who is signed in, or why nobody is.
///
/// Not `Option<OperatorSession>`. "Nobody has signed in", "the session expired" and "the
/// account store cannot be reached" are three different facts and the status strip has to
/// tell them apart -- the same argument `EmptyBecause` settles for the approval queue.
/// Only the third is a fault.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionState {
    SignedIn(OperatorSession),
    NobodySignedIn,
    Expired {
        operator: OperatorId,
        at: MissionTimeSeconds,
    },
    /// The store is unreachable, so nobody *can* sign in. Distinct from nobody having
    /// tried: this one belongs in the health summary.
    StoreUnavailable {
        reason: String,
    },
}

impl SessionState {
    /// The session, when there is a valid one.
    #[must_use]
    pub fn session(&self) -> Option<&OperatorSession> {
        match self {
            SessionState::SignedIn(session) => Some(session),
            _ => None,
        }
    }

    /// The operator to attribute an act to, or `None`.
    ///
    /// **The only way attribution should be obtained.** A caller that reached for
    /// `session.operator` without checking the state could attribute an act to an expired
    /// session.
    #[must_use]
    pub fn operator(&self) -> Option<OperatorId> {
        self.session().map(|s| s.operator)
    }

    /// Whether this state is a fault rather than an ordinary absence.
    #[must_use]
    pub fn is_fault(&self) -> bool {
        matches!(self, SessionState::StoreUnavailable { .. })
    }
}

/// Why an attempt failed.
///
/// **Never says which half was wrong.** Distinguishing an unknown operator from a bad
/// passphrase enumerates the operator identifiers, so both are [`AuthFailure::Rejected`].
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum AuthFailure {
    #[error("the credential was rejected")]
    Rejected,
    /// Too many attempts too quickly. **Not a lockout**: DN-23 §5 rule 3 backs off rather
    /// than locking, because an operator locked out of a command-and-control console
    /// during an engagement is a worse outcome than a slow attempt against a console
    /// already inside a defended network.
    #[error("too many attempts; try again in {retry_after_s:.0} s")]
    TooFast { retry_after_s: MissionTimeSeconds },
    #[error("the account store is unavailable")]
    Unavailable,
}

/// Establishing and ending sessions.
///
/// Separate from [`crate::Authenticator`] because checking a credential and holding a
/// session are different acts, and only the second one has a clock.
pub trait SessionAuthority: Send + Sync {
    /// Verify a credential and establish a session.
    ///
    /// The credential encoding is the implementation's business; [`LocalAccountAuthority`]
    /// documents its own.
    fn sign_in(
        &mut self,
        credential: &[u8],
        now: MissionTimeSeconds,
    ) -> Result<OperatorSession, AuthFailure>;

    fn sign_out(&mut self, now: MissionTimeSeconds);

    /// Who is signed in as of `now`.
    ///
    /// Takes the time because expiry happens on the clock, not on an event: a caller that
    /// asked without one would be told a session is live because nothing had touched it.
    fn state(&self, now: MissionTimeSeconds) -> SessionState;
}

/// The back-off schedule after failed attempts.
///
/// Doubling from a small base, capped. Deliberately not a lockout: see
/// [`AuthFailure::TooFast`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BackOff {
    /// Attempts allowed before any delay is imposed.
    pub free_attempts: u32,
    pub base_s: MissionTimeSeconds,
    pub max_s: MissionTimeSeconds,
}

impl Default for BackOff {
    fn default() -> Self {
        Self {
            // Two free attempts covers the ordinary typo without making a guessing
            // attack cheap.
            free_attempts: 2,
            base_s: 1.0,
            max_s: 30.0,
        }
    }
}

impl BackOff {
    /// How long to wait after `failures` consecutive failures.
    #[must_use]
    pub fn delay_after(&self, failures: u32) -> MissionTimeSeconds {
        if failures <= self.free_attempts {
            return 0.0;
        }
        let steps = failures - self.free_attempts - 1;
        // Saturating rather than shifting: a long-running console must not wrap this
        // into a zero delay.
        let factor = 2f64.powi(i32::try_from(steps.min(30)).unwrap_or(30));
        (self.base_s * factor).min(self.max_s)
    }
}

/// One account as the store holds it.
///
/// The hash is a PHC string produced by `argon2`; **no passphrase is stored anywhere**,
/// and this struct is what a store persists.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Account {
    pub operator: OperatorId,
    pub role: Role,
    /// An argon2 PHC string, e.g. `$argon2id$v=19$m=19456,t=2,p=1$...`.
    ///
    /// Never a passphrase, and never anything a configuration baseline may carry
    /// (DN-22 §6, DN-23 §5 rule 6).
    pub phc: String,
}

/// Where accounts come from.
///
/// A trait so the profile decides: the disconnected desktop reads the operating system's
/// keystore, a node reads its own store. `Err` means the store could not be reached,
/// which is [`SessionState::StoreUnavailable`] and not a rejection -- the difference
/// between "you are not who you say" and "I cannot tell" is the whole point of separating
/// them.
pub trait AccountStore: Send + Sync {
    fn account(&self, operator: OperatorId) -> Result<Option<Account>, SecurityError>;

    /// Whether the store can be reached at all, asked once at construction.
    ///
    /// Without this a desktop with no account store would come up reporting
    /// `NobodySignedIn` and only discover the truth when somebody tried to sign in --
    /// so the status strip and the health summary would say "nobody has signed in" about
    /// a console where nobody *can*. DN-23 §5 rule 5 requires the second, and those are
    /// different facts.
    ///
    /// The default is `Ok(())` for a store that cannot fail to be present.
    fn available(&self) -> Result<(), SecurityError> {
        Ok(())
    }
}

/// An in-memory store, for tests and for a desktop that has loaded its accounts.
#[derive(Debug, Default, Clone)]
pub struct InMemoryAccountStore {
    accounts: Vec<Account>,
    /// When set, every lookup fails. Models the unreachable-store case, which is a real
    /// state the desktop must start in rather than a test-only fiction.
    unavailable: Option<String>,
}

impl InMemoryAccountStore {
    #[must_use]
    pub fn new(accounts: Vec<Account>) -> Self {
        Self {
            accounts,
            unavailable: None,
        }
    }

    /// A store that cannot be reached.
    #[must_use]
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            accounts: Vec::new(),
            unavailable: Some(reason.into()),
        }
    }
}

/// Local accounts in a JSON file: `[{"operator": 7, "role": "Operator", "phc": "$argon2id$..."}]`
/// (DN-23 §5, the disconnected desktop's store; GAP-057; **signed by the owner
/// 2026-09-06**).
///
/// Read once at start. An unreadable or malformed file is an unavailable store, which
/// the desktop reports as `SessionState::StoreUnavailable` and starts anyway (rule 5).
/// The file holds PHC strings from [`hash_passphrase`], never a passphrase.
#[derive(Debug)]
pub struct FileAccountStore {
    accounts: Vec<Account>,
    /// Where the list was read from. `account` re-reads it, so a role assigned through
    /// PN-20 (GAP-057) is in force at the next sign-in without rebuilding the authority;
    /// a file that has become unreadable is an unavailable store, never the stale list.
    path: Option<std::path::PathBuf>,
}

impl FileAccountStore {
    /// # Errors
    ///
    /// `SecurityError::AuthenticationUnavailable` naming the reason, when the file
    /// cannot be read or parsed. The reason names the path, never a record.
    pub fn open(path: &std::path::Path) -> Result<Self, SecurityError> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            SecurityError::AuthenticationUnavailable(format!(
                "cannot read the account file {}: {e}",
                path.display()
            ))
        })?;
        let accounts: Vec<Account> = serde_json::from_str(&text).map_err(|e| {
            SecurityError::AuthenticationUnavailable(format!(
                "the account file {} is not a list of accounts: {e}",
                path.display()
            ))
        })?;
        Ok(Self {
            accounts,
            path: Some(path.to_path_buf()),
        })
    }

    /// The accounts as PN-20 lists them: operator and role, never the hash.
    #[must_use]
    pub fn listing(&self) -> Vec<(OperatorId, Role)> {
        self.accounts.iter().map(|a| (a.operator, a.role)).collect()
    }

    /// Change an account's role (GAP-057, PN-20). The passphrase hash is untouched; an
    /// unknown operator is refused rather than created, because creating an account
    /// needs a passphrase nobody has given.
    ///
    /// # Errors
    ///
    /// `SecurityError::UnknownOperator` for an operator the store does not hold.
    pub fn assign_role(&mut self, operator: OperatorId, role: Role) -> Result<(), SecurityError> {
        let account = self
            .accounts
            .iter_mut()
            .find(|a| a.operator == operator)
            .ok_or_else(|| {
                SecurityError::Forbidden(format!(
                    "no account for operator {}: creating one needs a passphrase nobody has given",
                    operator.0
                ))
            })?;
        account.role = role;
        Ok(())
    }

    /// Write the store back where it was read from. The hashes are written as they
    /// were read: PHC strings, never a passphrase.
    ///
    /// # Errors
    ///
    /// `SecurityError::AuthenticationUnavailable` when the file cannot be written.
    pub fn save(&self, path: &std::path::Path) -> Result<(), SecurityError> {
        let text = serde_json::to_string_pretty(&self.accounts).map_err(|e| {
            SecurityError::AuthenticationUnavailable(format!(
                "the account list could not be encoded: {e}"
            ))
        })?;
        std::fs::write(path, text).map_err(|e| {
            SecurityError::AuthenticationUnavailable(format!(
                "cannot write the account file {}: {e}",
                path.display()
            ))
        })
    }
}

impl AccountStore for FileAccountStore {
    fn account(&self, operator: OperatorId) -> Result<Option<Account>, SecurityError> {
        // The file is the truth: a role assigned since this store was opened counts.
        let fresh;
        let accounts = match &self.path {
            Some(path) => {
                fresh = Self::open(path)?;
                &fresh.accounts
            }
            None => &self.accounts,
        };
        Ok(accounts.iter().find(|a| a.operator == operator).cloned())
    }
}

impl AccountStore for InMemoryAccountStore {
    fn account(&self, operator: OperatorId) -> Result<Option<Account>, SecurityError> {
        self.available()?;
        Ok(self
            .accounts
            .iter()
            .find(|a| a.operator == operator)
            .cloned())
    }

    fn available(&self) -> Result<(), SecurityError> {
        match &self.unavailable {
            Some(reason) => Err(SecurityError::AccountStoreUnavailable(reason.clone())),
            None => Ok(()),
        }
    }
}

/// Hash a passphrase for storage.
///
/// Returns a PHC string. The salt is drawn from the operating system's generator, so two
/// operators with the same passphrase have different hashes.
///
/// # Errors
///
/// When the hashing parameters are rejected, which should not happen with the defaults.
pub fn hash_passphrase(passphrase: &str) -> Result<String, SecurityError> {
    use password_hash::{rand_core::OsRng, PasswordHasher, SaltString};
    let salt = SaltString::generate(&mut OsRng);
    argon2::Argon2::default()
        .hash_password(passphrase.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| SecurityError::AuthenticationUnavailable(e.to_string()))
}

/// Verify a passphrase against a store, without holding a session.
///
/// The credential check on its own, so the desktop's stateful authority and the node's
/// per-request one share it rather than each writing their own. Two copies of a
/// passphrase check is two places for the timing behaviour below to diverge.
///
/// # Errors
///
/// [`AuthFailure::Rejected`] for an unknown operator **and** for a wrong passphrase,
/// which are deliberately indistinguishable, and [`AuthFailure::Unavailable`] when the
/// store could not be reached at all.
pub fn verify_account(
    store: &dyn AccountStore,
    operator: OperatorId,
    passphrase: &str,
) -> Result<Account, AuthFailure> {
    use password_hash::{PasswordHash, PasswordVerifier};

    let account = store
        .account(operator)
        .map_err(|_| AuthFailure::Unavailable)?;

    // Work is done even when the account is unknown, so an attacker timing the response
    // cannot learn which operator identifiers exist -- which is what one `Rejected`
    // variant exists to prevent.
    let verified = if let Some(account) = &account {
        PasswordHash::new(&account.phc).is_ok_and(|parsed| {
            argon2::Argon2::default()
                .verify_password(passphrase.as_bytes(), &parsed)
                .is_ok()
        })
    } else {
        let _ = hash_passphrase(passphrase);
        false
    };

    match (verified, account) {
        (true, Some(account)) => Ok(account),
        _ => Err(AuthFailure::Rejected),
    }
}

/// The disconnected desktop's authority: local accounts (D-02's fallback).
///
/// Sessions here may be long-lived, which is the one profile DN-23 §5 allows that for: a
/// disconnected console has nobody to re-issue a token, and an operator locked out
/// mid-engagement is the failure this design keeps choosing against.
pub struct LocalAccountAuthority {
    store: Box<dyn AccountStore>,
    state: SessionState,
    back_off: BackOff,
    /// Consecutive failures and when the last one was, per operator.
    failures: std::collections::BTreeMap<OperatorId, (u32, MissionTimeSeconds)>,
    /// Seconds a session lasts, or `None` for the disconnected profile's non-expiring
    /// session (DN-23 §5). The baseline's `security.authentication.session_lifetime_s`.
    lifetime_s: Option<MissionTimeSeconds>,
}

impl std::fmt::Debug for LocalAccountAuthority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalAccountAuthority")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl LocalAccountAuthority {
    /// Build an authority, asking the store whether it is there.
    ///
    /// The probe is why a desktop can report `StoreUnavailable` in its health summary
    /// before anybody has tried to sign in, which is what DN-23 §5 rule 5 asks for.
    #[must_use]
    pub fn new(store: Box<dyn AccountStore>) -> Self {
        let state = match store.available() {
            Ok(()) => SessionState::NobodySignedIn,
            Err(err) => SessionState::StoreUnavailable {
                reason: err.to_string(),
            },
        };
        Self {
            store,
            state,
            back_off: BackOff::default(),
            failures: std::collections::BTreeMap::new(),
            lifetime_s: None,
        }
    }

    #[must_use]
    pub fn with_back_off(mut self, back_off: BackOff) -> Self {
        self.back_off = back_off;
        self
    }

    /// Sessions expire `lifetime_s` after they are established (GAP-057). `None` keeps
    /// the non-expiring session only the disconnected profile may issue.
    #[must_use]
    pub fn with_lifetime(mut self, lifetime_s: Option<MissionTimeSeconds>) -> Self {
        self.lifetime_s = lifetime_s.filter(|l| l.is_finite() && *l > 0.0);
        self
    }

    /// Encode a credential for [`SessionAuthority::sign_in`].
    ///
    /// `<operator id>:<passphrase>`. The identifier is decimal and the passphrase is
    /// everything after the first colon, so a passphrase may contain one.
    #[must_use]
    pub fn credential(operator: OperatorId, passphrase: &str) -> Vec<u8> {
        format!("{}:{passphrase}", operator.0).into_bytes()
    }

    fn parse(credential: &[u8]) -> Option<(OperatorId, String)> {
        let text = std::str::from_utf8(credential).ok()?;
        let (id, passphrase) = text.split_once(':')?;
        Some((OperatorId(id.parse().ok()?), passphrase.to_owned()))
    }

    /// The delay still owed by this operator, if any.
    fn owed(&self, operator: OperatorId, now: MissionTimeSeconds) -> Option<MissionTimeSeconds> {
        let (failures, last) = self.failures.get(&operator)?;
        let delay = self.back_off.delay_after(*failures);
        let elapsed = now - last;
        (elapsed < delay).then_some(delay - elapsed)
    }
}

impl SessionAuthority for LocalAccountAuthority {
    fn sign_in(
        &mut self,
        credential: &[u8],
        now: MissionTimeSeconds,
    ) -> Result<OperatorSession, AuthFailure> {
        let Some((operator, passphrase)) = Self::parse(credential) else {
            // A malformed credential is a rejection like any other. Saying it was
            // malformed would tell a prober the encoding.
            return Err(AuthFailure::Rejected);
        };

        if let Some(retry_after_s) = self.owed(operator, now) {
            return Err(AuthFailure::TooFast { retry_after_s });
        }

        match verify_account(self.store.as_ref(), operator, &passphrase) {
            Ok(account) => {
                self.failures.remove(&operator);
                let session = OperatorSession {
                    operator,
                    role: account.role,
                    established: now,
                    // The disconnected profile's session does not expire unless the
                    // baseline says how long it lasts; the node's always does, and that
                    // is minted elsewhere.
                    expires: self.lifetime_s.map(|l| now + l),
                };
                self.state = SessionState::SignedIn(session.clone());
                Ok(session)
            }
            Err(AuthFailure::Unavailable) => {
                // The store is a fault, not a rejection, and the state says so: a
                // desktop whose keystore is missing must not look like one whose
                // operator typed the wrong passphrase. The store's own words are kept,
                // because "the keystore is locked" and "the file is missing" send an
                // administrator to different places.
                self.state = SessionState::StoreUnavailable {
                    reason: self.store.available().err().map_or_else(
                        || "the account store could not be reached".into(),
                        |e| e.to_string(),
                    ),
                };
                Err(AuthFailure::Unavailable)
            }
            Err(failure) => {
                let entry = self.failures.entry(operator).or_insert((0, now));
                entry.0 = entry.0.saturating_add(1);
                entry.1 = now;
                Err(failure)
            }
        }
    }

    fn sign_out(&mut self, _now: MissionTimeSeconds) {
        self.state = SessionState::NobodySignedIn;
    }

    fn state(&self, now: MissionTimeSeconds) -> SessionState {
        match &self.state {
            SessionState::SignedIn(session) if !session.is_valid_at(now) => SessionState::Expired {
                operator: session.operator,
                at: session.expires.unwrap_or(now),
            },
            other => other.clone(),
        }
    }
}

/// Write one authentication attempt to the audit log.
///
/// Every attempt, successful or not (DN-23 §5 rule 7). Key *use* is deliberately not
/// audited per operation (DN-22 §5); attempts are, because there are few of them and each
/// one matters.
pub fn audit_attempt(
    log: &mut dyn AuditLog,
    operator: Option<OperatorId>,
    outcome: Result<(), AuthFailure>,
    now: MissionTimeSeconds,
) {
    let (action, detail) = match outcome {
        Ok(()) => ("session.sign_in", "signed in".to_owned()),
        Err(failure) => ("session.rejected", failure.to_string()),
    };
    log.record(AuditEntry {
        operator,
        action: action.to_owned(),
        mission_time: now,
        detail,
    });
}

/// Write a sign-out to the audit log.
pub fn audit_sign_out(log: &mut dyn AuditLog, operator: OperatorId, now: MissionTimeSeconds) {
    log.record(AuditEntry {
        operator: Some(operator),
        action: "session.sign_out".to_owned(),
        mission_time: now,
        detail: "signed out".to_owned(),
    });
}

#[cfg(test)]
// The back-off schedule is exact powers of two from an exact base, so these
// comparisons are exact by construction rather than by luck.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::InMemoryAuditLog;

    fn store() -> InMemoryAccountStore {
        InMemoryAccountStore::new(vec![Account {
            operator: OperatorId(7),
            role: Role::SensorManager,
            phc: hash_passphrase("correct horse battery staple").expect("hashed"),
        }])
    }

    fn local() -> LocalAccountAuthority {
        LocalAccountAuthority::new(Box::new(store()))
    }

    /// The passphrase is never stored. A store holding one would make every other
    /// property here beside the point.
    #[test]
    fn the_store_holds_a_hash_and_never_the_passphrase() {
        let account = &store().accounts[0];
        assert!(!account.phc.contains("correct horse"));
        assert!(account.phc.starts_with("$argon2"), "{}", account.phc);
    }

    /// The same passphrase hashes differently for two operators, so a store does not
    /// reveal that two people chose the same one.
    #[test]
    fn the_same_passphrase_hashes_differently() {
        let a = hash_passphrase("same").expect("hashed");
        let b = hash_passphrase("same").expect("hashed");
        assert_ne!(a, b);
    }

    #[test]
    fn a_correct_credential_establishes_a_session() {
        let mut authority = local();
        let session = authority
            .sign_in(
                &LocalAccountAuthority::credential(OperatorId(7), "correct horse battery staple"),
                10.0,
            )
            .expect("signed in");
        assert_eq!(session.operator, OperatorId(7));
        assert_eq!(session.role, Role::SensorManager);
        assert_eq!(authority.state(10.0).operator(), Some(OperatorId(7)));
    }

    /// The property the whole module exists for: nothing produces a session without a
    /// verification. An unknown operator and a wrong passphrase both fail, and fail
    /// *identically*, so a prober cannot enumerate operator identifiers.
    #[test]
    fn an_unknown_operator_and_a_bad_passphrase_are_indistinguishable() {
        let mut authority = local();
        let unknown = authority.sign_in(
            &LocalAccountAuthority::credential(OperatorId(999), "anything"),
            10.0,
        );
        let mut other = local();
        let wrong = other.sign_in(
            &LocalAccountAuthority::credential(OperatorId(7), "not the passphrase"),
            10.0,
        );
        assert_eq!(unknown, Err(AuthFailure::Rejected));
        assert_eq!(wrong, Err(AuthFailure::Rejected));
        assert_eq!(unknown, wrong, "the two failures can be told apart");
        assert_eq!(authority.state(10.0), SessionState::NobodySignedIn);
    }

    /// A malformed credential is rejected without saying it was malformed.
    #[test]
    fn a_malformed_credential_is_just_a_rejection() {
        let mut authority = local();
        assert_eq!(
            authority.sign_in(b"no colon here", 1.0),
            Err(AuthFailure::Rejected)
        );
        assert_eq!(
            authority.sign_in(&[0xff, 0xfe], 1.0),
            Err(AuthFailure::Rejected)
        );
    }

    /// Failures back off and never lock out: after waiting, the operator gets in.
    /// An operator locked out of a console mid-engagement is the worse failure.
    #[test]
    fn failures_back_off_and_never_lock_out() {
        let mut authority = local().with_back_off(BackOff {
            free_attempts: 1,
            base_s: 2.0,
            max_s: 8.0,
        });
        let bad = LocalAccountAuthority::credential(OperatorId(7), "wrong");
        let good = LocalAccountAuthority::credential(OperatorId(7), "correct horse battery staple");

        assert_eq!(authority.sign_in(&bad, 0.0), Err(AuthFailure::Rejected));
        // Second failure is past the free attempts, so the third attempt is delayed.
        assert_eq!(authority.sign_in(&bad, 0.1), Err(AuthFailure::Rejected));
        match authority.sign_in(&good, 0.2) {
            Err(AuthFailure::TooFast { retry_after_s }) => assert!(retry_after_s > 0.0),
            other => panic!("expected a back-off, got {other:?}"),
        }
        // Waiting it out works. This is the assertion that distinguishes a back-off
        // from a lockout.
        authority
            .sign_in(&good, 100.0)
            .expect("the correct passphrase gets in after the delay");
    }

    /// A successful sign-in clears the record of failures, so an operator who mistyped
    /// once is not delayed on their next session.
    #[test]
    fn success_clears_the_back_off() {
        let mut authority = local();
        let bad = LocalAccountAuthority::credential(OperatorId(7), "wrong");
        let good = LocalAccountAuthority::credential(OperatorId(7), "correct horse battery staple");
        assert!(authority.sign_in(&bad, 0.0).is_err());
        authority.sign_in(&good, 1.0).expect("signed in");
        assert!(authority.owed(OperatorId(7), 1.0).is_none());
    }

    /// The store is asked whether it is there at construction, so a console reports the
    /// fault before anybody tries to sign in. Reporting `NobodySignedIn` about a desktop
    /// where nobody *can* sign in would put the wrong sentence in the health summary.
    #[test]
    fn an_unavailable_store_is_reported_before_anyone_tries() {
        let authority = LocalAccountAuthority::new(Box::new(InMemoryAccountStore::unavailable(
            "the keystore is locked",
        )));
        assert!(
            authority.state(0.0).is_fault(),
            "a console with no account store reported an ordinary absence"
        );
        // And a store that is present reports the ordinary absence, not a fault.
        assert_eq!(local().state(0.0), SessionState::NobodySignedIn);
    }

    /// An unreachable store is a fault, not a rejection, and the state says which.
    /// A desktop whose keystore is missing must not look like one whose operator
    /// mistyped.
    #[test]
    fn an_unavailable_store_is_a_fault_and_not_a_rejection() {
        let mut authority = LocalAccountAuthority::new(Box::new(
            InMemoryAccountStore::unavailable("the keystore is locked"),
        ));
        let outcome = authority.sign_in(
            &LocalAccountAuthority::credential(OperatorId(7), "anything"),
            1.0,
        );
        assert_eq!(outcome, Err(AuthFailure::Unavailable));
        match authority.state(1.0) {
            SessionState::StoreUnavailable { reason } => {
                assert!(reason.contains("keystore"), "{reason}");
            }
            other => panic!("expected an unavailable store, got {other:?}"),
        }
        assert!(authority.state(1.0).is_fault());
        assert!(!SessionState::NobodySignedIn.is_fault());
    }

    /// A configured lifetime expires a local session on the clock, and after it nothing
    /// is attributed (GAP-057, DN-23 §5).
    #[test]
    fn a_configured_lifetime_expires_a_local_session() {
        let store = InMemoryAccountStore::new(vec![Account {
            operator: OperatorId(7),
            role: Role::Operator,
            phc: hash_passphrase("correct horse").expect("hashed"),
        }]);
        let mut authority = LocalAccountAuthority::new(Box::new(store)).with_lifetime(Some(600.0));
        let session = authority
            .sign_in(
                &LocalAccountAuthority::credential(OperatorId(7), "correct horse"),
                100.0,
            )
            .expect("signs in");
        assert_eq!(session.expires, Some(700.0));
        assert!(matches!(authority.state(699.0), SessionState::SignedIn(_)));
        match authority.state(700.0) {
            SessionState::Expired { operator, at } => {
                assert_eq!(operator, OperatorId(7));
                assert_eq!(at, 700.0);
            }
            other => panic!("expected an expired session, got {other:?}"),
        }
        assert_eq!(authority.state(700.0).operator(), None);
        // A non-positive lifetime is not a lifetime.
        let forever = LocalAccountAuthority::new(Box::new(InMemoryAccountStore::new(vec![])))
            .with_lifetime(Some(0.0));
        assert!(forever.lifetime_s.is_none());
    }

    /// Nobody signed in and a session that expired are different states, and neither
    /// attributes an act.
    #[test]
    fn an_expired_session_is_not_the_same_as_nobody_signed_in() {
        let expired = SessionState::Expired {
            operator: OperatorId(7),
            at: 50.0,
        };
        assert_eq!(
            expired.operator(),
            None,
            "an expired session attributed an act"
        );
        assert_eq!(SessionState::NobodySignedIn.operator(), None);
        assert_ne!(expired, SessionState::NobodySignedIn);
    }

    /// A session with an expiry stops being valid on the clock, without anything
    /// touching it.
    #[test]
    fn an_expiring_session_lapses_on_the_clock() {
        let session = OperatorSession {
            operator: OperatorId(7),
            role: Role::Operator,
            established: 0.0,
            expires: Some(60.0),
        };
        assert!(session.is_valid_at(59.0));
        assert!(!session.is_valid_at(60.0));
        assert_eq!(session.remaining(50.0), Some(10.0));

        // A session with no expiry is preserved indefinitely, which is a decision and
        // not a gap -- only the disconnected profile may issue one.
        let forever = OperatorSession {
            expires: None,
            ..session
        };
        assert!(forever.is_valid_at(1_000_000.0));
        assert_eq!(forever.remaining(0.0), None);
    }

    #[test]
    fn signing_out_leaves_nobody_signed_in() {
        let mut authority = local();
        authority
            .sign_in(
                &LocalAccountAuthority::credential(OperatorId(7), "correct horse battery staple"),
                1.0,
            )
            .expect("signed in");
        authority.sign_out(2.0);
        assert_eq!(authority.state(2.0), SessionState::NobodySignedIn);
        assert_eq!(authority.state(2.0).operator(), None);
    }

    /// Every attempt is audited, successful or not.
    #[test]
    fn both_outcomes_reach_the_audit_log() {
        let mut log = InMemoryAuditLog::new();
        audit_attempt(&mut log, Some(OperatorId(7)), Ok(()), 1.0);
        audit_attempt(
            &mut log,
            Some(OperatorId(7)),
            Err(AuthFailure::Rejected),
            2.0,
        );
        audit_sign_out(&mut log, OperatorId(7), 3.0);
        assert_eq!(log.entries().len(), 3);
        assert_eq!(log.entries()[0].action, "session.sign_in");
        assert_eq!(log.entries()[1].action, "session.rejected");
        // The failure's detail must not say which half was wrong, on the log either.
        assert!(!log.entries()[1].detail.contains("unknown"));
        assert_eq!(log.entries()[2].action, "session.sign_out");
    }

    /// The back-off grows and is capped, and never returns zero once past the free
    /// attempts -- a wrap to zero would silently remove the protection.
    #[test]
    fn the_back_off_grows_and_is_capped() {
        let back_off = BackOff {
            free_attempts: 2,
            base_s: 1.0,
            max_s: 8.0,
        };
        assert_eq!(back_off.delay_after(1), 0.0);
        assert_eq!(back_off.delay_after(2), 0.0);
        assert_eq!(back_off.delay_after(3), 1.0);
        assert_eq!(back_off.delay_after(4), 2.0);
        assert_eq!(back_off.delay_after(5), 4.0);
        assert_eq!(back_off.delay_after(6), 8.0);
        assert_eq!(
            back_off.delay_after(1_000),
            8.0,
            "the delay wrapped or overflowed"
        );
    }
}

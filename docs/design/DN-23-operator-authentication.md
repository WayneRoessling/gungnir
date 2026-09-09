# DN-23 Operator authentication and sessions

Closes GAP-057. Status: **signed off by the owner 2026-09-05**, with D-20 (the crates)
settled the same day. **Human-owned**: `gungnir-security` is a low-trust crate and this note
decides who the system believes you are.

**Implementation status, 2026-09-05.** The desktop half is built: §3's session types,
§5's rules 1 to 7, and the local-account authority D-02 makes the disconnected fallback.
`gungnir-security/src/session.rs` and `gungnir-app/tests/authentication.rs`.

**The node half followed the same day.** §5's node-issued token is
`gungnir-security/src/token.rs`: HMAC-SHA256 over JSON claims, constant-time comparison,
and an expiry that is never renewed on use. §6's `POST /v2/session` and `GET /v2/session`
are served, and **every other v2 route now requires the token** -- a caller is whoever the
token says, and `ApprovalRequest`'s own `operator` field is not believed.

`POST /v2/detections` is served for an authenticated caller. It **queues** the detection
for the ingest gateway rather than accepting it, so the submission is authenticated
against the sensor allow-list and validated by exactly the code a sensor's own feed goes
through; the node registers a `ProtocolAdapter` that polls the queue. `202` says taken,
not believed. A route that reached past the gateway would be a second way in with no
checks on it, and `gungnir-ingest` is the trust boundary for external data.

**One finding this half turned up.** `POST /v2/plans/{plan_id}/decision` still refuses,
and no longer for want of a caller: **a node runs no approval queue.** The desktop routes
plans through the policy chain and the queue (GAP-038); a node publishes `PlanProposed`
and stops. Serving the route would mean inventing a queue in a request handler, putting
the recommend-versus-act boundary in the transport. It returns `501` naming that, which
is now the true reason.

**A second consequence, in the desktop.** Connecting to a node is an authenticated act
now, and nobody is signed in while `AppState` is being constructed -- so a deployment
configured for a remote backend comes up embedded and says why, rather than connecting
with a credential it would have had to invent. Establishing the link after an operator
signs in is the remaining wiring.

What is **not** built: the machine-identity row of §5, which is GAP-060; §7's PN-20, which
additionally needs GAP-059; an account store for a node, which has no keystore integration
so a node with a signing key configured still authenticates nobody and says so; and the
desktop's connect-after-sign-in.

**One correction the implementation made to §5.** Rule 5 says a desktop whose account
store is unavailable comes up saying so. The first implementation only discovered an
unavailable store when somebody *tried* to sign in, so a console where nobody **could**
sign in reported `NobodySignedIn` -- an ordinary absence -- in its health summary. Caught
by `gungnir-app/tests/authentication.rs`. `AccountStore` gained an `available()` probe
that `LocalAccountAuthority::new` calls, so the state is right before anybody tries.

## 1. The gap and the thread steps it blocks

`gungnir_security::Authenticator` is a trait with **no implementation**. The authorization
half is real -- `Authorizer`, `role_permits`, the action names, the role-to-action matrix --
and it is never reached, because nothing turns a credential into an `OperatorId`.

The consequence is visible in three places already built, each of which had to invent a way
to say "we do not know who did this":

- `gungnir_command::DecisionRecord::operator_id` is `None` on every decision, and PN-07
  says so on screen (GAP-038).
- `gungnir_model::Concurrence::UnattributedRole` exists **only** because a concurrence must
  carry an operator and none can be produced (GAP-005, DN-11 amendment 1 b).
- Both v2 write paths return `501` rather than accept a request whose operator is asserted
  in its own body (GAP-041).

So MT-08's concurrence and MT-10's remote decision are both recorded without an actor, and
CAP-6.1 is a trait. This note answers what a credential is, per profile, and what a session
is once one has been verified.

## 2. The owning component

`gungnir-security`, which owns authentication, authorization, audit, and (since DN-22) key
custody. It gains session types and one `Authenticator` implementation per profile.

The binaries own the store, as they own the `KeyProvider`: custody belongs to the host.

## 3. Types

In `gungnir-security`:

```rust
/// A verified operator, and how long the system will keep believing it.
///
/// Held only after `Authenticator::authenticate` succeeded. There is no constructor
/// that takes a bare `OperatorId`, so a session cannot be forged by a caller that
/// merely knows an identifier -- the same reason `Handoff` has no constructor taking
/// a bare plan (DN-07).
#[derive(Debug, Clone, PartialEq)]
pub struct OperatorSession {
    pub operator: OperatorId,
    pub role: Role,
    pub established: MissionTimeSeconds,
    /// Absent means a session that does not expire, which **only** the disconnected
    /// desktop's local-account profile may issue. A node-issued session always expires.
    pub expires: Option<MissionTimeSeconds>,
}

impl OperatorSession {
    pub fn is_valid_at(&self, now: MissionTimeSeconds) -> bool;
}

/// Who is signed in, or why nobody is.
///
/// Not `Option<OperatorSession>`. "Nobody has signed in", "the session expired" and
/// "the account store could not be reached" are three different facts, and PN-01 and
/// PN-07 have to tell them apart -- the same argument `EmptyBecause` settles for PN-06.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionState {
    SignedIn(OperatorSession),
    NobodySignedIn,
    Expired { operator: OperatorId, at: MissionTimeSeconds },
    /// The store is unreachable, so nobody *can* sign in. Distinct from nobody having
    /// tried: this one is a fault and belongs in the health summary.
    StoreUnavailable { reason: String },
}

/// Why an attempt failed. Never carries which half was wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthFailure {
    /// The identifier is unknown, or the secret did not match. **One variant on
    /// purpose**: distinguishing them tells an attacker which operator identifiers
    /// exist.
    Rejected,
    /// Too many attempts too quickly; try again after the delay.
    TooFast { retry_after_s: f64 },
    Unavailable,
}
```

`Authenticator` keeps its shape and gains a sibling, because a credential check and a
session are different acts and only the second one has a clock:

```rust
pub trait SessionAuthority: Send + Sync {
    fn sign_in(&mut self, credential: &[u8], now: MissionTimeSeconds)
        -> Result<OperatorSession, AuthFailure>;
    fn sign_out(&mut self, session: &OperatorSession, now: MissionTimeSeconds);
    fn state(&self, now: MissionTimeSeconds) -> SessionState;
}
```

## 4. Edges

**None.** `gungnir-security` already sits below everything that needs it. The binaries
construct the authority and pass `&dyn SessionAuthority` in, as they do the journal and
(per DN-22) the key provider.

## 5. Behaviour

**One mechanism per profile, which D-02 already chose:**

| Profile | Credential | Verified by | Session |
|---|---|---|---|
| Disconnected desktop | Local account: an operator identifier and a passphrase | The desktop, against the local account store | May be long-lived; ends at sign-out or shutdown |
| Connected desktop | A short-lived token the node issues after a local or identity-provider login | The node, on every request | Always expires; renewed by signing in again |
| Machine: node, peer, sensor | Mutual TLS client certificate | The TLS layer, which is GAP-060 | Per connection |

The rules that keep it honest:

1. **Authentication never invents attribution.** Until a session exists, records keep saying
   nobody was signed in: `DecisionRecord::operator_id` stays `None` and
   `Concurrence::UnattributedRole` is what a concurrence carries. The types that exist to
   express "we do not know who" are not removed by this note -- they become *reachable but
   unused* in a deployment that authenticates, and they remain correct for one that does
   not. **A system that filled them in with a role name once login existed would be worse
   than the one that admits it does not know.**

2. **A session that has expired refuses; it does not silently renew.** Renewal on use makes
   "short-lived" meaningless. An operator whose session expires mid-decision is told, and
   the decision is not recorded under the expired session -- it is not recorded at all, and
   PN-07 keeps what was typed so it can be submitted again after signing in.

3. **Failure is rate-limited, never a hard lockout by default.** An operator locked out of a
   command-and-control console during an engagement is a worse outcome than a slow
   brute-force attempt against a console that is already inside a defended network. Failed
   attempts back off (`AuthFailure::TooFast`) and every one is audited. A deployment that
   needs a hard lockout configures it deliberately; the default does not.

4. **A failure never says which half was wrong.** `AuthFailure::Rejected` covers both an
   unknown operator and a bad secret, because telling them apart enumerates the operators.

5. **The disconnected fallback: the desktop starts.** If the account store is unavailable,
   the desktop comes up in `SessionState::StoreUnavailable`, says so in the status strip and
   the health summary, and behaves exactly as it does today -- role-selected, unauthenticated,
   nothing attributed. It does **not** refuse to run. This is DN-22's journal-encryption
   fallback applied to identity, and for the same reason: a console that will not start
   because a credential store is missing is a worse failure than one that runs and says what
   it cannot do.

6. **No credential material in the configuration baseline.** The baseline names the provider
   and its parameters; account records live in the profile's own store. Validation rejects a
   value that looks like a secret, as DN-22 §6 already requires for keys.

7. **Every attempt is audited, successful or not.** Sign-in, sign-out, failure, and expiry
   are `AuditEntry` rows. Key *use* is deliberately not audited per operation (DN-22 §5);
   authentication attempts are, because there are few of them and each one matters.

## 6. Configuration and interface delta

`ConfigBaseline.security.authentication: AuthenticationConfig` -- the provider, the session
lifetime, and the back-off schedule. No credential material, checked by validation.

Interface:

- `POST /v2/session` with a credential, returning a short-lived token and its expiry.
  Authorization: none, since this is what establishes identity. This is the one v2 route
  that may be reached unauthenticated.
- Every other v2 route requires the token, including the two write paths GAP-041 left
  returning `501`. Serving them is the second half of this gap.
- `GET /v2/session` returns the caller's own `SessionState`, so a desktop can tell an
  expired session from an unreachable node.

## 7. User-interface delta

| Panel | Change |
|---|---|
| PN-20 Audit and accounts | Sign-in and sign-out, the account list with roles, and the audit log including failed attempts. Blocked on this gap and GAP-059 |
| PN-01 Status strip | Who is signed in, or which of the three reasons nobody is |
| PN-07 Decision dialog | The attribution line stops saying nobody is signed in once somebody is; unchanged otherwise, since it already reads the session state |
| PN-15 Requirements | The concurrence carries `Concurrence::Operator` instead of `UnattributedRole` |

## 8. Verification

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-6.1 Authentication | Unit tests against a stub account store that can accept, reject, or be unavailable | A rejected credential yields one indistinguishable failure for an unknown operator and a bad secret; an expired session refuses and is never renewed by use; an unavailable store yields `StoreUnavailable` and the desktop still starts; every attempt appears in the audit log; no code path constructs an `OperatorSession` without a successful verification | Stub account store; generated credentials |
| CAP-6.1 Attribution end to end | MT-08 replay with a signed-in operator and without one | With a session, a concurrence carries `Concurrence::Operator` and a decision names an operator; without one, both record that nobody was signed in and neither invents a name | TT-08 sample set |

The second row is what finally gates DN-11's CAP-2.12 criterion, which asks for a
concurrence carrying an operator and which no build can satisfy today.

## 9. The open decision this note cannot make: D-20

**No cryptographic crate is in the workspace**, and every mechanism above needs one:
passphrase verification needs a memory-hard key-derivation function, and a short-lived token
needs a signature or a message authentication code. §2.9 has no row for either, and D-02
chose the *mechanism* without choosing the *libraries* -- exactly the gap D-18 filled for the
transport before GAP-041 could be written.

The recommendation, for the owner and the security engineer to accept or replace:

| Need | Proposed | Why |
|---|---|---|
| Passphrase verification | `argon2` (with `password-hash`) | The current password-hashing competition winner and the OWASP default; memory-hard, so a stolen store is expensive to attack offline. Pure Rust, no system library |
| Token integrity | `hmac` + `sha2` | A node verifying its own tokens needs no public-key scheme, and a symmetric MAC keeps the secret inside DN-22's `seal`/`unseal` boundary. Public-key signing would need the private key in the process, which is the same conflict GAP-060 hit |
| Constant-time comparison | `subtle` | Already a transitive dependency of both; named directly so comparisons are obviously constant-time rather than incidentally so |

**`hmac` over `ed25519` is the load-bearing choice here**, and it is worth arguing rather
than assuming: a signed token that a *third party* must verify needs public-key signing, and
a token only its issuer verifies does not. D-02 says the node issues and the node verifies.
Choosing public-key now would force the same private-key-in-process problem that stopped TLS
in GAP-041, for no benefit this deployment can use. If peer C2 systems are ever to verify a
Gungnir operator token themselves, that is a different decision and should be taken then.

Whatever is chosen must be one sign-off recorded in §2.9, with the duplicate-linkage check
D-18's rows carry.

## 10. Amendment 1 (2026-09-07): the account nobody could create

**Signed by the owner 2026-09-07.**

This note gives the account file a format, gives `FileAccountStore` a file to read, and
gives the node a reason to refuse callers when it has neither. What it never said is
**who writes the file**.

`hash_passphrase` produced the `phc` string §5 requires. Until 2026-09-07 it was called
from tests in six crates and **from nowhere else** -- no binary, no example, no
documented command. So a deployment following this note arrived at a node that warned
`no caller authority` and had no way forward that did not involve writing Rust. The
authentication half of GAP-057 was implemented and signed on 2026-09-06 and was, in the
only sense that matters to a deployment, unreachable.

That is the failure mode this workspace calls a silent stub seen from the other side:
nothing claimed to work that did not. The node's warning was accurate the whole time.
What was missing was the other end of the sentence.

**`gungnir-node account add <file> <operator-id> <role> [--replace]`**, and
`account list <file>`. Five rules, each of which is a refusal rather than a default:

1. **The passphrase is read from standard input, never from an argument.** Arguments are
   visible to every process on the host through the process table; a passphrase that
   reaches argv is disclosed before it is hashed.
2. **A duplicate operator is refused** unless `--replace` is passed, and the refusal says
   so. Silently overwriting a credential is how an account is taken over, not how one is
   provisioned.
3. **A file that exists and does not parse is refused, not treated as empty.** Reading a
   corrupt account file as "no accounts" would discard every account in it on the next
   write.
4. **An empty passphrase is refused and no file is created.**
5. **A listing shows operator and role and never the hash**, which is the same rule PN-20
   already follows.

**Two limitations are stated rather than hidden.** Echo of a typed passphrase is not
suppressed, because that needs a terminal crate no §2.9 decision admits; the documented
usage pipes the passphrase in. And the file is set to owner-only on Unix while on Windows
it inherits the directory's permissions -- the command says which of those happened
instead of implying the stricter one.

**What this does not do.** It does not remove an account, because the audit consequences
of deleting a credential belong with the audit trail and PN-20's listing, and inventing
them here would be a decision this note has not taken. It does not rotate the token
signing key, which is `GUNGNIR_TOKEN_KEY` and DN-22's business. It does not create the
first account automatically at start-up: a node that provisions itself is a node with a
default credential.

## 11. Amendment 2 (2026-09-08, **signed by the owner the same day**): the operating system's keystore for a node's own accounts

**Human-owned; signed by the owner 2026-09-08, together with DN-22 §13's OS-keystore
mechanism and item 111's TLS-identity generalisation -- one review over the whole
mechanism and its four services.** The owner's review found and closed a first-run race
in `os_keystore::ensure_secret` before signing (DN-22 §13 records the fix); this
amendment shares that helper and the fix applies here too.

Amendment 1 gave the node a way to write its account file; that file is still plaintext
JSON with argon2-hashed passphrases, protected only by whatever the platform's file
permissions grant -- owner-only on Unix, nothing on Windows, per
[`permissions_note`](../../gungnir-node/src/account.rs). DN-22 amendment 4 admitted
`keyring` and used it to seal the desktop's own key material; this amendment reaches the
same crate for a node's account list instead, closing the remaining item both DN-22
amendment 4 and `ARCHITECTURE.md` item 103 named and left.

**One mechanism, two payloads, no shared constructor.** `gungnir_security::
EncryptedAccountStore` seals a `Vec<Account>` in one file the same shape
`PersistentKeyProvider`'s already does -- AES-256-GCM, a key argon2 derives from a
string, that string from `os_keystore::wrapping_secret` -- and shares nothing else with
it, because a P-256 key snapshot and an account list are typed for different things and
forcing one through the other's constructor would be the wrong kind of reuse.
`wrapping_secret` gained a `service` parameter for this: a node's entries live under
`gungnir-node-accounts`, distinct from the desktop's `gungnir-desktop-keystore`, so the
two never collide when both run on one machine.

**Answering item 103's objection rather than ignoring it.** DN-22 amendment 4 assigned
the OS-keystore key-provider row to the desktop alone because a node has no operator
login to unlock it at. That objection is about *whose login* unlocks an entry meant to
be unlocked at one; this amendment is not that mechanism reused for a second purpose, it
is the same crate answering a different question -- whether a sealed file beats a
plaintext one for accounts a node keeps somewhere regardless. A node's own process
identity (a Windows service account's own Credential Manager, a Linux keyring a systemd
unit has been given access to) can hold an entry with nobody logged in to anything, which
is a narrower, real claim rather than a reuse of the one amendment 4 declined. Where no
such facility is reachable -- a headless Linux host with no Secret Service session on
its bus, the same case amendment 4's own tests document -- `wrapping_secret`'s error
path fires and the node reports `SecurityError::AccountStoreUnavailable`, which is
§5 rule 5's fallback and not a gap in coverage.

**`AuthenticationProvider::OsKeystoreAccounts { account }`** joins `LocalAccounts` in
the baseline, named and validated the same way (§5 rule 6: an account, never a secret;
an empty name or a value shaped like a PHC string is refused). `gungnir-node account
add-os-keystore <data-dir> <keystore-account> <operator-id> <role> [--replace]` and
`account list-os-keystore <data-dir> <keystore-account>` provision it -- the same five
rules amendment 1 lists, reached against the sealed store instead of the plaintext file.

**The desktop was given a fourth arm, not a `todo!()`.** `gungnir-app`'s two
account-provider matches (`account_listing`, `build_session_authority`) had to cover
the new variant to keep compiling; both report "this provider is for gungnir-node" and
fall back to `SessionState::StoreUnavailable`; the desktop's own row is unchanged, since
nothing asked it to gain a second account-storage mechanism alongside `LocalAccounts`.

## Traceability

GAP-057; CAP-6.1, and CAP-6.2/CAP-6.3 through PN-20; D-02 for the mechanism, D-20 for the
crates; unblocks the CAP-2.12 criterion in DN-11 §8 and the write paths GAP-041 left
refusing; depends on GAP-060 for the machine-identity row and on GAP-059 for the account
list; `../ux/wireframes/WF-20-audit.puml`; principles AP-02, AP-03.

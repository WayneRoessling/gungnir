# DN-22 Key custody, rotation, and escrow

Closes GAP-084, filed by plan 11 finding F-3. Status: **signed off by the owner 2026-09-05**, with **amendment 1 (§9) signed the same day** after GAP-060 found the note unusable as written: no way to obtain a TLS identity, no algorithm behind `seal`, and no way to test either. **Amendment 2 (§11), signed by the owner 2026-09-06**: who holds the escrow key, which §10 left open (D-27). **Amendment 3 (§12), signed by the owner 2026-09-06**: a passphrase-sealed keystore as the disconnected profile's persistent custody until a §2.9 decision admits an OS-keystore crate. **Amendment 4 (§13), 2026-09-08, signed by the owner the same day**: that decision taken (D-39) and the OS keystore built as amendment 3's sibling, unlocked at operator login rather than typed at sign-in. The owner's review found a first-run race in the secret-generation helper before signing; §13 records the fix that closed it. **Amendment 5 (§14), 2026-09-08, written and gated, not signed**: `ManagedService`, the third and last row of §5's table, designed at last -- envelope encryption because the journal budget forbids a network round trip per envelope, signing left in the service because it is not on a per-frame path, and the one place `may_destroy` cannot reach said plainly rather than papered over.
**Human-owned and signed**: `gungnir-security` is a low-trust crate and this note decides
who can read what. The owner signed it on 2026-09-05.

## 1. The gap and the thread step it blocks

`ARCHITECTURE.md` §8.5 states the protection intent and D-02 fixed the credential
mechanism. Between those two there is nothing: no component owns key material. Where keys
live per profile, who may read them, how they rotate, what happens to a journal encrypted
under a retired key, and how a disconnected desktop holds its own are all unanswered.

GAP-060 cannot be implemented without answering them, and answering them at coding time
means inventing a custody model in a pull request. An accreditor asks about custody before
they ask about ciphers.

## 2. The owning component

`gungnir-security`, which already owns authentication, authorization, and audit. It gains
a key-provider trait and no key material of its own.

`gungnir-store` consumes the provider for journal encryption; the transport consumes it for
its credentials. Both already depend on what they need or will when the transport lands.

## 3. Types

In `gungnir-security`:

```rust
/// What a key is for. Separate purposes never share material, so compromising
/// one does not compromise the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum KeyPurpose {
    /// Machine identity for mutual TLS (D-02).
    TransportIdentity,
    /// Journal encryption at rest.
    JournalAtRest,
    /// Signing configuration baselines.
    BaselineSigning,
}

/// Identifies one key version. Recorded on anything the key protected, so a
/// retired key can still be found for what it encrypted.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct KeyId {
    pub purpose: KeyPurpose,
    pub version: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum KeyState {
    /// Usable for new material.
    Active,
    /// Not used for new material; still available to read old material.
    Retired,
    /// Unavailable. Anything it protected is unreadable, and the system says so.
    Destroyed,
}

/// The custody boundary. Implementations hold key material; nothing above this
/// trait ever sees bytes it did not ask to use.
pub trait KeyProvider: Send + Sync {
    fn active(&self, purpose: KeyPurpose) -> Result<KeyId, SecurityError>;
    fn state(&self, id: &KeyId) -> Result<KeyState, SecurityError>;
    /// Encrypt or decrypt without exposing the key. The provider does the work.
    fn seal(&self, id: &KeyId, plaintext: &[u8]) -> Result<Vec<u8>, SecurityError>;
    fn unseal(&self, id: &KeyId, ciphertext: &[u8]) -> Result<Vec<u8>, SecurityError>;
    fn rotate(&self, purpose: KeyPurpose) -> Result<KeyId, SecurityError>;
}
```

`seal` and `unseal` rather than `get_key` is the design's central choice. A provider that
hands out key bytes has no custody boundary at all, and every consumer becomes a place
material can leak.

## 4. Edges

**None.** `gungnir-store` gains a reference to a `&dyn KeyProvider` passed in by the
binary, which is the same construction pattern the journal already uses. That does mean the
binary owns the provider, which is correct: custody belongs to the host, not to a library.

## 5. Behaviour

**Custody per profile.** The three profiles have genuinely different answers and the design
says so rather than picking one:

| Profile | Where material lives | Who can read it |
|---|---|---|
| Disconnected desktop | The operating system's keystore on that machine, unlocked at operator login | That machine's operator. No remote party |
| On-prem node | The deployment's own store, an operating-system keystore or a hardware module | The node process only |
| Cloud node | A managed key service, **off-host**. The node process may `seal` and `unseal` and never holds material | The node process, through the service, auditable there |

The cloud row is why `seal` and `unseal` are the interface: a managed service performs the
operation and never releases material, and a design built around fetching key bytes could
not use one.

**Rotation.** `rotate` mints a new version and moves the previous to `Retired`. New material
uses the active version; old material records the `KeyId` that protected it and is read
with that version. **Rotation never rewrites existing data.** Re-encrypting a journal on
rotation would rewrite the record, which AP-08 forbids.

**Retirement and destruction are different, and the difference is visible.** A `Retired`
key still reads. A `Destroyed` key does not, and everything it protected is permanently
unreadable. The system therefore refuses to destroy a key that protects retained data
without an explicit, recorded override naming what will become unreadable. An accidental
destruction that silently orphans a year of journals is the worst outcome in this note.

**The disconnected fallback.** A desktop that cannot reach any service must still journal
and must still start. It uses a local key from the operating-system keystore. If that
keystore is unavailable, the desktop **starts with journal encryption off and says so** in
the status strip and the health summary, rather than refusing to run or, far worse,
appearing to encrypt. That is AP-02 applied to a security feature: a system that claims
encryption it is not performing is worse than one that admits it is not.

**Audit.** Every rotation, retirement, destruction, and override is an audit entry with an
operator. Key **use** is not audited per operation, because that would put an entry in the
log for every journal append and drown the entries that matter.

## 6. Configuration and interface delta

`ConfigBaseline.security.key_provider: KeyProviderConfig`, naming which provider a
deployment uses and its parameters. **No key material, no secret, and no path to one
appears in the baseline**, which is checked by validation: a value that looks like key
material is rejected rather than stored.

Interface: nothing. Key state is not published. The health summary reports whether
encryption is active per profile, which is a boolean and not a disclosure.

## 7. User-interface delta

| Panel | Change |
|---|---|
| PN-09 System health | Whether at-rest encryption is active, and the disconnected fallback state when it is not |
| PN-01 Status strip | Journal encryption off, when it is |
| PN-20 Audit and accounts | Rotations, retirements, destructions, and overrides with their operators |
| PN-14 Configuration editor | The provider selection, with no field that accepts key material |

## 8. Verification

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-6.4 Data protection | Unit tests with an in-memory provider, plus a rotation and recovery test | No consumer can obtain key bytes through the trait; rotation leaves existing data readable and unmodified; a key protecting retained data cannot be destroyed without a recorded override naming the affected data; an unavailable keystore yields an honest unencrypted state reported in health, never a claimed-but-absent encryption; no baseline field accepts key material | Generated journals across a rotation; a provider stub that can be made unavailable |

The first criterion is enforced by the trait's shape, which is why the trait has no getter.

## 9. Amendment 1 -- **signed by the owner 2026-09-05**

Raised by GAP-060, which is the gap that implements against this note and could not
start. Three things this note settled in principle and left unusable in practice. The same
sign-off covers the code that conforms to it, and D-22 settled the crates the same day.

**Implemented 2026-09-05.** `gungnir-security/src/provider.rs` is the first
`KeyProvider` there has ever been: AES-256-GCM behind `seal`/`unseal`, the sealed form of
(b), rotation that retires rather than rewrites, and destruction that says what it has made
unreadable. `KeyProvider::sign` exists per (a) and this provider refuses it, because it
holds symmetric keys only -- the asymmetric provider a cloud deployment wants needed D-22's
third row, signed by the owner 2026-09-05 (`p256`, ECDSA P-256). The decision is taken; no
gap builds the provider yet.

Mutual TLS is `gungnir-api/src/tls.rs`, verified against real handshakes in
`gungnir-api/tests/mutual_tls.rs` with certificates generated by `rcgen` and never checked
in, per (c). It uses the **on-prem** custody model of §5's table -- a PEM the host provides,
custody being the file's permissions -- and not the cloud one, which is what `sign` is for.
The two coexist; neither supersedes the other.

### a. A provider must be able to **sign**, or there can be no mutual TLS

`KeyPurpose::TransportIdentity` names a machine identity for mutual TLS (D-02). §3's trait
offers `seal`, `unseal` and `rotate` and deliberately no getter, and §3 argues that
correctly: *"A provider that hands out key bytes has no custody boundary at all."*

But a TLS handshake needs a **signature over the transcript** with the private key, and
neither `seal` nor `unseal` can produce one. As written, `TransportIdentity` is a purpose
no consumer can use, and GAP-041 stopped at exactly this: the transport serves loopback
only because there is no way to obtain a TLS identity without breaking the boundary this
note exists to draw.

**The resolution is not to add a getter.** `rustls` does not require key bytes: its
`sign::SigningKey` is a trait, and a `ResolvesServerCert` may hand back a `CertifiedKey`
whose signer delegates elsewhere. That is how a hardware module or a managed key service
terminates TLS today, and it is exactly the shape §5's cloud row already assumes. So the
trait gains one operation:

```rust
/// Sign a message with a key the provider holds. The provider does the work, as
/// `seal` does; the bytes never leave it.
fn sign(
    &self,
    id: &KeyId,
    message: &[u8],
    scheme: SignatureScheme,
) -> Result<Vec<u8>, SecurityError>;
```

`gungnir-node` adapts it to `rustls::sign::SigningKey`, so no crate below the binary
learns about TLS and this note keeps naming no library.

**The certificate chain is not key material** and needs no custody: it is public by
construction, so it is read from a PEM path like any other configuration file. That is
what §2.9's `rustls` row means by "reads them"; only the private half goes through
the provider.

**A cost worth stating**: a signature per handshake means a round trip to a managed service
per connection in the cloud profile. That is how every KMS-backed TLS deployment works and
is acceptable for a node with few long-lived peers; it would not be for a public web
service, and this is not one.

### b. `seal` and `unseal` name no algorithm, and the sealed form has no shape

§5 requires that rotation never rewrite existing data and that old material be read with
the key that protected it. That is only possible if the ciphertext **carries its
`KeyId`**, and this note does not say it does. The sealed form is therefore specified:

```text
<KeyId as 8 bytes: purpose, version> || <96-bit nonce> || <ciphertext || tag>
```

The nonce is per-operation and never reused under one key, which is the failure mode that
makes AES-GCM catastrophic rather than merely broken.

**No cipher is named** anywhere in this note or the register, and none is in the workspace:
D-20 signed off `argon2`, `hmac`, `sha2` and `subtle` for authentication and **explicitly
did not cover this**. `hmac` authenticates and does not encrypt. That is D-22 below.

### c. The verification row cannot be met without a way to make certificates

§8 asks for tests against "a provider stub that can be made unavailable", which is
straightforward. GAP-060's other half is not: **mutual TLS cannot be tested without
certificates**, and the workspace has no certificate-generation crate and no fixture. It
must not gain a fixture either -- a private key checked into the repository is key material
in the repository, whatever the comment above it says.

So a test-only certificate generator is needed, which is a §2.9 row like any other. It
belongs in `[dev-dependencies]` and must never enter a shipped manifest.

## 10. Escrow is in this note's title and nowhere in its body

**Raised 2026-09-05 by GAP-084 and not answered here.** The gap's closing action asks for
"escrow for recorded journals"; the word appears in this note's heading and in no section
of it.

§5 answers what happens when a key is **deliberately** retired or destroyed:
`may_destroy` refuses to destroy a key protecting retained data without an override that
names what becomes unreadable. It does not answer what happens when a key is **lost** --
a machine that fails, a keystore that is wiped, an operator who leaves -- nor how a
journal is read later by somebody who is entitled to it and does not hold the key. For a
system whose journals are the record an after-action review or an investigation reads,
those are the questions an accreditor asks second, right after custody.

The shape of an answer, for the owner to accept or replace: seal each journal's data key
to a second **escrow** key held by a different authority, so the record can be recovered
without that authority being able to read anything live. That needs the asymmetric scheme
D-22 left as its third row -- signed 2026-09-05, so **that half is no longer the blocker** --
and it needs a decision about who holds the escrow key, which is a deployment's question
and not a design's, and which remains open.

Recorded rather than designed, because inventing an escrow model in a pull request is the
thing §1 of this note exists to stop.

## 11. Amendment 2 -- the escrow holder is a named security-officer role, per deployment (**signed by the owner 2026-09-06**)

**Answers §10's open question.** On 2026-09-06 the owner decided (D-27) who holds the
escrow key: **a security officer**, a named role filled by a named person in each
deployment. This section records the consequences, **signed by the owner the same day**; none
of it is built, because all of it waits on the asymmetric provider (§9a's third row,
`p256`, "not yet").

**The role.** `SecurityOfficer` joins DN-20's role table as a role that **operates
nothing**: it may not decide a plan, task a sensor, state a requirement or apply a
baseline. It may recover a sealed journal, and that is the whole of its authority. It is
held by a person recorded in the account store like any other account, so a recovery is
attributed to a name (DN-23 §5, rule 1), and it is distinct from every operating role
so that no supervisor is, by virtue of deciding engagements, also the one who can read
every record afterwards. One deployment, one officer; a deputy is a second account with
the same role, and both are named in the baseline.

**The mechanism.** At sealing time the provider wraps each journal segment's data key to
the officer's **public** key: ECDH over P-256 with HKDF-SHA-256 deriving a wrapping key
and AES-256-GCM wrapping the data key, all of which the approved stack already holds
(`p256` with its `ecdh` feature, `sha2`, `aes-gcm`; a feature flag on a signed-off crate,
recorded in §2.9 when it lands, and no new crate). The wrapped key travels with the
segment. The officer's private half **never enters a node or a desktop**: recovery is an
offline act, on a machine the officer controls, that produces a readable copy of one
segment. Nothing recovered becomes a live key, and a live system's ability to read its
own journal is unchanged by escrow either way.

**Audit.** Recovery is an authorized action, `KEY_ESCROW_RECOVER`, in
`gungnir_security::actions`, audited with the segment recovered and the officer's
identity, and the audit row is written by the recovery tool into a journal of its own,
because the journal it recovered is by definition one the live system could not read.

**Configuration delta.** `SecurityConfig` gains `escrow: Option<EscrowConfig>` with the
officer's operator identifier and the public key **inline as PEM**, not by path. A public
key is not key material and the hard rule against secrets and paths to them is kept; a
path would have been a path to a file that, on an officer's machine, sits beside the
private half. Validation refuses anything that parses as a private key, and refuses an
escrow section whose officer is not an account with the `SecurityOfficer` role. A
deployment with no escrow section escrows nothing and PN-09 says so, in the same three
states the encryption line already uses.

**Verification.** Seal a journal under a provider with escrow configured; assert the node
holding only the public half cannot recover it, that the recovery tool with a test private
key (`p256` in tests, never a checked-in key) can, that the recovered segment is
byte-identical, and that the audit row names the officer. Fixture keys are generated in
the test.

**What is decided.** The holder, the mechanism above (the least scheme the stack already
supports), the role's name and the rule that it operates nothing: all signed 2026-09-06.
GAP-084 stays open on the asymmetric provider and on the persistent keystores §5 names;
this amendment is now the design to build against.

## 12. Amendment 3 -- a passphrase-sealed keystore for the disconnected desktop (2026-09-06, **signed by the owner the same day**)

**Raised by GAP-084.** §5's disconnected row names the operating system's keystore,
unlocked at operator login. No crate in the approved stack reaches an OS keystore, and
admitting one is a §2.9 decision this amendment does not pre-empt. What the stack holds
is argon2 and AES-256-GCM, and they are enough for the property §5 wants: **a persistent
key this machine's operator can unlock and no remote party can read.**

**The mechanism.** Every key the desktop owns lives in one file, `keystore.sealed`, in
the data directory: a salt, a nonce, and AES-256-GCM ciphertext over the provider's keys,
under a wrapping key derived by argon2 from the operator's passphrase and the file's own
salt. The file is created at the first sign-in and opened at every later one. The
baseline names the mechanism (`security.key_provider: passphrase-sealed-file`) and no
path; the file's name is fixed. A file that does not open under the presented passphrase
is refused and never overwritten, because it may be the only way to read a year of
journals.

**Unlocked at sign-in, not at start.** A desktop starts with the journal in the clear and
says so on the strip, which is §5's fallback rule; from the sign-in on, the journal seals
under the store's journal key, and the lines before it stay as they were written. The
escrow record (§11) is written beside the journal at the same moment, so the key is
recoverable without this desktop from its first use.

**What it is not.** Not the OS keystore: the file is only as strong as the passphrase and
argon2's cost, and a machine an attacker can read at rest yields a ciphertext they can
attack offline. That is weaker than DPAPI-class custody and stronger than an ephemeral
key nothing can read tomorrow. The row in §5 stands as the target; this is the profile's
answer until a §2.9 decision admits the crate that reaches the OS.

**Verification.** Keys survive a restart under the same passphrase and not another; the
file holds no legible key; an escrowed journal key recovers under the officer's key
(`gungnir-security/src/keystore.rs`); the desktop's journal reports sealing only after a
sign-in (`gungnir-app/tests/keystore.rs`).

**Landed 2026-09-06, and both halves are signed**: the code (`Role::SecurityOfficer` and
the passphrase-sealed `PersistentKeyProvider`) and then this amendment as a design, each
put to the owner separately on the same day. They were kept apart on purpose while one
was signed and the other was not, because a signature on an implementation says the code
does what it says and a signature on a design says the design is the right one; recording
the first as though it were the second is how a note nobody agreed to becomes the thing
later work cites.

## 13. Amendment 4 -- the operating system's keystore, the row §5 actually named (2026-09-08, **signed by the owner the same day**)

**Raised by D-39.** §5's disconnected row never named a passphrase-sealed file; it named
"the operating system's keystore, unlocked at operator login." Amendment 3 built the
file because no crate reached the OS keystore and said so in its own text -- "until a
§2.9 decision admits an OS-keystore crate." `docs/agentic-coding-standards.md` §2.9
records that decision: `keyring` 4.2.0, `v1` feature.

**The mechanism changes what supplies one string, not the custody model.**
`PersistentKeyProvider` already does everything this row needs -- one sealed file, an
AES-256-GCM-wrapped `P256KeyProvider` snapshot, a wrapping key argon2 derives from a
string and the file's own salt. Amendment 3 got that string from an operator typing a
passphrase at sign-in. This amendment gets the same shape of string -- 32 random bytes,
hex-encoded, argon2's cost against it buying nothing beyond what it already buys against
a human passphrase, since it is fed through unchanged -- from Windows Credential
Manager, macOS Keychain, or Linux Secret Service, generated once and read back
thereafter with no prompt. One file format, one set of tests, for either source.

**Unlocked at login, not at sign-in, and that is the entire point of admitting this
crate at all.** `gungnir-app`'s `build_encryption` opens it at start, the way
`Ephemeral` already does, rather than waiting for `unlock`'s sign-in event the
passphrase-sealed file needs: the OS session being unlocked already **is** the login §5
means, so there is nothing further to wait for. A desktop configured this way therefore
encrypts from its first frame, which no other persistent profile can claim.

**`gungnir-app` only.** §5 assigns this row to the disconnected desktop specifically;
`gungnir-node` has no operator login to unlock at; its row is `ManagedService`, still
unbuilt for the reason it always was.

**What DN-22 §6 already required, applied here rather than restated**: the baseline
names a mechanism (`security.key_provider: operating-system-keystore`) and an account,
never a secret. The account distinguishes one deployment's entry from another's on the
same machine, exactly as `dir` already distinguishes their keystore files.

**Verification.** `gungnir-security/src/os_keystore.rs`'s own tests, against
`keyring-core`'s always-on mock store: a first run generates and stores a secret, a
second returns the one already stored, two generated secrets differ, and a fault
distinct from "nothing stored yet" is reported rather than read as first-run and
overwritten. **Found in review and closed the same day (2026-09-08), before signing:**
a first-run race where a second writer's `set_password` lands between this process's
own write and its return -- `ensure_secret` now re-reads the store rather than trusting
what it generated, so the loser adopts the winner's secret instead of sealing its file
under one the store no longer holds; staged directly against that interleaving, and
against a read-back that itself cannot answer (a fault, not a silent fall-through to
the generated value). `gungnir-security/tests/os_keystore.rs` and
`gungnir-app/tests/encryption_status.rs` each carry one test against whatever backend
the machine running them actually has, honest either way: where one is reachable the
secret round-trips for real and the entry is cleaned up; where none is reachable (a
headless Linux CI runner with no Secret Service session) the function's own error path
is what fires, which is §5's fallback and not a gap in coverage.

**Human-owned; signed by the owner 2026-09-08, together with item 104's node account
store and item 111's TLS-identity generalisation -- one review over the whole
OS-keystore mechanism and its four services.** The mechanism this amendment describes
and the code behind it (`gungnir-security/src/os_keystore.rs`,
`PersistentKeyProvider::open_or_create_via_os_keystore`, and the wiring in
`gungnir-app/src/state.rs`) were put to the owner together rather than kept apart the way
amendment 3's design and code were: amendment 3 was a real design decision the owner
could have taken differently, where this one is D-39 with no room left for a different
shape once the crate was chosen -- the string source changes, the reviewed and signed
custody model does not.

## 14. Amendment 5 -- `ManagedService`, the cloud node's row, designed (2026-09-08, **written and gated, not signed**)

**Raised by D-42.** §5's table has three rows and until now only two of them had a
design. The cloud row says "a managed key service, **off-host**. The node process may
`seal` and `unseal` and never holds material", and that sentence is the whole of it:
nothing says what a `seal` costs when the key is in another company's hardware, what
happens when the network to it is down, or what `may_destroy` means when the destroy
button is in somebody else's console. D-42 admitted the crates (`aws-sdk-kms`;
`azure_security_keyvault_keys` with `azure_identity`) and said in its own text that a
design note was owed before code. This is that note.

**What is categorically different about this profile.** In every other row a key is
held *inside* this process -- minted in memory, or unwrapped from a file or the
operating system's keystore into memory. Here the master key is in a hardware security
module this deployment does not own and cannot read, and reaching it is a network round
trip. That single fact decides everything below.

### a. Envelope encryption, because a round trip per envelope cannot meet the budget

The choice is between calling the key service for **every** `seal` and `unseal`, or
having it wrap a data key that then does the bulk work locally. `../performance-budgets.md`
settles it, and the numbers are not close:

- **Journal append**: under 1 ms for 50 envelopes -- **20 microseconds per envelope**.
- **Journal durability**: an accepted envelope is on disk within 100 ms, and the node
  profile fsyncs **every** envelope (D-04).

A call to a regional key service is a TLS round trip measured in tens of milliseconds.
Per envelope that is roughly a thousand times the append budget and it consumes the
entire durability budget in one network hop, before the fsync that budget is actually
about. It would also make the journal -- the system of record -- unwritable whenever the
link is slow, which is precisely the condition a deployment most wants a record of.

**So: the key service wraps, and this process encrypts.** The provider holds an
AES-256-GCM data key, `seal` and `unseal` are local and use amendment 1 (b)'s sealed form
unchanged, and the key service is called **once at start and once per rotation** and at
no other time. A journal written under this profile is byte-identical in shape to one
written under any other, which is what lets the replay and recovery tooling stay one
implementation.

**What is conceded by saying so.** The data key **is** in this process's memory; only
the master key is not. That is weaker than the literal reading of §5's "never holds
material", and this amendment corrects §5 rather than pretending to satisfy it: what
the cloud row buys is that key material is **never persisted here and never recoverable
from this host's disk**, because the only thing written down is a blob that the key
service alone can open. A process-memory disclosure on a running node reads the data
key either way, in this profile as in every other; that is a different threat and no
custody model in this note defends against it.

### b. Signing is the exception, and stays in the service

`sign` is not on any per-frame path. `TransportIdentity` signs once per TLS handshake,
and this is a node with few long-lived peers; `BaselineSigning` signs when an operator
applies a baseline. Amendment 1 (a) already priced exactly this -- "a signature per
handshake means a round trip to a managed service per connection in the cloud profile...
acceptable for a node with few long-lived peers" -- and that pricing holds.

So the split is by frequency, and stating the rule that way rather than by key type is
deliberate:

| Purpose | Where the operation happens | Cost |
|---|---|---|
| `JournalAtRest` | Locally, under a data key the service wrapped | One round trip at start, one per rotation |
| `TransportIdentity` | In the key service; the private half never leaves it | One round trip per handshake |
| `BaselineSigning` | In the key service; the private half never leaves it | One round trip per baseline signed |

**The two signing purposes therefore keep the property §5 wanted and the journal
purpose does not**, and the note says which is which rather than claiming both.

### c. The mechanism reuses amendment 3's file, the way amendment 4 did

Amendment 4's shape applies again, and this is the second time it has: `PersistentKeyProvider`
already seals a key snapshot into one file under a 32-byte wrapping key that argon2
derives from a string, and amendment 4 changed only where that string comes from. This
amendment changes it once more. The string is **32 random bytes, hex-encoded, that the
key service wraps**; the wrapped blob is written beside the keystore as
`keystore.kms-wrapped`, and at every later start it is handed back to the service to
unwrap. One file format, one derivation, one set of tests, for a third source.

As in amendment 4, argon2's cost against a high-entropy generated secret buys nothing
beyond what it already buys against a human passphrase; it is fed through unchanged so
there is no second code path.

**Why a wrapped 32-byte secret rather than wrapping the snapshot directly.** AWS KMS
`Encrypt` caps its plaintext at 4096 bytes, and a snapshot grows with every rotation --
a deployment would have hit that ceiling silently, years in, with no way to open its own
keystore. Wrapping one fixed-size secret has no ceiling.

### d. When the service is unreachable

§7's rule governs and needs no exception: an unavailable keystore yields a **stated
unencrypted state reported in health, never a claimed-but-absent encryption**. This
profile reports through the same `EncryptionStatus` three-state machinery the desktop
already uses, with no new state:

- **At start, unreachable** -- no credential, no route, or the service's own policy
  denies -- is `UnavailableWritingPlaintext { reason }`, carrying what the service
  actually said. The node starts and journals in the clear, exactly as a desktop does
  when its keystore will not open.
- **Open** is `Active { provider: "managed-service" }`.
- **Never configured** is `NotConfigured`, unchanged.

**Unreachable *after* start is deliberately not a fourth state, and the reason is worth
recording**: because the data key is already in this process, the journal keeps sealing
through an outage. That is a real availability property of (a)'s choice and not an
oversight -- a design that called the service per envelope would have stopped
journalling the moment the link went, which for a system of record is the worst possible
moment. What does fail during an outage is `sign`: an established connection continues,
a **new** handshake cannot be made, and the node refuses it by name rather than serving
without one.

### e. Rotation, retirement, destruction: what carries over and what does not

**Rotation carries over unchanged.** A rotation mints a new data key, retires the
previous one, wraps the new secret, and rewrites nothing -- §5's rule and AP-08's. Old
material still names the version that protected it in amendment 1 (b)'s header.

**Rotation of the *master* key is the service's, not this system's**, and the two must
not be confused. AWS KMS rotates the backing key under a stable ARN and a blob wrapped
before a rotation still opens after it, so this system never sees the event. Azure Key
Vault mints a new key *version* instead, so the wrapped blob records the version it was
wrapped under and is opened with that version. Neither is something this system
initiates or audits; the cloud account's own trail holds it.

**Retirement carries over.** A retired data key still reads and does not write.

**Destruction does not carry over, and this is the one place DN-22's machinery genuinely
fails to reach.** §5 says the system "refuses to destroy a key that protects retained
data without an explicit, recorded override naming what will become unreadable", and
calls an accidental destruction that orphans a year of journals "the worst outcome in
this note". `may_destroy` enforces that, and for this profile **it cannot**: deleting
the master key is an action in the cloud provider's console or API, taken by whoever
holds that account, and this system can neither prevent it, require an override for it,
nor observe that it happened until an unwrap fails. Saying so plainly is the answer;
pretending `may_destroy` covers it would be worse than the gap.

What is left in its place is not nothing, but it is not this system's either: both
services impose a mandatory waiting period before a key is actually gone (AWS KMS
schedules deletion 7 to 30 days out; Azure Key Vault offers soft-delete with purge
protection), and configuring those is a deployment act outside this baseline. **The
deployment guidance is therefore part of this design and not an afterthought**: a
`ManagedService` deployment that has not enabled its provider's deletion protection has
no equivalent of `may_destroy` at all.

### f. Escrow: required for this profile, and only for the journal

§11's escrow wraps a journal data key to the security officer's **public** key by ECDH
over P-256. For this profile it works **unchanged**, and that is a direct consequence of
(a): the data key is in process, so there is something to wrap. Had the note chosen a
call per envelope there would have been nothing to escrow at all.

**Escrow of the two signing keys does not apply and there is nothing to build.** Their
private halves never enter this process, so this system cannot wrap them to anybody.
Recovering a signing key is the cloud account's own affair. That is a real reduction in
what §11 covers for this profile and it is stated rather than left to be discovered.

**This amendment adds one rule the other profiles do not carry: a `ManagedService`
baseline with no `security.escrow` section is refused at validation.** §11 deliberately
allows a deployment to escrow nothing and have PN-09 say so, and for every other profile
that stays true. This profile is the exception because (e) is: it is the only one where
the safeguard against the note's own worst outcome is **absent rather than merely
unused**, and the escrow record -- wrapped to a key the cloud account does not hold and
stored beside the journal -- is the only thing that survives the master key being
deleted. Requiring it turns "the worst outcome in this note" from unguarded back into
guarded. **This is the amendment's own new rule, not a reading of an existing one**, and
it is the part of this note most worth an owner disagreeing with.

### g. Configuration: a region, an endpoint, a key, and never a credential

§6's rule is unchanged and this profile is the hardest test of it, because a cloud
client is exactly the place an access key would otherwise be pasted. The baseline names:

```
security.key_provider:
  kind: managed-service
  cloud: aws | azure     # which service; never inferred from the endpoint's shape
  endpoint: <AWS region, or the Azure vault URL>
  key_id: <AWS key ARN or alias, or the Azure key name>
```

**`cloud` is named and not inferred.** A region string and a vault URL are
distinguishable by eye, and guessing between two key services from the shape of a string
is the kind of confidently-wrong inference this system forbids everywhere else.

**`key_id` replaces the earlier field name `key_ring`**, which was a term from Google
Cloud's key hierarchy -- a service D-42 explicitly put out of scope. AWS has a key ARN
and Azure a key name; neither has a ring. The variant has never been constructible, so
nothing is migrated.

**How the process authenticates instead.** Neither cloud takes a credential from this
baseline:

- **AWS**: the SDK's default credential chain -- environment, web identity token (an
  EKS service account), container credentials, and the EC2 instance metadata service.
  In a cloud deployment that resolves to the instance's or pod's own IAM role.
- **Azure**: `azure_identity`'s **`ManagedIdentityCredential`**, which resolves to the
  managed identity assigned to the host -- system-assigned by default, and a
  user-assigned one where the deployment says so. **Not `DefaultAzureCredential`, which
  no longer exists**: at 1.0 the Azure SDK split that chain into a developer-tools
  credential and this one, and a deployment wants only this one. Reaching for the old
  name would have compiled against nothing; keeping the chain that falls back to a
  developer's own signed-in Azure CLI session would have been worse than that, because
  it would work on an engineer's machine and fail on the node.

Both are the same idea and it is the idea §6 was reaching for: the deployment's identity
is a property of where the process is running, granted by the cloud account, and never a
string in a file this system reads. A baseline therefore still holds no secret, and
validation's existing key-material check still refuses one pasted into `endpoint` or
`key_id`.

### h. `gungnir-node` only

The mirror of amendment 4's last rule. §5 assigns this row to the **cloud node**;
`gungnir-node` gains the arm and `gungnir-app` keeps refusing it by name -- with the
reason corrected from "designed and not built", which it no longer is, to what it
actually is now: the cloud node's custody row, not the desktop's. A connected desktop
wanting a cloud key service is a change to §5's table and therefore a later question,
not this amendment's to take.

### i. Verification, and what it honestly does not cover

The key service sits behind a `CloudKeyService` seam -- wrap, unwrap, sign, describe --
so what can be tested without a cloud account is tested against a fake that implements
it, and what cannot is named:

**Genuinely verified, against a fake:** that `seal` and `unseal` make **no** call to the
service at all, counted rather than asserted in prose, which is (a)'s whole claim; that
the wrapped secret round-trips so a restart opens the same keystore; that a service
unreachable at start yields `UnavailableWritingPlaintext` carrying the service's own
reason and never a claimed encryption; that sealing **continues** through an outage that
begins after start, and `sign` **fails** through the same outage, per (d); that a
rotation leaves earlier material readable; that escrow wrapping still works for the
journal key; that a wrapped secret which does not open, and a keystore whose wrapped
secret has gone missing, are each refused and **named** rather than quietly re-created as
a first run; and that a baseline naming this profile without an escrow section is
refused, per (f).

**Not verified, and it must not be claimed otherwise:** no call has been made to a real
AWS KMS or Azure Key Vault. The two SDK-backed implementations are compile-verified and
carry `#[ignore]`d integration tests that would run against real credentials if a
deployment had any. Whether a real service's error text, latency, wrapped-blob size and
credential-chain behaviour match what the fake models is **unverified and needs a cloud
account**. This follows the precedent amendment 4 set for the OS keystore -- honest
either way -- with the difference stated rather than glossed: there, the real backend
was reachable on the machine running the suite and half the tests actually used it; here
no real backend is reachable at all, so the real path has exactly the coverage a
compiler gives it and no more.

**Human-owned; written and gated, not signed.** The design here and the code behind it
(`gungnir-security/src/managed_service.rs`, `PersistentKeyProvider::open_or_create_via_managed_service`,
and the arm in `gungnir-node/src/main.rs`) are put to the owner together, as amendment 4
was. Unlike amendment 4, this one had real room for a different shape -- (a)'s choice
between a call per envelope and envelope encryption, and (f)'s new mandatory-escrow rule
are both decisions the owner could take differently -- so a signature here is a
signature on a design, not only on a conformance.

## Traceability

GAP-084 (plan 11 finding F-3), and GAP-060 which implements against it; CAP-6.4; D-02;
`../../ARCHITECTURE.md` §8.5; `../release-governance.md` for the open question about signed
configuration baselines, which `BaselineSigning` answers the key half of;
`../ux/wireframes/WF-09-system-health.puml`, `WF-20-audit-accounts.puml`; principles AP-02,
AP-03; contract C-04.

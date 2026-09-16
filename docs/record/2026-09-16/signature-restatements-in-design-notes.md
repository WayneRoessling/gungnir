# Signature restatements removed from the design notes on 2026-09-16

These are the passages of `docs/design/` that stated what the owner had signed, signed
off, approved, accepted or reviewed, or that something still waited for that, moved here
verbatim when `docs/signatures.md` became the only place that says what is signed. Each
block is the passage as it stood before the edit, under its original line number.

## docs/design/DN-01-defended-assets.md

Line 224:
```text
## 9. Amendment 1 -- **signed by the owner 2026-09-06**
```

Line 226:
```text
Raised 2026-09-06; signed the same day. The same sign-off covers the code that conforms to it.
```

## docs/design/DN-03-warning.md

Line 3:
```text
**Amendment 1 (§9) gives the pass-close trigger its distance, and amendment 2 (§10) its due time, both signed by the owner 2026-09-06** (corrected 2026-09-07: this line had called amendment 1 unsigned a full day after §9's own header recorded the signature). **Amendment 3 (§11, how an acknowledgement arrives) is signed by the owner 2026-09-07.**
```

Line 128:
```text
## 9. Amendment 1: the pass-close distance (2026-09-06, **signed by the owner the same day**)
```

Line 156:
```text
## 10. Amendment 2: the pass-close due time (2026-09-06, **signed by the owner the same day**)
```

Line 170:
```text
## 11. Amendment 3: how an acknowledgement arrives (2026-09-06, **signed by the owner 2026-09-07**)
```

## docs/design/DN-04-effector-model.md

Line 3:
```text
Status: signed and **implemented and wired** (GAP-030, 2026-09-06). **Amendment 1 (§9), signed by the owner 2026-09-06**: the closing speed GAP-031's geometry needs.
```

Line 132:
```text
## 9. Amendment 1 -- a closing speed, for the intercept geometry (**signed by the owner 2026-09-06**)
```

## docs/design/DN-06-engagement-and-effect.md

Line 154:
```text
## 9. Amendment 1 -- **signed by the owner 2026-09-06**
```

Line 156:
```text
Raised 2026-09-06; signed the same day. The same sign-off covers the code that conforms to it.
```

## docs/design/DN-08-policy-configuration.md

Line 4:
```text
Status: **signed off by the owner
2026-09-05.** Design only; no code exists yet.
**Human-owned and signed**: everything this note configures is a rule about who may do
what. The owner signed it on 2026-09-05, which also unblocks DN-09 and DN-10, both of which
read the schema defined here.
```

Line 179:
```text
## 9. Amendment 1 -- **signed by the owner 2026-09-05**
```

Line 182:
```text
The same sign-off covers the code that conforms to it.
```

Line 183:
```text
The behaviour below is what the note already asks for; what is new, and
what needs a signature, is **how** two of them are reached, because §6 said "Interface: no
change" and both of these change one.
```

## docs/design/DN-09-authority-and-control-status.md

Line 3:
```text
Status: **signed off by the owner 2026-09-05.** Design only; no code exists yet.
**Human-owned and signed**: `gungnir-policy` is a low-trust crate and this note defines
who may engage what. The owner signed it on 2026-09-05; it may now be implemented, and a
change to it is a change request under phase H rather than an edit.
```

## docs/design/DN-10-queue-expiry-and-escalation.md

Line 3:
```text
Status: **signed off by the owner 2026-09-05**, implemented
the same day, and **amendment 1 (§9) signed by the owner 2026-09-05**.
**Human-owned and signed**: `gungnir-command` is a low-trust crate and this note changes
what happens to a decision nobody takes. The owner signed it on 2026-09-05.
```

Line 145:
```text
## 9. Amendment 1 — **signed by the owner 2026-09-05**
```

Line 149:
```text
The same sign-off covers the code that
conforms to it: `OperatorDecision` in `gungnir-command`, and the expiry rule in
`gungnir-collab`'s `RoleRankArbiter`.
```

## docs/design/DN-11-sensor-control-and-tasking.md

Line 15:
```text
Where the implementation departs from this note, §9 and §10 record it: amendment 1,
signed by the owner on 2026-09-05, and amendment 2 (the first `SensorControlAdapter`),
signed by the owner on 2026-09-07.
```

Line 201:
```text
## 9. Amendment 1 -- **signed by the owner 2026-09-05**
```

Line 204:
```text
The same sign-off covers the code that conforms
to it.
```

Line 262:
```text
**Signed by the owner 2026-09-07.**
```

## docs/design/DN-12-coverage-and-gaps.md

Line 130:
```text
**Correction, 2026-09-05 (GAP-006), signed by the owner 2026-09-05.**
```

## docs/design/DN-14-hazard-layer.md

Line 130:
```text
## 9. Amendment 1 -- **signed by the owner 2026-09-06**
```

Line 132:
```text
Raised 2026-09-06; signed the same day. The same sign-off covers the code that conforms to it.
```

Line 135:
```text
Two corrections; both change what the note says, and each needs a signature.
```

## docs/design/DN-16-peer-sources.md

Line 4:
```text
§3's
`PeerOrigin` and `PeerSourceAdapter` are built (`gungnir-model/src/exchange.rs`,
`gungnir-ingest/src/adapters/peer.rs`), §9's launch-warning types are built and signed, and
§10 gives `LaunchWarningReport` its one caller: `gungnir-app/src/launch_warning.rs::declare`,
a manual operator action gated on `RELEASE_PRODUCT`.
```

Line 137:
```text
## 9. Amendment 1: the launch-warning type, and which exchange item gates it (2026-09-06, **signed by the owner 2026-09-07**)
```

Line 222:
```text
**Not human-owned, and so not signed.**
```

Line 224:
```text
The engineering stands on its own tests
(`gungnir-app/tests/launch_warning.rs`) rather than on a signature, the same way GAP-065
recorded its outbox and producer as "ordinary transport and wiring work" separately from
the one new action that did need the owner's sign-off.
```

## docs/design/DN-17-releasability.md

Line 3:
```text
Status: **signed off by the owner 2026-09-05.** Design only; no code exists yet.
**Human-owned and signed**: this is `gungnir-security`'s enforcement and the interface's
write path. The owner signed it on 2026-09-05.
```

## docs/design/DN-18-coalition-exchange.md

Line 3:
```text
Status: first draft, 2026-09-05; amendment 1 (2026-09-06, signed
2026-09-07) built the three read routes; amendment 2 (2026-09-08, written and gated where
human-owned, signed by the owner the same day) built the write path, its store-and-forward,
and the handoffs producer.
```

Line 126:
```text
## 9. Amendment 1: the exchange endpoints §6 said would not exist (2026-09-06, **signed by the owner 2026-09-07**)
```

Line 163:
```text
## 10. Amendment 2: the write path, its store-and-forward, and one producer (2026-09-08, **written and gated where human-owned, signed by the owner the same day**)
```

Line 231:
```text
`gungnir-security` (the `PUBLISH_EXCHANGE` action and its role
grants, now three: `Commander`, `IntelligenceAnalyst`, and `Supervisor`) and the
`gungnir-api` write path are human-owned (`docs/agentic-workflow.md`); both are signed by
the owner the same day this amendment was written.
```

## docs/design/DN-21-battle-rhythm.md

Line 181:
```text
## 9. Amendment 1 -- **signed by the owner 2026-09-05**
```

Line 184:
```text
The same sign-off covers the code that
conforms to it.
```

## docs/design/DN-22-key-management.md

Line 3:
```text
Status: **signed off by the owner 2026-09-05**, with **amendment 1 (§9) signed the same day** after GAP-060 found the note unusable as written: no way to obtain a TLS identity, no algorithm behind `seal`, and no way to test either. **Amendment 2 (§11), signed by the owner 2026-09-06**: who holds the escrow key, which §10 left open (D-27). **Amendment 3 (§12), signed by the owner 2026-09-06**: a passphrase-sealed keystore as the disconnected profile's persistent custody until a §2.9 decision admits an OS-keystore crate. **Amendment 4 (§13), 2026-09-08, signed by the owner the same day**: that decision taken (D-39) and the OS keystore built as amendment 3's sibling, unlocked at operator login rather than typed at sign-in. The owner's review found a first-run race in the secret-generation helper before signing; §13 records the fix that closed it. **Amendment 5 (§14), 2026-09-08, signed by the owner 2026-09-10**: `ManagedService`, the third and last row of §5's table, designed at last -- envelope encryption because the journal budget forbids a network round trip per envelope, signing left in the service because it is not on a per-frame path, and the one place `may_destroy` cannot reach said plainly rather than papered over. The review before signing found and closed a real defect in the code behind it: Azure Key Vault's raw ECDSA signature format, handed through unconverted, would have broken every TLS handshake a Key-Vault-backed identity signed (`ARCHITECTURE.md` §10 item 127).
```

Line 4:
```text
**Human-owned and signed**: `gungnir-security` is a low-trust crate and this note decides
who can read what. The owner signed it on 2026-09-05.
```

Line 147:
```text
## 9. Amendment 1 -- **signed by the owner 2026-09-05**
```

Line 150:
```text
The same
sign-off covers the code that conforms to it, and D-22 settled the crates the same day.
```

Line 156:
```text
`KeyProvider::sign` exists per (a) and this provider refuses it, because it
holds symmetric keys only -- the asymmetric provider a cloud deployment wants needed D-22's
third row, signed by the owner 2026-09-05 (`p256`, ECDSA P-256).
```

Line 222:
```text
**No cipher is named** anywhere in this note or the register, and none is in the workspace:
D-20 signed off `argon2`, `hmac`, `sha2` and `subtle` for authentication and **explicitly
did not cover this**.
```

Line 253:
```text
That needs the asymmetric scheme
D-22 left as its third row -- signed 2026-09-05, so **that half is no longer the blocker** --
and it needs a decision about who holds the escrow key, which is a deployment's question
and not a design's, and which remains open.
```

Line 261:
```text
## 11. Amendment 2 -- the escrow holder is a named security-officer role, per deployment (**signed by the owner 2026-09-06**)
```

Line 265:
```text
This section records the consequences, **signed by the owner the same day**; none
of it is built, because all of it waits on the asymmetric provider (§9a's third row,
`p256`, "not yet").
```

Line 278:
```text
At sealing time the provider wraps each journal segment's data key to
the officer's **public** key: ECDH over P-256 with HKDF-SHA-256 deriving a wrapping key
and AES-256-GCM wrapping the data key, all of which the approved stack already holds
(`p256` with its `ecdh` feature, `sha2`, `aes-gcm`; a feature flag on a signed-off crate,
recorded in §2.9 when it lands, and no new crate).
```

Line 308:
```text
The holder, the mechanism above (the least scheme the stack already
supports), the role's name and the rule that it operates nothing: all signed 2026-09-06.
```

Line 313:
```text
## 12. Amendment 3 -- a passphrase-sealed keystore for the disconnected desktop (2026-09-06, **signed by the owner the same day**)
```

Line 347:
```text
**Landed 2026-09-06, and both halves are signed**: the code (`Role::SecurityOfficer` and
the passphrase-sealed `PersistentKeyProvider`) and then this amendment as a design, each
put to the owner separately on the same day. They were kept apart on purpose while one
was signed and the other was not, because a signature on an implementation says the code
does what it says and a signature on a design says the design is the right one; recording
the first as though it were the second is how a note nobody agreed to becomes the thing
later work cites.
```

Line 355:
```text
## 13. Amendment 4 -- the operating system's keystore, the row §5 actually named (2026-09-08, **signed by the owner the same day**)
```

Line 393:
```text
**Found in review and closed the same day (2026-09-08), before signing:**
```

Line 406:
```text
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
```

Line 417:
```text
## 14. Amendment 5 -- `ManagedService`, the cloud node's row, designed (2026-09-08, **signed by the owner 2026-09-10**)
```

Line 665:
```text
**Human-owned; signed by the owner 2026-09-10.** The design here and the code behind it
(`gungnir-security/src/managed_service.rs`, `PersistentKeyProvider::open_or_create_via_managed_service`,
and the arm in `gungnir-node/src/main.rs`) were put to the owner together, as amendment 4
was. Unlike amendment 4, this one had real room for a different shape -- (a)'s choice
between a call per envelope and envelope encryption, and (f)'s new mandatory-escrow rule
are both decisions the owner could take differently -- so the signature is on a design,
not only on a conformance. The review before it found and fixed a real defect: Azure Key
Vault's `sign` returns a raw ECDSA signature this crate's `KeyProvider::sign` contract
never expected, which would have broken every TLS handshake a Key-Vault-backed identity
signed (`ARCHITECTURE.md` §10 item 127).
```

## docs/design/DN-23-operator-authentication.md

Line 3:
```text
Status: **signed off by the owner 2026-09-05**, with D-20 (the crates)
settled the same day.
```

Line 259:
```text
**Signed by the owner 2026-09-07.**
```

Line 268:
```text
The
authentication half of GAP-057 was implemented and signed on 2026-09-06 and was, in the
only sense that matters to a deployment, unreachable.
```

Line 305:
```text
## 11. Amendment 2 (2026-09-08, **signed by the owner the same day**): the operating system's keystore for a node's own accounts
```

Line 307:
```text
**Human-owned; signed by the owner 2026-09-08, together with DN-22 §13's OS-keystore
mechanism and item 111's TLS-identity generalisation -- one review over the whole
mechanism and its four services.** The owner's review found and closed a first-run race
in `os_keystore::ensure_secret` before signing (DN-22 §13 records the fix); this
amendment shares that helper and the fix applies here too.
```

## docs/design/DN-24-mission-profiles-and-algorithm-baselines.md

Line 3:
```text
Status: **signed by the owner 2026-09-05**, and implemented the same day
under GAP-086. The sign-off covers the code that conforms to this note, and a second
sign-off the same day covers the three corrections in §5, §6 and §8 that building it
raised — the missed dependency edge, the contradiction between §6 and §9 about
`model.promote`, and the two event variants §8 did not name.
```

Line 107:
```text
**Correction, 2026-09-05 (GAP-086), signed by the owner 2026-09-05.**
```

Line 162:
```text
**Correction, 2026-09-05 (GAP-086), signed by the owner 2026-09-05.**
```

Line 206:
```text
**Correction, 2026-09-05 (GAP-086), signed by the owner 2026-09-05.**
```

## docs/design/DN-25-cursor-on-target.md

Line 3:
```text
Status: first draft, 2026-09-06; **the design (§1 to §9) signed
by the owner as the one to build, 2026-09-10. No code exists.**
```

Line 50:
```text
**Signed by the owner 2026-09-10: this table's four-way split, and the "no new crate"
finding it rests on, is the right shape.** Checked before that signature against what each
named type and function actually does today (`PeerOrigin`'s `assigned_quality`/`age_s`/
`is_stale_beyond`, `ExchangeFormat::is_lossy`, `ExchangeSet::may_send`,
`Releasability::permits`), against `external-standards.md` §5's pinned schema, §5.7's
`friend` predicate and §1.5's copyleft rule, and against `dependency-edges.md` (see §4's
note on what that check turned up). No code exists; this signature is on the design alone,
the same two-step DN-28 and DN-29 went through -- a future `gungnir-model`/`gungnir-interop`/
`gungnir-ingest`/`gungnir-remote` diff is signed on its own account, against this design.
```

Line 125:
```text
**Accepted by the owner as engineering reviewer, 2026-09-06**, and recorded as edge (v) in
[`dependency-edges.md`](dependency-edges.md) §16 -- relabelled 2026-09-09 from (s)/§13, which
collided with the real, already-drawn `gungnir-node` to `gungnir-identity` edge of the same
letter (`dependency-edges.md` §16's own note has the finding).
```

Line 255:
```text
**All three rows were agreed by the owner on 2026-09-06 and have moved into**
[`../verification-capability-table.md`](../verification-capability-table.md) §2, under
"Rows added by DN-25"; [`verification-rows.md`](verification-rows.md) maps them back here.
```

Line 306:
```text
Requires
[`external-standards.md`](external-standards.md) §5 for the pinned schema, the transcribed
attributes and the licence finding, and §5.7 for the type tree, the `friend` predicate and
the case-sensitivity finding; **D-33** for the scope decision and the pins, extended
2026-09-07 to cover the type tree; edge (v) accepted 2026-09-06 and recorded in
[`dependency-edges.md`](dependency-edges.md) §16 (relabelled 2026-09-09 from (s)/§13, which
collided with a different, real edge of the same letter -- §16's own note has the finding).
```

## docs/design/DN-26-laydown-options.md

Line 4:
```text
Status: **signed by the owner 2026-09-06, confirmed 2026-09-07** (the confirmation
was needed because the signed status appeared on disk with nobody present to vouch for
it; see GAP-087's register entry).
```

## docs/design/DN-27-bearing-only-detections.md

Line 4:
```text
Status: **proposed 2026-09-06; signed by the owner 2026-09-07.**
```

Line 6:
```text
**Built 2026-09-06 and gated; signed 2026-09-07, and the note is unchanged below.**
```

Line 24:
```text
That was true when this note was signed
and stopped being true the next day: **GAP-001's closing action built the resolver**
(`gungnir_tracking_service::SensorPositions`, `with_sensor_positions`, 2026-09-07), so a
bearing whose sensor has a declared position now reaches `offer_bearing` as a
`BearingDetection` and is offered under DN-27 §5's three rules; only a bearing from a
sensor the deployment never declared a position for still refuses, under
`SubmitError::NotAPosition` or `UnknownSensorPosition` depending on which of the two is
missing.
```

## docs/design/DN-28-imm-in-the-pipeline.md

Line 5:
```text
Status: **proposed 2026-09-07, signed by the owner 2026-09-07.
Built and gated 2026-09-07, and signed by the owner the same day.**
```

Line 8:
```text
**What each signature settled.** The design signature is on the design -- the scope in
§6, the gating question raised as open in §3, and the sizing in §8. The code signature is
on the `gungnir-fusion-async`/`gungnir-filters`/`gungnir-config` diff built from it (§4,
§5): the `TrackFilter` enum and its interleaving with the reorder buffer
(`gungnir-fusion-async`, concurrency correctness), the gating formula on `Imm`'s combined
estimate (`gungnir-filters`, numerical stability), and the config validation mirroring
`Imm::new`'s own rules (`gungnir-config`). Per the distinction this directory's other
notes draw throughout (DN-16, DN-18, DN-27): a signature on an implementation says the
code does what it says; a signature on a note says the design is the right one to build.
§3's gating question -- whether gating against the IMM's combined, spread-widened
covariance is the right choice -- is answered by §7 item 2's clean isolation (identical
settings, only `filter_selection` differs, `kf-cv` still does not confirm through the
turn and `imm-cv-ct` does) rather than by argument alone, and that evidence is what the
code signature is on.
```

Line 212:
```text
Medium-to-large, not XL: the estimator, its oracle gate, and the association/lifecycle
machinery around it are all already built and signed (item 94).
```

Line 220:
```text
Per
`docs/agentic-workflow.md`, the mandatory verification gate ran (§7, all rows passing)
and the owner signed the diff 2026-09-07: `TrackFilter`'s interleaving with the reorder
buffer adds no new await point and no new shared state -- the enum is matched on and
mutated exactly where the old concrete type was, under the same `&mut self` the pipeline
already serializes through one task -- and the gating formula in §3 is the right one
because §7 item 2 measured it rather than argued it.
```

## docs/design/DN-29-a-third-mode-for-the-imm.md

Line 6:
```text
Status: **proposed 2026-09-07; §5's recommendation signed by the
owner 2026-09-09. No code exists.**
```

Line 9:
```text
This is a scoping document, matching DN-28's own two-signature discipline (a signature on
a design says the design is the right one to build; a signature on an implementation says
the code does what it says): it names the problem, weighs two designs, recommends one, and
states what is explicitly out of scope, so an owner can sign the shape of the work before
anyone writes the `gungnir-core` or `gungnir-filters` diff. **That signature is what is
recorded here** -- the augmented-state design (§5) is the right one to build. It is not a
signature on any code, because none exists yet: **the implementation still needs §6's
three open questions answered or explicitly deferred to its own verification gate before
that future diff is itself signed**, the identical two-step DN-28 went through.
```

Line 130:
```text
First, it changes public, already-signed, already-gated surface
(`Imm`, `ModeFilter`; item 94's oracle gate and DN-28's `imm.rs` tests) rather than adding
beside it, so every existing guarantee about those types has to be re-argued rather than
inherited.
```

Line 144:
```text
## 5. Recommendation (signed by the owner 2026-09-09: this is the design to build)
```

Line 146:
```text
It needs zero changes to `gungnir-filters::Imm` or
`ModeFilter`, both signed off under item 94 and exercised by `imm_diff.rs`'s oracle gate;
it needs zero changes to the `TrackFilter`/`FilterSelection`/`PipelineSettings` shape DN-28
just built and tested, beyond one more enum arm and wider arrays, which is exactly the
one-line-per-touch-point change `pipeline.rs`'s `match` arms are built for (DN-28 §4); and
it needs exactly two new `gungnir-core` types, each smaller than `ConstantAcceleration`
itself, that are additions rather than redefinitions of a type `gungnir-core` owns.
```

Line 159:
```text
**The owner signed this recommendation on 2026-09-09, on its design merits alone**: no
`gungnir-core`, `gungnir-filters`, or `gungnir-fusion-async` diff exists yet, and this
signature does not stand in for the one that diff will need on its own account (per this
note's own two-signature discipline, restated at the top). §6's three questions are
unanswered as of this signature and remain the gate before any implementation lands.
```

Line 165:
```text
## 6. The open numerical-stability question §5 still has to answer before sign-off
```

Line 211:
```text
- **No claim that scenario 1 is tracked through its full 300 s duration.** That is the
  acceptance test §8 proposes building, once this note is signed and implemented — not a
  result this note reports.
```

Line 239:
```text
Per `docs/agentic-workflow.md`, this was a design note only, and the precondition its own
opening paragraph named — an owner's signature on §5's recommendation — is now met
(2026-09-09). An agent may now draft the `gungnir-core`/`gungnir-filters`/
`gungnir-fusion-async` diff on that basis, but the mandatory verification gate still has
to include §6's three questions answered — either here in a signed amendment or in the
implementation PR's own written argument — before the code is `main`-worthy, per the same
low-trust-tier reasoning DN-28 §8 stated for itself. **Not requested this session**: the
signature above covers the design only, and no implementation PR was asked for.
```

## docs/design/DN-30-measurement-noise-from-the-baseline.md

Line 6:
```text
Status: **proposed, built, gated and
signed by the owner 2026-09-07; re-reviewed 2026-09-09** (§7), the review finding one
thing to change in the low-trust crate: `from_baseline` no longer keeps a value it cannot
honour by silently substituting the default (§4).
```

Line 39:
```text
It follows the exact approximation DN-28 §7 already used and
got signed off on for scenario 1's own test -- treating a sensor's
`[sigma_range², sigma_cross², sigma_height²]` as if it were `[east, north, height]`
variance -- because a deployment naming one sensor's noise as its baseline figure is the
same approximation, made once at configuration time instead of once per test.
```

Line 159:
```text
Built and gated
2026-09-07, and **signed by the owner the same day** over the `gungnir-fusion-async`
diff. `docs/agentic-workflow.md`'s low-trust tier names `gungnir-fusion-async` for
concurrency correctness and numerical stability; this change touches neither
(`from_baseline` gains an argument it validates and stores, with no new await point, no
new shared state, and no change to how a filter is predicted or updated), but the crate
is named by the tier itself rather than by what any one change inside it does, so the
signature was sought rather than assumed unnecessary. The record of that signature was
written on a branch that was never merged (`claude/dn-29-measurement-noise-baseline`,
under the note's number at the time) and is carried here instead.
```

Line 170:
```text
**Re-reviewed 2026-09-09, on the owner's request, before the record was made**
```

## docs/design/README.md

Line 7:
```text
**Status: 2026-09-05.** Twenty-two notes and four consolidations, **all implemented**,
plus DN-23 (signed, desktop half implemented) and **DN-24 (signed and implemented
2026-09-05)**.
```

Line 10:
```text
Three of its own sections needed correcting while it was
being built; those corrections were signed the same day.
```

Line 15:
```text
Signed off by the owner on 2026-09-05: **all five human-owned notes** DN-08, DN-09, DN-10,
DN-17, and DN-22; option B for the one breaking change; and all twenty-three verification
rows, which have moved into
[`../verification-capability-table.md`](../verification-capability-table.md) §2 as agreed
criteria. Nothing in the set now waits on the owner.
```

Line 28:
```text
The set is fully reviewed and signed.
```

Line 43:
```text
| DN-01 Defended assets | **Implemented and wired** (GAP-026) (amendment 1 signed 2026-09-06) | `gungnir-model/src/assets.rs`, the asset section and its validation in `gungnir-config`, `gungnir-assessment/src/assets.rs`; `sustainment::asset_assessor` scores the picture against it and PN-06 draws the score |
```

Line 44:
```text
| DN-04 Effector model | **Implemented and wired** (GAP-030, 2026-09-06); **amendment 1 (§9) signed 2026-09-06**, its field landed | `gungnir-model/src/effectors.rs`, the resource fields and their validation in `gungnir-config`, `ResourceView::is_adequate`; the planner filters on it and PN-05 draws what was withheld and why. `intercept_speed_mps` (amendment 1, D-26) is on both types, validated, and read by nothing until GAP-031's solver |
```

Line 49:
```text
| DN-14 Hazard layer | **Implemented and wired** (GAP-017, 2026-09-06) (amendment 1 signed 2026-09-06) | `gungnir-geo/src/hazard.rs`, two new layer kinds; `HazardConfig` in the baseline, `gungnir-app/src/hazards.rs`, drawn on PN-02, toggled on PN-11, listed on PN-14. The PN-04 note waits on GAP-020 and the PN-16 input on GAP-087 |
```

Line 52:
```text
| DN-06 Engagement and effect | **Implemented and wired on the desktop** (GAP-043, 2026-09-06) (amendment 1 signed 2026-09-06) | `gungnir-intercept-service/src/engagement.rs`, `DecisionId` in the model, engagement and expiry events; `gungnir-app/src/engagements.rs` opens on a decision and closes on track-lifecycle evidence; PN-17 and the report count the evidence sources apart. Effector reports arrive through the node's report route and move the engagement (GAP-040, 2026-09-06) |
```

Line 53:
```text
| DN-24 Mission profiles and algorithm baselines | **Signed and implemented 2026-09-05**, with three §-corrections signed the same day | `gungnir-model/src/profiles.rs`, the profile schema and its six rules in `gungnir-config`, `gungnir-modelops` keyed on `AlgorithmBaselineId`, `gungnir-app/src/governance.rs`, the node's session record, PN-14's profiles section. GAP-053 closed 2026-09-06 and GAP-011 with it |
```

Line 54:
```text
| DN-23 Operator authentication and sessions | **Amendment 1 (§10), 2026-09-07, signed by the owner the same day**: the note gave the account file a format and never said who writes it, and `hash_passphrase` had no caller outside tests, so the file could not be produced by any shipped path; `gungnir-node account add|list` now writes it. **Signed 2026-09-05; desktop half implemented and wired** (GAP-057, 2026-09-06) **Session lifetime enforced on the desktop and PN-01's operator line drawn 2026-09-06** (GAP-057; `LocalAccountAuthority::with_lifetime` signed by the owner 2026-09-06). **Amendment 2 (§11, the node's own OS-keystore account store), 2026-09-08, signed by the owner the same day**, with DN-22 §13 and `ARCHITECTURE.md` item 111 in one review; the review found and closed a first-run race in the shared `os_keystore::ensure_secret` helper before signing | `gungnir-security/src/session.rs` (`FileAccountStore`), `gungnir-config`'s `security.authentication`, `gungnir-app/src/session.rs`, PN-20. A sign-in establishes the node link. The node-issued token and `POST /v2/session` are not built |
```

Line 55:
```text
| DN-11 Sensor control and tasking | **Implemented and wired** (amendment 1 signed 2026-09-05); **amendment 2 (§10, the first `SensorControlAdapter`, SAPIENT), 2026-09-07, signed by the owner the same day** | `gungnir-sensor-management/src/lib.rs` (`SensorControl`) and `tasking.rs`, `sapient_task.rs`, `gungnir-model/src/requirements.rs`, `gungnir-workflow/src/tasking_case.rs`, PN-10 and PN-15 in `gungnir-ui`, `gungnir-app/src/requirements.rs`. A command travels to the node and is closed by the acknowledgement it streams back (GAP-004, 2026-09-06). **The wire itself, both ways, is now built and signed** (`ARCHITECTURE.md` item 106, 2026-09-08): `TcpSapientSource::sink`/`TcpTaskSink` in `gungnir-ingest` carry a task out on the same connection the inbound adapter already reads, and `gungnir-node/src/main.rs`'s `SapientTaskRouter` and `apply_sapient_task_acks` complete the round trip on the node |
```

Line 58:
```text
| DN-03 Warning | **Implemented and wired** (GAP-042, 2026-09-06) **Amendments 1 (§9, the pass-close distance) and 2 (§10, the due time from the closest approach) both signed by the owner 2026-09-06; amendment 3 (§11, how an acknowledgement arrives) signed by the owner 2026-09-07.** Amendment 3 is the one that made §5 rule 2 reachable: `Warning::acknowledged` had existed with no caller, so a delivered warning went `Sent` then `Late` for ever | `gungnir-workflow/src/warning.rs`: `raise_due` and `mark_overdue` under a `WarningLedger` the desktop tick evaluates; PN-08 and PN-04 list warnings. No transport exists, so every warning fails loudly until D-08's endpoints carry one; the pass-close trigger waits on a distance the obligation does not carry |
```

Line 63:
```text
| DN-16 Peer sources | **Wired** (GAP-009, 2026-09-06) | `gungnir-model/src/exchange.rs`, peer origin on `Provenance`; `gungnir-ingest/src/adapters/peer.rs` over `gungnir_remote::peer::PeerLink`, a machine link under the host's certificate, bound per peer on both binaries; PN-09 and the node's health line say whether each is linked. **Launch warnings as a distinct message (§5) built 2026-09-06**: `LaunchWarningReport` carries no kinematic state, which is §5's own reason for the message existing, and "it never creates a track" is held structurally -- warnings and detections ride separate queues, so nothing in the ingest path can make a detection from a warning. **Amendment 1 (§9), written the same day, signed by the owner 2026-09-07**, because §3 named no type for one and §5 named no exchange item to send it under. **No producer**: nothing issues a launch warning |
```

Line 65:
```text
| DN-18 Coalition exchange | **Wired both ways for the picture, and the write path is built** (GAP-065, 2026-09-06; amendment 2 2026-09-08, written and gated where human-owned, signed by the owner the same day) | `gungnir-model/src/exchange.rs`, the two independent gates, composed in `gungnir-api` for a machine caller: `exchange` agreements in the baseline, a party with none refused with the reason. Inbound through the peer link since later the same day (GAP-009). **Amendment 1 (§9, three `/v2/exchange` routes), 2026-09-06, signed by the owner 2026-09-07**, recording that §6's "no new endpoints" was diverged from: `Warnings`, `Reports` and `Handoffs` had no producer and no gate, and they cannot ride a snapshot. **Amendment 2 (§10)** answers what amendment 1 left open: `POST` on the same three paths (`gungnir_security::actions::PUBLISH_EXCHANGE`, human-owned, signed the same day), store-and-forward mirroring `gungnir-remote`'s sensor-task outbox, and one producer -- `gungnir-app/src/handoffs.rs` republishes the whole handoff set on every new one. `Warnings` and `Reports` still have no producer |
```

Line 66:
```text
| DN-22 Key management | **Implemented and wired** for the provider surface (GAP-084); **amendment 2 (§11) signed 2026-09-06** **The keystore code, the escrow record at sign-in, and amendment 3 (§12, the passphrase-sealed keystore) are all signed by the owner 2026-09-06; the node's provider-issued TLS identity (D-29, `gungnir-remote/src/identity.rs`) is reviewed and signed by the owner 2026-09-10, after an earlier record of this same code conflated a 2026-09-06 trust-tier classification with a code signature (`ARCHITECTURE.md` §10 item 126).** **Amendment 4 (§13, the OS keystore itself, D-39), 2026-09-08, signed by the owner the same day**, together with DN-23 §11's node account store and `ARCHITECTURE.md` item 111's TLS-identity generalisation -- one review over the whole mechanism and its four services, which found and closed a first-run race in `os_keystore::ensure_secret` before signing. **Amendment 5 (§14, `ManagedService`, the cloud row, D-42), 2026-09-08, reviewed and signed by the owner 2026-09-10**, design and code together: the review found and closed a real defect, Azure Key Vault's raw ECDSA signature format reaching a TLS handshake unconverted (`ARCHITECTURE.md` §10 item 127) | `gungnir-security/src/keys.rs`, five new authorization actions; the desktop builds the provider the baseline names and PN-01 reports the sealing state honestly. The asymmetric provider (`P256KeyProvider`, signing and escrow per §11) is built and unwired (2026-09-06, signed by the owner the same day); the OS keystore is not built; no binary writes an escrow record yet |
```

Line 67:
```text
| DN-27 Bearing-only detections | **Proposed 2026-09-06; signed by the owner 2026-09-07. Built and gated 2026-09-06; §7's display half is not built at all.** Unblocks the acoustic, passive-RF and spotter halves of GAP-001 and the sensor half of GAP-004 | `DN-27-bearing-only-detections.md`. The note exists to forbid one thing: **a bearing must never become a position by assuming a range**, in any of its three tempting forms -- a nominal range, a terrain intersection, or a projection onto a defended asset -- each of which produces a valid `DetectionView` at a place nothing is. `DetectionView.measurement` becomes an enumeration and every variant carries its error, because for a bearing the error *is* the information. Three rules: a bearing may update a track and **may not initiate one** (a fixed sensor cannot localise from bearings at all, and a filter given them converges confidently to the wrong range); two crossing bearings may initiate, refused below a minimum crossing angle rather than initiated with a huge covariance; an unmatched bearing is kept and drawn as a **ray**, never a symbol. Ghost resolution is named as `gungnir-association`'s open problem rather than specified. **Breaking: `SCHEMA_VERSION` must bump**, and it did, from 2 to 3. The note's own §1 records what landed where, including the one piece built and not wired: `gungnir-tracking-service` names a bearing `NotAPosition` rather than offering it to the pipeline, because nothing resolves the reporting sensor's position for it |
```

Line 68:
```text
| DN-26 Laydown options | **Signed by the owner 2026-09-06, confirmed 2026-09-07. The schema and the options table are built (GAP-087); the rehearsal section, the gap-acceptance control and the viewport push are not.** Unblocks GAP-087, and through it GAP-020's approach corridors and GAP-045's rehearsal record | `DN-26-laydown-options.md`. Gives a laydown a schema and an identity so a planning panel has alternatives to compare, which is the same shape of fix DN-24 made for algorithm baselines and for the same reason: an options table with one row is theatre with a map behind it. **Adds no dependency edge.** Three rules are about not showing a plausible number: a comparison carries the terrain model it was computed under, a laydown that could not be evaluated is not one that scored zero, and any ranking is labelled advisory. **Adopting a laydown is deliberately out of scope**: moving a sensor is a physical act with an authority chain this system does not model |
```

Line 69:
```text
| DN-25 Cursor-on-Target exchange | **Design only; no code exists** (GAP-090, GAP-091, 2026-09-06). Not part of the plan-11 set; the design signed by the owner as the one to build, 2026-09-10 | `DN-25-cursor-on-target.md`. Adds `ExchangeFormat::CursorOnTarget` and a `ReportedPosition` that is deliberately not a track; the codec in `gungnir-interop`, the feed in `gungnir-ingest` on edge (i), the sink in `gungnir-remote` behind proposed edge (v) (relabelled 2026-09-09 from (s), which collided with a different, real edge of the same letter -- `dependency-edges.md` §16). **Both preconditions met 2026-09-06**: `external-standards.md` §5 pins the schema under D-33, and edge (v) is accepted. All three of its verification rows (CAP-7.4, CAP-3.8, CAP-1.6) were agreed 2026-09-06 and are in `../verification-capability-table.md` §2. It waits on a self-recorded corpus before the codec. **Researched 2026-09-08** in `tak-interoperability-research.md`: the client's own source sends protobuf on the mesh from its first datagram, so the corpus has a mesh half (protobuf) and a stream half (XML, via a recorder mode not yet written), and the owner took the three D-33 extensions (e, f, g) the same day: the framing is pinned at `TAK-Product-Center/atak-civ` tag 5.5.1.8 and transcribed in `external-standards.md` §5.4.2, and the recorder gained its `--tcp` and `--unicast` modes. **The recording itself is the one open step before the codec** |
```

Line 70:
```text
| DN-28 Selecting the IMM in the fusion pipeline | **Signed by the owner 2026-09-07. Built and gated 2026-09-07, and the code signed by the owner the same day.** Not part of the plan-11 set | `DN-28-imm-in-the-pipeline.md`. Closed what `ARCHITECTURE.md` §10 item 94 named and did not build: `gungnir-fusion-async`'s `TrackFilter` is now an enum over the fixed constant-velocity filter and the CV/CT IMM, selected by `PipelineSettings::filter_selection`, and `"imm-cv-ct"` -- the default baseline's own name -- is in `IMPLEMENTED_FILTERS`. Motivated by a diagnosed defect rather than a feature request: the track-fragmentation finding on GAP-011 did not go away under 1000x more process noise, because a CV-only filter cannot represent a real turn. Scoped narrowly on purpose -- CV and CT are both six-dimensional `MotionModel`s, unlike the nine-dimensional constant-acceleration phase found while verifying this (below), so `Imm<6, 3>` over that pair is dimensionally identical to the pipeline's existing filter. `Imm` gained `innovation`/`innovation_covariance` over its combined estimate for gating (`gungnir-filters`), and `gungnir-config` validates a candidate's `imm-cv-ct` fields against the same rules `Imm::new` refuses on. **Building the acceptance test found two confounds neither belongs to `imm-cv-ct`**: scenario 1's comparison instant sits in a nine-dimensional constant-acceleration phase no six-dimensional filter can represent, and `PipelineSettings::default().measurement_noise_var` understates scenario 1's actual sensor noise by up to 25x, which alone produces most of the fragmentation. Isolated from both, the row is clean: identical settings but `filter_selection`, `kf-cv` still does not confirm through the turn and `imm-cv-ct` does |
```

Line 71:
```text
| DN-29 A third mode for the pipeline's IMM | **Design only; no code exists. §5's recommendation (the augmented-state IMM, over the more general heterogeneous-dimension redesign) signed by the owner 2026-09-09** | `DN-29-a-third-mode-for-the-imm.md`. Scopes the follow-up DN-28 §7 named: scenario 1's true comparison instant sits in a constant-acceleration phase no six-dimensional `Imm<6, 3>` mode can represent (`gungnir_core::ConstantAcceleration` is a `MotionModel<9>`), so DN-28's own row was truncated to source time < 199 s. Recommends two new nine-dimensional `gungnir-core` motion models (`ConstantVelocityAugmented`, `CoordinatedTurnAugmented`) padded to share `ConstantAcceleration`'s state layout, over redesigning `Imm`/`ModeFilter` to hold mixed-dimension modes directly -- the smaller change, needing no change to either type's already-signed public surface. **Adds no dependency edge.** Three numerical-stability questions (padding inertness, the phantom block's own process noise, the accel prior at initiation) are named as the gate an implementation still has to clear, unanswered as of this signature; no `gungnir-core`/`gungnir-filters`/`gungnir-fusion-async` diff exists |
```

Line 72:
```text
| DN-30 Measurement noise from the baseline | **Proposed, built and signed by the owner 2026-09-07; re-reviewed 2026-09-09**, the review turning `from_baseline`'s silent fallback for a gate threshold or noise axis it cannot honour into a named refusal (`ARCHITECTURE.md` §10 item 124) | `DN-30-measurement-noise-from-the-baseline.md`. Closes the follow-up DN-28 §7 named and left unfixed: `measurement_noise_var` now reaches `PipelineSettings::from_baseline` from `TrackingConfig`/`TrackingProfileConfig` (`gungnir-config`, validated finite and positive, unconditionally on filter selection), so a deployment can match its baseline to its actual sensor instead of running every deployment on `PipelineSettings::default()`'s generic `[400, 400, 900]`. `scenario_truth_replay.rs`'s scenario 1 and 2 tests now run with `radar_medium`'s and `radar_coastal`'s own noise: scenario 1's worst measured error drops from 238 m to 169 m and its coverage-test track count from three (none confirmed) to two (still none confirmed) -- most of the original fragmentation finding was this noise mismatch, not a defect DN-30 closes on its own account. Scenario 2 is nearly unchanged (85 m to 83 m; its ten-tracks-for-six-vessels count is unaffected, driven by clutter and dropout rather than noise). Two things this note explicitly does not do, named rather than absorbed: it does not rotate a sensor's line-of-sight noise into ENU per detection (it follows DN-28 §7's own approximation instead), and it does not carry `radar_coastal`'s real zero height variance into scenario 2's test, because this pipeline seeds a new track's prior covariance from the same array it builds `R` from, and an exact zero there is a zero prior, not a stated noise |
```

Line 81:
```text
It is **signed by
the owner 2026-09-06, confirmed 2026-09-07**; `ConfigBaseline.laydowns` with its five
refusals is built the same day (GAP-087), and the panel is the remaining engineering.
```

Line 89:
```text
It is not signed and nothing in it is built.
```

Line 98:
```text
**Signed by the owner 2026-09-07**,
and built, gated, and signed the same day: the design signature covers the scope, the open
gating question in §3, and the sizing in §8; the code signature is on the
`gungnir-fusion-async`/`gungnir-filters`/`gungnir-config` diff built from it, per
`docs/agentic-workflow.md`.
```

Line 111:
```text
Proposed, built and signed by the owner the same
week as DN-28 -- it touches `gungnir-fusion-async`'s low-trust tier even though the
change itself is mechanical (a new argument to an existing function, no new estimator,
no new concurrency), so the signature was sought rather than assumed unnecessary -- and
re-reviewed 2026-09-09, when its one silent fallback became a refusal (DN-30 §4).
```

Line 229:
```text
All five were
signed by the owner on 2026-09-05, DN-08 last because an earlier version of this index left
it off the list.
```

Line 239:
```text
| DN-08, DN-09, DN-10, DN-17, DN-22 | **All signed.** The three that need no dependency edge (DN-08, DN-09, DN-10) are cleared to implement; DN-17 and DN-22 need no edge either |
```

## docs/design/dependency-edges.md

Line 3:
```text
Status: **reviewed and accepted by the engineering reviewer, 2026-09-05; edges (h), (i) and (j) accepted 2026-09-06 (§7).**
```

Line 18:
```text
All three are in the
manifests and drawn in §7.1 as edge (h), and the owner signed the third as a correction to
DN-24 §5 on 2026-09-05.
```

Line 20:
```text
**None was seen by the engineering reviewer**, which is a separate
review from the owner's sign-off and is still outstanding for these three.
```

Line 102:
```text
| `gungnir-remote` to `gungnir-interop` | DN-25, GAP-091 | **No: accepted 2026-09-06, no code yet** | Not yet; §7.1 gains (v) in the change that adds the sink (§16, correcting a collision with the real (s) below -- see §16's own note) |
```

Line 150:
```text
## 7. Edges (h), (i) and (j) -- **accepted by the owner as engineering reviewer, 2026-09-06**
```

Line 156:
```text
This is the record drafted for one
acceptance covering all five, drafted and accepted the same day. What the reviewer
accepted, and the evidence for each:
```

Line 163:
```text
| modelops → model | A productization crate joining the model. The identity type `AlgorithmBaselineId` has to live in the model because `Provenance` carries one; the alternative was two bare strings assembled in every caller, a second answer to "what is a baseline" outside the crate that owns them (DN-24 §5 correction, signed) | `dependency_graph.rs`: `Productization → Model` is downward; no cycle |
```

Line 184:
```text
Accepted by the owner on 2026-09-06, as engineering reviewer, on the evidence above and
with `gungnir-app/tests/dependency_graph.rs` passing; the same line stands in
`ARCHITECTURE.md` §7.1 against (h), (i) and (j). Every edge in the graph is now reviewed.
```

Line 188:
```text
## 7a. Edges (k) and (l) -- **accepted by the owner as engineering reviewer, 2026-09-06**
```

Line 190:
```text
Added by GAP-028's node half, after the §7 acceptance, and accepted the same day on the evidence below with `gungnir-app/tests/dependency_graph.rs` passing.
```

Line 206:
```text
## 10. Edges (n) and (o) -- **accepted by the owner as engineering reviewer, 2026-09-06**
```

Line 211:
```text
Reviewed and accepted the same day.
```

Line 218:
```text
## 11. Edges (p) and (q) -- **accepted by the owner as engineering reviewer, 2026-09-06**
```

Line 222:
```text
Reviewed and accepted the same day.
```

Line 234:
```text
## 13. Edge (s) -- **accepted by the owner as engineering reviewer, 2026-09-06**
```

Line 237:
```text
Reviewed and accepted the same day.
```

Line 255:
```text
## 12. Edge (r) -- **accepted by the owner as engineering reviewer, 2026-09-06**
```

Line 259:
```text
Reviewed and accepted the same day.
```

Line 263:
```text
| (r) fusion-async → core, filters, association | The pipeline predicts a track to a measurement's time, gates the measurement, assigns and updates: a motion model, a filter and an associator. It names the three crates that own them rather than carrying its own. **All three were already beneath this crate** through `gungnir-track` → `gungnir-association` → `gungnir-filters` → `gungnir-core`, so the graph gains no reach and no depth; what changed is that the manifest now says what the code imports. The refused alternative was a filter inside `gungnir-fusion-async`, which would have put a second Kalman update in the workspace beside the signed one | `dependency_graph.rs`; `gungnir-fusion-async/tests/oos_convergence.rs` |
```

Line 270:
```text
## 16. Edge (v) -- **accepted by the owner as engineering reviewer, 2026-09-06; relabelled 2026-09-09**
```

Line 272:
```text
Accepted
ahead of the code rather than with it, because DN-25 is a design-only note and it had to say
which crate owns the sink before it could say anything else about it.
```

Line 276:
```text
This section and §13 below
were both numbered 13 and both named their edge (s) -- a genuine collision found reviewing
DN-25 for the owner's design sign-off, between this still-unbuilt edge and §13's real,
already-drawn `gungnir-node` to `gungnir-identity` (`dependency_graph.rs` hardcodes that one
as `"(s)"`; ARCHITECTURE.md §10 item 98 is where it closed).
```

Line 280:
```text
(s) is legitimately item 98's;
this section keeps its 2026-09-06 acceptance date but takes the next letter and number
actually free, following (u) at §15.
```

Line 356:
```text
**Not yet put to the owner.** Unlike the lettered edges above, this one has not been
individually reviewed; it is recorded here in the same change that adds it to the
manifest, per §5's rule that the two land together, so the review has something
concrete to look at rather than a bare Cargo.toml line.
```

## docs/design/external-standards.md

Line 252:
```text
**For the owner's review**: the pin, the delta reading, and the copies' provenance.
```

Line 423:
```text
`gungnir_interop::adsb`, on the open-source-consensus route the owner approved under
GAP-010.
```

Line 1172:
```text
**Reviewed before signing, 2026-09-09.**
```

Line 1180:
```text
The adapter was signed by the owner the same day
(`ARCHITECTURE.md` §10 item 113); the tag semantics remain this secondary source's
reading, and the primary text remains unpinned.
```

Line 1226:
```text
What was needed was the same bearing-only
decision §7.3 names, and it was already built and signed (DN-27) before this section's
survey was written.
```

Line 1281:
```text
**Reviewed before signing, 2026-09-09.**
```

Line 1291:
```text
The adapter's
Category 205 arm was signed by the owner the same day (`ARCHITECTURE.md` §10 item 114).
```

Line 1327:
```text
**A genuine discrepancy in the primary source, found and not silently resolved -- and
re-read at the owner's review, 2026-09-09.**
```

Line 1409:
```text
**`gungnir-ingest` is human-owned; signed by
the owner 2026-09-09 (next paragraph).**
```

Line 1412:
```text
**Reviewed before signing, 2026-09-09 (GAP-101).**
```

Line 1417:
```text
The adapter's Category 129 arm and its wiring were signed by the owner the same
day (`ARCHITECTURE.md` §10 item 123).
```

## docs/design/handoff-2026-09-06-radar-feed.md

Line 46:
```text
It is drawn as (j) in `ARCHITECTURE.md` §7.1
  and listed in `dependency-edges.md` §4a; and it was reviewed -- `dependency-edges.md`
  §7, accepted by the owner as engineering reviewer on 2026-09-06, on the evidence of
  `gungnir-app/tests/dependency_graph.rs`, which now checks every edge's direction and
  the graph's acyclicity on every `cargo test`.
```

## docs/design/model-and-schema-deltas.md

Line 20:
```text
| `Measurement` (replacing `DetectionView.measurement`) | DN-27, proposed 2026-09-06; **built 2026-09-06, DN-27 still unsigned**, and `SCHEMA_VERSION` is now 3 | **Breaking, and there is no smaller change.** An optional range beside a position would let a producer set both or neither and force the reader to guess. Three variants -- position, range/azimuth/elevation, and bearing -- each carrying its own error, because a bearing's angular error *is* its information and a fixed positional variance is wrong at every range but one. `elevation_rad` is `Option` because a missing elevation is not a zero one: zero means the horizon, and an optional that serialised indistinguishably from zero would put every acoustic detection there. **`SCHEMA_VERSION` must bump**; the exact-match version rule will then refuse a peer one version out, which is the correct outcome |
```

Line 42:
```text
| `Event` | Gains a `Requirement` variant | Yes | **Not in DN-11 §6.** Added 2026-09-05 with GAP-005 because CAP-2.12's method is an MT-08 replay and a lifecycle that never reaches the journal cannot be replayed. DN-11 amendment 1 (a), signed 2026-09-05 |
```

## docs/design/verification-rows.md

Line 3:
```text
Status: **agreed by the owner 2026-09-05.**
```

Line 15:
```text
Having the owner agree them at
design time rather than at review time is what makes contract C-17 enforceable: a later
change to any of these is a change request under phase H, not an edit.
```

# Signature restatements removed from code comments on 2026-09-16

These are the passages of Rust source and Cargo manifest comments that stated what the
owner had signed, moved here verbatim when `docs/signatures.md` became the only place
that says what is signed. Each block holds the whole lines of the original sentences,
comment markers included, under the line number its first line had before the edit.

## Cargo.toml

Line 199:

```text
# Docking for the role workspaces (signed off 2026-09-05, D-19;
# docs/agentic-coding-standards.md §2.9). Pinned to 0.10 because that is the line that
```

Line 207:

```text
# API transport stack (signed off 2026-09-05, D-18; docs/agentic-coding-standards.md
# §2.9). JSON over HTTP plus a WebSocket event stream, per docs/gungnir-api-v1.md,
```

Line 232:

```text
# Cryptography (D-20, signed off 2026-09-05). No cryptographic crate was in this
```

Line 251:

```text
# gRPC, the second transport for peer C2 systems (D-21, signed off 2026-09-05).
```

Line 253:

```text
# gRPC surface. tonic 0.14 and prost 0.14 share axum's hyper, tower and http rather than
# pulling second copies, which was the condition of the sign-off.
```

Line 258:

```text
# Data protection (D-22, signed off 2026-09-05, with DN-22 amendment 1).
```

Line 269:

```text
# UUID v7 for `GlobalEntityId` (D-11, signed off 2026-09-04; landed 2026-09-05 under
# GAP-069).
```

Line 281:

```text
# ECDSA P-256 for `KeyPurpose::BaselineSigning` and for the asymmetric provider a cloud
# deployment's TLS identity needs (D-22's third row, signed off 2026-09-05).
```

Line 305:

```text
# D-39, signed off 2026-09-08: the operating system's keystore (DN-22 §5, amendment 3's
# "until a §2.9 decision admits an OS-keystore crate"). `v1` is the crate's own
```

Line 330:

```text
# ONNX Runtime inference (D-40, signed off 2026-09-08; docs/agentic-coding-standards.md
# §2.9, "ONNX inference runtime"). GAP-077's `Model` trait needed a real backend; `ort`'s
```

Line 360:

```text
# D-42, signed off 2026-09-08: the cloud node's `ManagedService` custody profile
# (DN-22 §5's third row, designed by DN-22 amendment 5 §14). AWS and Azure are the two
```

## gungnir-api/Cargo.toml

Line 30:

```text
# GAP-060, D-18: mutual TLS on the node's surface. Signed off 2026-09-05 and unused
# until now; `rustls::pki_types::pem` reads the PEM and never logs what it read.
```

## gungnir-api/src/transport.rs

Line 53:

```text
//! **`POST` on those same three paths (GAP-065, DN-18 §5 amendment 2, human-owned,
//! signed by the owner the same day) is the write path DN-18's own amendment 1 said
//! neither existed nor was decided.** The caller is this deployment's own desktop,
```

Line 1513:

```text
/// amendment 2). **`gungnir-api` write path: human-owned, signed by the owner the same
/// day** (`docs/agentic-workflow.md`).
```

## gungnir-api/tests/exchange.rs

Line 241:

```text
/// Holds `PUBLISH_EXCHANGE` (GAP-065, written and gated, signed by the owner the same day).
```

## gungnir-assessment/src/assets.rs

Line 7:

```text
//! Design: docs/design/DN-01-defended-assets.md §5 (signed 2026-09-05). Capability
```

## gungnir-collab/src/lib.rs

Line 61:

```text
/// The expiry rule is a decision-authority rule and was **signed by the owner on
/// 2026-09-05** with DN-10 amendment 1 (§9).
```

## gungnir-command/src/lib.rs

Line 15:

```text
//! The queue is timed as of GAP-034 and GAP-035 (**signed by the owner on
//! 2026-09-05**): [`InMemoryApprovalWorkflow`] holds [`PendingApproval`]s carrying the
//! deadlines [`queue::deadlines`] computes from the baseline, [`ApprovalWorkflow::sweep`]
//! applies expiry and escalation, and the desktop tick calls it every frame.
```

Line 20:

```text
//! `DecisionRecord::to_event` carries the verdict and the rationale as of GAP-047's
//! resolution (**signed by the owner on 2026-09-06**), so MOE-05 can be read from the
//! journal alone.
```

Line 50:

```text
/// This is `docs/design/DN-10-queue-expiry-and-escalation.md` §3's type, conformed to
/// on 2026-09-05 and **signed by the owner** the same day with amendment 1 (§9). Two things had drifted from the signed note and both mattered:
```

Line 132:

```text
                // MOE-05 reads both from the journal (signed by the owner 2026-09-06).
```

## gungnir-command/src/queue.rs

Line 7:

```text
//! Design: docs/design/DN-10-queue-expiry-and-escalation.md, **signed by the owner
//! on 2026-09-05**. Capabilities CAP-3.6 and CAP-3.7; mission thread MT-01 under
//! saturation.
```

Line 14:

```text
//! Wired into [`crate::InMemoryApprovalWorkflow`] under GAP-034 and GAP-035, **signed by
//! the owner on 2026-09-05**. Until then every function here was correct, fully tested
```

## gungnir-data/src/geospatial/mod.rs

Line 21:

```text
//! **Real-world CRS conversion** (signed off 2026-09-08, D-41): a file's `GridCrs` no
//! longer has to be `Unstated` or the deployment's own local frame. `crs::to_wgs84`
```

## gungnir-fusion-async/src/dense_group.rs

Line 53:

```text
//! **Human-owned (concurrency), signed by the owner 2026-09-10**: see
//! `crate::pipeline`'s module documentation, "The dense-group mode", for what the
//! review covered and what it found (`ARCHITECTURE.md` §10 item 128).
```

Line 140:

```text
    /// **The default.** Both derivations are now signed by the owner (PHD 2026-09-06,
    /// CPHD 2026-09-09, item 109; this mode's own wiring, item 128) -- the reason this
    /// used to be the default (the cheaper filter being the only one reviewed) no
    /// longer holds, and whether to flip it is left to the owner rather than changed in
    /// passing here. See [`Self::Cphd`] for the case for the flip.
```

Line 157:

```text
    /// and "often 0, occasionally 5". **No longer not-the-default for the reason this
    /// comment used to give**: the derivation is signed (2026-09-09, item 109), and
    /// this mode's own wiring around it is now signed too (item 128). Whether to make
    /// this the default is a live open question left to the owner, named rather than
    /// decided here.
```

Line 555:

```text
    /// The default threshold cites the workspace's own association limit rather than
    /// inventing one, and the default filter is the signed one.
```

## gungnir-fusion-async/src/lib.rs

Line 128:

```text
/// **Signed by the owner 2026-09-09.** This crate is human-owned
/// (`docs/agentic-workflow.md`); this struct and the channel type change below were the
/// mechanical part of wiring [`FusionPipeline::retained_bearings`] and
/// [`FusionPipeline::stats`] out to a caller, written and gated 2026-09-08 and reviewed
/// before signing. The bundling claim below is not only gated but model-checked:
```

Line 198:

```text
/// **True since 2026-09-06** (GAP-011): [`pipeline::FusionPipeline`] composes the
/// signed linear Kalman filter, the Jonker-Volgenant associator, the chi-square gate
/// and the `gungnir-track` lifecycle over a reorder buffer, and
/// `tests/oos_convergence.rs` gates the `fusion-async` row -- the async path against
/// the offline batch over the same multi-sensor timeline.
```

## gungnir-fusion-async/src/loom_model.rs

Line 201:

```text
    // **Found reviewing this file for the owner's signature.** `dense_group` and
```

## gungnir-fusion-async/src/pipeline.rs

Line 14:

```text
//! Every piece of mathematics here already existed and was signed off on 2026-09-05
//! (`ARCHITECTURE.md` §10 item 36): the Joseph-form linear Kalman filter, the
//! Jonker-Volgenant assignment solver behind [`GlobalNearestNeighbor`], the chi-square
//! gate, and the confirm/coast/delete state machine in `gungnir-track`. What was
```

Line 19:

```text
//! time, gates, associates, updates, initiates and ages. That is this module, and it
//! adds no new estimator of its own -- **the one exception is `imm-cv-ct`'s selection
//! logic** (DN-28, [`TrackFilter::ImmCvCt`]), which composes the already-signed IMM
//! (item 94) rather than estimating anything new either.
```

Line 51:

```text
//! **Human-owned (concurrency; `docs/agentic-workflow.md`): the wiring above --
//! [`FusionPipeline::run_dense_group`], its engage/release state machine, and
//! [`PipelineSnapshot::dense_group`]'s epoch coherence with everything else in the
//! same bundle -- is signed by the owner 2026-09-10** (`ARCHITECTURE.md` §10 item
//! 128), reviewed alongside `gungnir-rfs`'s LMB derivation (item 128 also carries
//! that). `crate::loom_model`'s epoch-coherence assertion checked `tracks`,
//! `retained_bearings` and `stats` but never `dense_group`, added to
//! `PipelineSnapshot` after that check was written; extended in the same review so a
//! future change that split it onto a second channel -- the exact failure the loom
//! suite exists to catch -- would actually be caught rather than assumed safe by the
//! same argument that covers the older three fields.
```

Line 913:

```text
    /// clock has passed -- rather than here. Found in review before signing (item 115):
    /// until then, with no further bearing arriving, the last unmatched bearing was
    /// drawn for as long as the session lasted, while PN-08 had told the operator it was
    /// retained for `bearing_retention_s`.
```

## gungnir-ingest/src/adapters/asterix.rs

Line 29:

```text
//! **The Category 205 arm and its [`DfBinding`] (GAP-100): human-owned (the
//! `gungnir-ingest` gateway), signed by the owner 2026-09-09**, after the review before
//! signing confirmed the codec's bearing scale and angular reference against the
//! primary text's own item definitions (`gungnir_interop::asterix::cat205`'s module
//! documentation records the one discrepancy inside that text) and added the angular
//! range refusals those definitions state.
```

Line 36:

```text
//! **The Category 129 arm, its [`UasBinding`] and `uas_detection` (GAP-101): human-owned
//! (the `gungnir-ingest` gateway), signed by the owner 2026-09-09**, after the review
//! before signing checked every item's scale, width and sign against the primary
//! text's own item pages and reversed the codec's reading of the one item that text
//! contradicts itself on (`gungnir_interop::asterix::cat129`'s module documentation:
//! I129/120, one octet).
```

## gungnir-ingest/src/adapters/misb.rs

Line 46:

```text
//! **Human-owned (the `gungnir-ingest` gateway); signed by the owner 2026-09-09**,
//! after the review before signing found and closed the corrupt-length stall above,
//! recorded the missing-altitude axis a placed fix silently carried, and made the
//! codec's fixed tag widths strict (`gungnir_interop::misb0601`).
```

Line 79:

```text
/// byte than a frame worth waiting for. Waiting is not free: every byte that arrives
/// behind such a header is swallowed until the promised count is met, so before this
/// bound existed (2026-09-09, found in review before signing) a single corrupt length
/// stalled the feed for good -- twenty valid frames queued behind a header claiming
/// four gigabytes produced nothing across two hundred polls, with no error, no
/// resynchronization, and a buffer that only grew. A header past this bound is now
```

Line 376:

```text
                // The fix is still placed, but an absent Sensor True Altitude puts it
                // at 0 m in the local frame, and that axis is then not a measurement:
                // named in the provenance, the same way `cat048::map` records "no
                // height in report" rather than passing the radar site's height off
                // as measured (2026-09-09, found in review before signing).
```

Line 613:

```text
    /// The stall `MAX_FRAME_BYTES` closes (2026-09-09, found in review before signing):
    /// one header claiming four gigabytes, then twenty valid frames. Before the bound,
```

Line 694:

```text
    /// A fix with no Sensor True Altitude is still placed -- at 0 m in the local
    /// frame -- and says so in its provenance rather than carrying a 30 m vertical
    /// sigma for an axis nobody measured, the same record `cat048::map` keeps for a
    /// plot with no height (2026-09-09, found in review before signing).
```

## gungnir-ingest/src/adapters/sapient.rs

Line 22:

```text
//! gate is the whole of this change. **Human-owned (the `gungnir-ingest` gateway),
//! signed by the owner 2026-09-08.**
```

Line 29:

```text
//! **Human-owned (the `gungnir-ingest` gateway), signed by the owner 2026-09-08.**
```

Line 39:

```text
//! inbound. [`TcpTaskSink::send`]'s own doc comment records a `WouldBlock` handling bug
//! found in review and closed the same day, before signing. **Human-owned (the
//! `gungnir-ingest` gateway); signed by the owner 2026-09-08.**
```

Line 282:

```text
    /// **Retries on `WouldBlock` instead of treating it like any other error
    /// (2026-09-08, found in review before signing).** This connection is nonblocking
```

Line 1627:

```text
    /// The bug `send_within` closes (2026-09-08, found in review before signing):
    /// `write_all` treated a transient `WouldBlock` like a dead connection and gave up
    /// mid-message. This forces a real `WouldBlock` -- not a mocked one -- with many
```

## gungnir-ingest/src/lib.rs

Line 47:

```text
    /// How strong an admission by this authenticator is (GAP-002; **signed by the owner
    /// 2026-09-06**). The gateway stamps
```

Line 236:

```text
                // GAP-002 (signed by the owner 2026-09-06): the record says how strongly the
                // source was authenticated, from the authenticator that admitted it, never
                // from the adapter.
```

## gungnir-model/src/events.rs

Line 394:

```text
/// **Not in DN-11 §6**: added by amendment 1 (a), signed by the owner 2026-09-05. §6
```

## gungnir-model/src/identity.rs

Line 8:

```text
//! **The newtype is a UUID** as of GAP-069 (D-11, signed 2026-09-04). It stayed a bare
```

## gungnir-model/src/policy_settings.rs

Line 8:

```text
//! Design: docs/design/DN-08-policy-configuration.md (signed 2026-09-05), with
//! `WeaponsControlStatus` from docs/design/DN-09-authority-and-control-status.md.
```

## gungnir-model/src/releasability.rs

Line 7:

```text
//! Design: docs/design/DN-17-releasability.md, **signed by the owner 2026-09-05**.
```

## gungnir-node/src/account.rs

Line 12:

```text
//! **no way to write one**. The authentication half of GAP-057 was built, signed, and
//! unreachable in practice: the node warned `no caller authority` and the only remedy
//! was to run a Rust test.
```

Line 30:

```text
//! **Human-owned: this CLI writes credential material** (`hash_passphrase`'s output)
//! **into the account store both `gungnir-security` and `gungnir-app`/`gungnir-node`
//! trust; signed by the owner 2026-09-10** (`ARCHITECTURE.md` §10 item 125), after the
//! review found and closed a real gap: `role_from_str` had every role this workspace had
//! when this file was written (2026-09-07) but not `Role::IntelligenceAnalyst`, added to
//! the enum afterward and never re-checked against this CLI -- the one role this system
//! grants coalition-exchange release authority to
//! (`gungnir_security::authz::role_permits`) could not be provisioned on a node at all.
```

## gungnir-node/src/auth.rs

Line 6:

```text
//! **Signed by the owner 2026-09-06.**
```

## gungnir-policy/src/authority.rs

Line 7:

```text
//! Design: docs/design/DN-09-authority-and-control-status.md, **signed by the owner
//! on 2026-09-05**. Capabilities CAP-3.6 and CAP-6.2; measure MOP-38.
```

Line 435:

```text
    /// **Signed by the owner 2026-09-06** (this crate is human-owned).
```

## gungnir-policy/src/lib.rs

Line 41:

```text
    /// stable per variant. One mapping, used by every publisher (GAP-028; **signed by
    /// the owner 2026-09-06**).
```

Line 128:

```text
/// The lifetime is what lets a chain hold engines that borrow, and it was added
/// under GAP-038, **signed by the owner on 2026-09-05**.
```

## gungnir-remote/src/identity.rs

Line 8:

```text
//! **Human-owned (the certificate and key-custody path, added to
//! `docs/agentic-workflow.md`'s low-trust list by name on 2026-09-06): the issuance code
//! below -- `ProviderKey`, `issue`, `issue_for_client`, and the invariant they exist
//! for -- is signed by the owner 2026-09-10.** The gap register's own account of this
//! path's history said both "signed by the owner 2026-09-06" and, a few sentences later,
//! "unsigned" for the same code; that contradiction, not a code defect, is what made this
//! review overdue, and it is corrected in the same change as this signature. The
//! "Persistent identities" section below was already reviewed and signed on 2026-09-08
//! (`ARCHITECTURE.md` §10 item 111); this signature is the one the code above it never
//! had a documented instance of.
```

Line 23:

```text
//! (`the_certificates_own_embedded_key_is_the_true_one`) checks it directly. Every other
//! claim in this file's documentation was checked against the code it describes:
//! `ProviderKey::sign` and `ProviderSigner::sign` both go through `KeyProvider::sign`
//! alone, with no path that reads or returns the private scalar, which is what
//! `gungnir-app/tests/architecture_compliance.rs` also pins at the crate-surface level;
//! the ephemeral-vs-persistent fallback in the later section falls back honestly rather
//! than silently, per its own already-signed review.
```

Line 55:

```text
//! # Persistent identities (2026-09-08, GAP-060's remaining slice; human-owned per
//! `docs/agentic-workflow.md` -- signed by the owner 2026-09-08, `ARCHITECTURE.md` §10
//! item 111)
```

Line 465:

```text
    /// **Found reviewing this file for the owner's 2026-09-10 signature.** The existing
```

## gungnir-rfs/src/lib.rs

Line 25:

```text
//! **The CPHD build was signed by the owner on 2026-09-09**, after the review before
//! signing found and closed a numerical instability in its leave-one-out elementary
//! symmetric functions (see `elementary_symmetric_leave_one_out`'s own doc comment).
//! **The LMB build is signed by the owner on 2026-09-10**, after the same review found
//! and closed a different instability: [`association_marginals`]'s doc comment on the
//! log-domain rewrite has the finding, a genuine underflow at the settings' own stated
//! label ceiling. Both are reached by `docs/agentic-workflow.md`'s numerical-stability
```

Line 676:

```text
/// **Deliberately not the `O(m^2)` synthetic-division shortcut this replaced
/// (2026-09-09, found in review before signing).** `E(x) = Π(1 + v_i x)` factors as
```

Line 1455:

```text
/// # Log domain, and why: a real underflow found reviewing this before signing
```

Line 3041:

```text
    /// **Found and fixed reviewing this file for the owner's signature.** No fixture
```

## gungnir-security/Cargo.toml

Line 24:

```text
# D-20, signed off 2026-09-05: operator authentication (GAP-057, DN-23).
```

Line 33:

```text
# D-22, signed off 2026-09-05: AES-256-GCM behind KeyProvider::seal/unseal (DN-22
# amendment 1 b). The sealed form carries the KeyId that protected it, so rotation never
```

## gungnir-security/src/account_store.rs

Line 9:

```text
//! **Human-owned** (docs/agentic-workflow.md); signed by the owner 2026-09-08
//! (`ARCHITECTURE.md` §10 item 104, reviewed with items 103 and 111).
```

## gungnir-security/src/asymmetric.rs

Line 7:

```text
//! **Signed by the owner 2026-09-06** (`gungnir-security` is human-owned).
```

## gungnir-security/src/authz.rs

Line 43:

```text
        // PUBLISH_EXCHANGE (GAP-065, signed by the owner the same day): granted alongside
        // RELEASE_PRODUCT on the judgment that whoever may mark a product releasable
        // should be who may send it, matching the §4 row this change added in
        // `docs/mission/roles-and-stakeholders.md`.
```

Line 60:

```text
        // PUBLISH_EXCHANGE joins RELEASE_PRODUCT here for the same reason it joins it
        // above (GAP-065, signed by the owner the same day).
```

Line 82:

```text
        // to match rather than left as an exception with no stated reason. Human-owned
        // (gungnir-security); signed by the owner 2026-09-08.
```

Line 199:

```text
    /// GAP-065 (signed by the owner the same day) tied `PUBLISH_EXCHANGE` to
    /// `RELEASE_PRODUCT` by rule: whoever may release, may publish. Amended the same
```

Line 227:

```text
    /// GAP-065 commit. Investigated and signed by the owner 2026-09-08 -- see the
    /// comment on `Role::Supervisor`'s arm.
```

## gungnir-security/src/keys.rs

Line 7:

```text
//! Design: docs/design/DN-22-key-management.md, **signed by the owner 2026-09-05**.
```

## gungnir-security/src/keystore.rs

Line 7:

```text
//! **Signed by the owner 2026-09-06** (`gungnir-security` is human-owned). The header
//! here had not caught up with `ARCHITECTURE.md`'s own record of that day; corrected
//! 2026-09-08 after the owner confirmed it directly, the same way DN-26's laydown
//! signature needed a direct confirmation before this register could act on it.
```

Line 34:

```text
//! at every sign-in. **Human-owned; signed by the owner 2026-09-08 (`ARCHITECTURE.md`
//! §10 item 103).**
```

Line 46:

```text
//! identical mechanism. **Human-owned; signed by the owner 2026-09-08 (`ARCHITECTURE.md`
//! §10 item 111).**
```

## gungnir-security/src/lib.rs

Line 199:

```text
    /// **Signed by the owner the same day.** Granting this to `Commander` and
```

Line 209:

```text
    /// (DN-20 §6). Audited under this name (GAP-059; signed by the owner 2026-09-06);
    /// not yet in the role table.
```

Line 213:

```text
    /// Audited under this name (GAP-059; signed by the owner 2026-09-06); tasking
    /// authority is `TASK_SENSOR`.
```

## gungnir-security/src/managed_service.rs

Line 11:

```text
//! **Human-owned** (docs/agentic-workflow.md: `gungnir-security` decides who can read
//! what); signed by the owner 2026-09-10, over both halves together -- the design
//! (DN-22 amendment 5, §14) and this code -- per the register's own rule that this
//! profile needed both signed at once.
```

Line 16:

```text
//! **One real defect found and fixed before signing.** Key Vault's `sign` returns an
```

Line 708:

```text
/// **Found and fixed 2026-09-10, reviewing this file for the owner's signature.**
```

Line 1162:

```text
    /// **Found and fixed reviewing this file for the owner's 2026-09-10 signature.** Key
```

## gungnir-security/src/os_keystore.rs

Line 8:

```text
//! **Human-owned** (docs/agentic-workflow.md: `gungnir-security` decides who can read
//! what); signed by the owner 2026-09-08 (`ARCHITECTURE.md` §10 item 103, reviewed with
//! items 104 and 111: the review found and closed `ensure_secret`'s first-run race, see
//! `read_back`).
```

## gungnir-security/src/provider.rs

Line 169:

```text
    /// Signed by the owner 2026-09-06 with the asymmetric provider.
```

## gungnir-security/src/session.rs

Line 7:

```text
//! Design: `docs/design/DN-23-operator-authentication.md`, signed off 2026-09-05.
```

Line 259:

```text
/// Local accounts in a JSON file: `[{"operator": 7, "role": "Operator", "phc": "$argon2id$..."}]`
/// (DN-23 §5, the disconnected desktop's store; GAP-057; **signed by the owner
/// 2026-09-06**).
```

## gungnir-security/src/token.rs

Line 166:

```text
    /// HMAC accepts a key of any length, so the error is unreachable in practice; it is
    /// still returned rather than unwrapped, because the unwrap policy (CONTRIBUTING,
    /// checked by `gungnir-app/tests/architecture_compliance.rs`) allows no exception
    /// on a request path, and a panic here would take the transport down with it.
    /// **Signed by the owner 2026-09-06** (GAP-081).
```

## Found after the first pass

The same kind of passage, in files the first pass's list did not name, removed the same day.

### gungnir-api/src/tls.rs

Line 7:

```text
//! Crates: `rustls` and `tokio-rustls`, signed off under D-18 on 2026-09-05 and unused
```

### gungnir-api/tests/mutual_tls.rs

Line 9:

```text
//! is why D-22 signed off `rcgen` as a development dependency and nothing else.
```

### gungnir-app/Cargo.toml

Line 131:

```text
# GAP-075 / D-19: the in-window dock tree. Signed off in `agentic-coding-standards.md`
```

### gungnir-ml/src/onnx.rs

Line 5:

```text
//! `OnnxModel`: the [`Model`] trait's real backend, now that GAP-077's runtime sign-off
```

### gungnir-node/Cargo.toml

Line 50:

```text
# Edge (s), accepted by the owner as engineering reviewer 2026-09-06 (GAP-019): the node
```

### gungnir-interop/src/asterix/cat129.rs

Line 51:

```text
//! undocumented). The review before the adapter was signed weighed everything the
```

### gungnir-interop/src/asterix/cat205.rs

Line 34:

```text
//! not "correct" this decoder the wrong way (2026-09-09, found in review before the
//! adapter was signed).**
```

### gungnir-interop/src/misb0601/mod.rs

Line 83:

```text
//! rather than scaled as if it were. **Made strict 2026-09-09, in review before the
//! adapter was signed**:
```

### gungnir-fusion-async/tests/bearing.rs

Line 305:

```text
/// (2026-09-09, found in review before item 115 was signed).
```

### gungnir-tracking-service/src/lib.rs

Line 726:

```text
        // a snapshot arrived (2026-09-09, found in review before item 115 was signed).
```

### gungnir-tracking-service/src/lib.rs

Line 1182:

```text
    /// The other half of the lifetime (2026-09-09, found in review before item 115 was
    /// signed):
```

### gungnir-tracking-service/tests/scenario_truth_replay.rs

Line 328:

```text
/// advance. **A three-mode CV/CT/CA IMM is not what DN-28 scoped or what the owner
/// signed**, so this row scores only the phase the signed scope actually covers:
```

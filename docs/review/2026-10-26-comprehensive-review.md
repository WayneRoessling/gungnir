# Gungnir comprehensive review, 2026-10-26

Reviewed at `9e9fe9f5` (`origin/main`, "GAP-015: the dense group on the picture (#223)").
Read-only: no tracked file was changed. This document is the review itself and is not a
record item, a gap entry or a decision; where it names a status it cites the source that
owns it (`docs/README.md` "Keeping the set consistent"). What is or is not built is read
from `docs/unbuilt.md` and the gap register, never from prose, including this prose.

## Scope and method

Seven areas, each reviewed independently against the code and the documents, then
cross-checked:

| Area | What was read | Grade |
|---|---|---|
| Rust practices and code quality | Every `gungnir-*/src`, the workspace manifest, `deny.toml`, a full clippy run | A- |
| System of systems architecture | `ARCHITECTURE.md`, the API contract, UAF and TOGAF sets, DN-16/18/25/31, `gungnir-remote`, `gungnir-node`, `gungnir-api`, `gungnir-interop` | C+ |
| Security in contested and disconnected operations | `gungnir-security`, the TLS path in `gungnir-api` and `gungnir-remote`, the ingest gateway, the journal and audit code, the release pipeline | C+ |
| Small-unit viability | The disconnected profile end to end: config, accounts, adapters, clock, decision loop, effector handoff, resilience | C- |
| Combined-unit viability in integrated command structures | The node profile, peer links, exchange, airspace control, planning, identity | C |
| User interface maturity and usability | `gungnir-ui`, `gungnir-viewport3d`, the desktop wiring, `docs/ux` | C |
| Documentation completeness for a Department of War customer | The whole `docs/` tree against a milestone deliverable list | C+ program office, A- engineering authority |

Grades are relative to what a defense customer would accept for a field experiment (C)
or a program of record (B or above). A grade below C means the area blocks that use.

A verification run accompanied the review (section 2). The GUI was not run and no
usability session was held; those limits are stated where they matter.

## 1. Executive summary

Gungnir is a disciplined, honest, single-node command-and-control system whose engineering
quality is well above what its operational maturity would suggest. The code has zero
`unwrap`, `expect`, `panic` or `unsafe` in production source across roughly 130,000
production lines, three clippy warnings workspace-wide, oracle-gated tracking mathematics,
loom and miri gates, a mutual-TLS transport that refuses to downgrade, and a governance
apparatus (generated unbuilt list, history-derived gap status, dependency-graph test,
UAF views generated from manifests) that a government engineering authority would hold up
as an example.

What it is not yet is a node in anyone else's system. Every finding that reaches High
severity has the same shape: the system works with itself and stops at its own edge.

- **No production track is releasable.** `project_track` and the node's entity projection
  both stamp `Releasability::Internal`, no configuration field sets anything else, and the
  exchange gate withholds `Internal`. Tracks therefore never cross a peer link or an
  exchange agreement outside the tests that build them as `AllPeers`. The two-node
  picture the architecture describes has no producer.
- **No outbound standard.** Cursor-on-Target is designed and pinned but has no code;
  STANAG 4676 is descoped; ASTERIX encode refuses by design; Link 16/JREAP, VMF, OTH-Gold
  and NFFI are not mentioned anywhere. A higher headquarters cannot consume the picture.
- **The decision loop is complete but not operable at the edge.** Weapons control status
  can only be changed by rewriting the baseline and restarting the desktop; effector
  handoff is a generic JSON POST or a radio call; the only account-provisioning path ships
  in the Linux node binary; airspace control is a circle with a no-go flag.
- **The operator interface is a decision-record console attached to a diagnostic plot.**
  No basemap, no hook-on-map, no keyboard operation, no alert acknowledgement, no
  standard symbology, and no usability session has ever run.
- **The disconnected desktop trusts itself.** The audit chain is unkeyed and anchored in a
  journal the same operator account unlocks; reconciliation arbitration ranks a role string
  read from that journal; configuration baselines are not signed although a key purpose
  for it exists.
- **The acquisition document set is missing.** No hazard analysis for a system that gates
  engagement, no accreditation package, no operator manual, no training material, no data
  rights or export statement.

A second, cross-cutting finding: about a dozen customer-facing documents still describe
built subsystems as scaffold (`README.md`, `ARCHITECTURE.md` §8, `deploy/README.md`, the
API contract's preamble, the capabilities crate map that feeds a generated UAF view). The
drift under-claims rather than over-claims, which is the safe direction, but the project's
own rule is that status is never restated in prose, and these predate the rule.

**Overall: C+.** Viable now as a single site or sector air-defense and counter-UAS cell
talking to itself, with a short closure list for a fixed-site field experiment. Not yet
viable as a subordinate or coalition node. The path from here is short because the
designs exist: releasability from configuration, the CoT bearer, a wider identity
vocabulary, polygon airspace measures, and the operator-facing interface work.

## 2. Verification run

Run on this machine on 2026-10-26 against the same commit. Drive D had under 14 GB free, so
no full workspace test was run and every cargo command was scoped with incremental
compilation off (the project's own note on this box).

| Check | Result |
|---|---|
| `cargo check --workspace --all-targets` | Succeeded, 0 compiler warnings; one future-incompatibility notice (`nom` 3.2.1 via `vtkio` 0.6.3 into `gungnir-data`) |
| `cargo clippy --workspace --all-targets` | Exit 0; 3 pedantic warnings (`gungnir-approval/src/queue.rs:95`, two in `gungnir-app/tests/cut_off_and_reconnected.rs`) |
| `gungnir-app` dependency-graph and architecture-compliance tests | 15 passed |
| `gen_unbuilt.py`, `signatures.py check`, `gen_gaps.py --base origin/main`, `build_uaf.py`, `build_catalogue.py`, `validate_tracks`, `record.py check` | All pass; no generated output changed. Run individually because Git Bash is broken on this machine and `check_documents.sh` could not be used |
| Tests in the six human-owned decision and security crates | 210 passed, 0 failed, 3 ignored (cloud key-service tests needing real credentials) |
| CI on `origin/main` for the reviewed commit | `ci` and `cross-document-recheck` success; `bench-regression` pending on the self-hosted runner after 29 minutes (the two previous bench runs were cancelled after about eight hours); `fuzz-nightly` success the same day |
| Open pull requests | One: #224, base `main`, all checks green |
| `cargo deny` on `main` (from the CI log; not installed locally) | advisories, bans, licenses, sources all ok; three advisory ignores no longer match any crate (RUSTSEC-2026-0192, -0194, -0195) |

CI's clippy step reports success with the same three pedantic warnings present, so
pedantic is advisory in CI as the manifest comment says. `cargo tree -d` reports 35
duplicated packages on the normal and build graph, including a second `glow` (0.13 via
`wgpu-hal` 22 beside the shared 0.17) and a second RustCrypto generation (`sha2`/`hmac`
0.11 via `aws-sigv4` into `gungnir-security`); the manifest's "one copy each" notes for
those rows predate the AWS addition.

## 3. Rust practices and code quality (A-)

**Verdict.** Unusually disciplined for a 50-crate, 241,000-line workspace. Production
source (129,695 lines before each file's first `#[cfg(test)]`) contains zero `unwrap()`,
zero `expect()`, zero `panic!`/`todo!`/`unimplemented!`, and zero `unsafe` anywhere in the
tree. The four `unreachable!` each state their invariant. `clippy::all` is denied and
`pedantic` warned workspace-wide and every member opts in. The numerical core uses
Joseph-form updates, symmetrisation after every step, a debug-only Cholesky PSD guard
(`gungnir-core/src/numeric.rs:35`) called after every predict and update, `let Some(..) =
.cholesky() else` fall-through everywhere, `total_cmp` for float ordering, and 276
finiteness guards. Float `==` appears only as degenerate-input guards. Error enums are
`thiserror` throughout (56 public error enums, no `anyhow`). `#[must_use]` appears 1,069
times.

**Findings.**

1. **Medium. Lock-poisoning policy is inconsistent across 198 acquisitions.** 49 sites
   silently skip on poison (`if let Ok(..) = x.lock()`; `gungnir-remote` 21,
   `gungnir-ingest` 12, `gungnir-app` 9, `gungnir-api` 5), 18 more swallow with `.ok()`
   or `unwrap_or_default`, 14 recover with `into_inner`, 9 surface a `Poisoned` error.
   After a poison a link projection or feed-stats sink stops updating with no health
   flag, which sits uneasily beside the rule against a connected state that is not true.
   Example: `gungnir-api/src/transport.rs:312,326,348`. No std lock is held across an
   `.await` (checked in `transport.rs` and `link.rs`). Recommendation: one policy; for
   state a health flag depends on, recover with `into_inner` or mark the link unhealthy.
2. **Medium. Eleven files over 2,000 lines.** `gungnir-config/src/lib.rs` 8,310
   (4,536 production, 40 structs, 255 functions), `gungnir-api/src/transport.rs` 4,192,
   `gungnir-node/src/main.rs` 3,749 (2,648 production), `gungnir-rfs/src/lib.rs` 3,220,
   `gungnir-remote/src/link.rs` 2,816. The 62 `too_many_lines` allows are the same
   symptom. Recommendation: split config by section, transport by route family, and lift
   the node's production code into the library it already references.
3. **Medium. `Result<_, String>` on 107 production sites**, including the human-owned
   TLS identity path (`gungnir-remote/src/identity.rs:419,659,681,699`,
   `gungnir-remote/src/lib.rs:179`). Recommendation: a `thiserror` enum on
   `gungnir-remote`'s public identity and TLS surface first, then the 41 sites in
   `gungnir-app`.
4. **Medium. The human-owned `gungnir-approval` has seven in-crate tests** across 2,359
   lines; `deliveries.rs`, `engagements.rs`, `handoffs.rs` and `lib.rs` have none. It is
   exercised from `gungnir-node/tests/approval_queue.rs` (10 tests, 1,356 lines), so the
   coverage is integration-level. Recommendation: unit tests on the state transitions of
   deliveries and handoffs.
5. **Medium. A blocking poll on the eframe thread.** `gungnir-app/src/laydown_rehearsal.rs:1185-1192`
   sleeps 2 ms up to 5,000 times (10 s worst case) inside `apply(PanelAction)`.
   Recommendation: a worker thread with a result channel, as the point-cloud loader
   already does.
6. **Low. `InProcessBus` gives every subscriber an unbounded channel**
   (`gungnir-eventing/src/lib.rs:177-178`) with no depth metric. Recommendation: a gauge,
   or a bound with a documented drop policy.
7. **Low. Enforcement is by review, not compiler.** No `unsafe_code = "forbid"`, no
   `missing_docs`, no `clippy::unwrap_used`/`expect_used`. Public-item doc coverage is
   80.7% (3,164 of 3,923), lowest in `gungnir-observability` (30%), `gungnir-resilience`
   (42%), `gungnir-modelops` (46%). Thirteen runnable doctests across the tree.
   Recommendation: add the three lints to the workspace manifest so the strongest
   properties of the codebase become guarantees.
8. **Low. Dependency notes have drifted.** The `glow` and RustCrypto duplicates above;
   `copc-rs` and `pasture-*`, named in the standards as unpinned, are not in the
   manifest at all (`las` 0.11 covers COPC).

**What is good, with evidence.** `kalman.rs:174-195` Joseph form; `kalman.rs:179-188`
singular innovation handled by counting and returning; `gungnir-fusion-async/src/sync.rs:95-140`
loom-modelled SPSC queue behind the crossbeam API so the gate drives the real pipeline;
every row in the workspace manifest carries a dated rationale and a duplicate-linkage note;
`deny.toml` denies wildcards and unknown sources; 2,526 test functions and roughly 0.85
test lines per production line.

## 4. System of systems architecture (C+)

**Verdict.** As a single node, well-governed and documented to the line: one crate set
deploys as disconnected desktop, on-prem node or cloud node, and the desktop-to-node seam
(snapshot, sequence-ordered WebSocket stream, bounded history, store-and-forward,
reconciliation with a fixed arbitration rule, node-held approval queue) is built, tested
across a real wire and described in an ICD-grade endpoint table. As a member of a system
of systems it is thin: pairwise mutual-TLS subscriptions with no global track identity,
no cross-node fusion, no cross-node authority, no production track that is releasable, no
outbound standard, and a cloud profile that is the on-prem container on another host.

**The architecture as built.**

- **Profiles.** Disconnected desktop with embedded services and a buffered local journal
  (fsync every 5 s); on-prem node (Linux container, 50 ms tick, fsync per envelope);
  cloud as "posture, not code" with no manifests.
- **The node's external surface** (`gungnir-api/src/lib.rs:59-94`): session minting;
  snapshot, event stream and history for an operator token with `picture.view` or a
  machine party with an exchange agreement (which receives a marking-filtered
  `snapshot_for`); health; coverage and gap acceptances; detection submission (operator
  or a certificate speaking for that sensor; exact schema-version match required);
  sensor tasking; handoff reports; warning acknowledgement; exchange of warnings, reports
  and handoffs; the queue and its decision route; forwarded outage decisions as one batch;
  audit read; every `/v2` route answers 410 naming its successor. Plaintext is refused on
  any non-loopback bind.
- **Event stream.** A bus-wide monotonic sequence, encoded once, 8,192-envelope backlog,
  410 when a client falls behind, 2 s heartbeat, 7 s timeout, per-tick picture diff.
- **Peer links.** Pairwise, pull, one direction per configured entry; the node runs a
  desktop-style client link to each partner's snapshot and stream under a `node-peer`
  identity; mutual exchange needs both sides configured; no hub, discovery or relay;
  warnings are never forwarded to a third party (DN-16 §9). Partner tracks arrive as
  detections with a receiver-assigned quality through the ordinary gateway.
- **Coalition exchange.** Per-party agreements over items (tracks, warnings, reports,
  handoffs, health), two gates (agreement and marking), withheld counts on every answer,
  and three declared formats of which only the canonical JSON has an encoder.
- **Interop as implemented.** Decoders, gated and bound on the node: ASTERIX Cat 048,
  034, 205 and 129; AIS; ADS-B (normative source deliberately unpinned); MISB ST 0601;
  SAPIENT v2 (with task acknowledgements). Encoders: the v3 JSON contract and Arrow.
  Refused or absent: ASTERIX encode (by design; a fused track is not a monoradar report),
  STANAG 4676, Cursor-on-Target (designed, no code), ASTERIX Cat 062 system tracks,
  Link 16/JREAP-C, VMF, OTH-Gold, NFFI, MIL-STD-2525 on the wire.

**Findings.**

1. **High. No production track is releasable, so track exchange is inert.**
   `gungnir-tracking-service/src/lib.rs:355` and `gungnir-node/src/entities.rs:252` both
   stamp `Releasability::default()`, which is `Internal`; `DetectionView` carries no
   marking; `gungnir-config` has no releasability field; the only non-`Internal` track
   stamps in the tree are in test modules; `gungnir-remote/tests/wire_conformance.rs:120,370`
   passes by building tracks as `AllPeers`. DN-17 §5 specified per-sensor default
   markings. Recommendation: a `default_releasability` per deployment and per sensor or
   peer in the baseline, carried through `Provenance` into `project_track`, and a two-node
   test that a real tracker-produced track crosses the wire.
2. **High. No global track identity and no origin guard.** `GlobalEntityId` is minted
   per node and never placed on `TrackView` (`gungnir-node/src/main.rs:2180-2183`);
   `PeerOrigin.remote_track` is the peer's session-local id as a string; the peer adapter
   overwrites `provenance.peer` (`gungnir-ingest/src/adapters/peer.rs:164-171`) and the
   gateway has no loop guard, so two mutually configured peers would re-ingest each
   other's tracks as fresh measurements, a rumour loop that finding 1 currently prevents
   by accident. The covariance-intersection fuser in `gungnir-track-fusion` is unused by
   the peer path. Recommendation: carry the global id and originating node on
   `TrackView`, refuse re-ingest of a lineage containing the receiver, fuse peer tracks
   through covariance intersection, and define a system-track-number scheme.
3. **High. No cross-node authority.** DN-31 §11 scopes shared queues out; `BothActed` is
   keyed by a local `TrackId`. Two nodes with overlapping sectors can each engage the same
   object without either knowing. Recommendation: publish engagement-opened over the
   exchange with the global id, and design sector ownership.
4. **High. Outbound interoperability toward an integrated command structure is absent**
   (table above). Recommendation: build the CoT codec and sinks first (every artifact is
   pinned under D-33; the open step is a recording), then ASTERIX Cat 062 encode, and
   file an explicit decision on Link 16/JREAP-C and VMF rather than leaving them
   unmentioned.
5. **Medium. The cloud profile has no scaling or availability design.** One
   single-threaded loop owns ingest, tracking, planning, policy, queue, audit, coverage
   (recomputed every tick, `main.rs:2236`), journal and publish; everything is in memory;
   tokens are minted in-process; no replication, leader election or manifests.
   Recommendation: a cloud design note, or a recorded decision that scaling is vertical
   and one node per sector.
6. **Medium. Node budgets are unevidenced beyond ingest and fsync.** The budgets document
   names 5,000 detections per second, p99 150 ms to stream, 10 desktops, 50 ms snapshot at
   500 tracks; the only node harness measures ingest and fsync with the tracker stubbed.
   No fan-out, snapshot-latency, recovery-time or multi-desktop harness exists.
7. **Medium. The shipped example node config does not serve.** `deploy/node/config.example.json`
   binds `0.0.0.0:7410` and, without the TLS environment, `transport::bind` refuses a
   non-loopback address; the README's "nothing listens" sentence is true for a stale
   reason.
8. **Medium. No metrics export and no signed baselines.** Health is three booleans and
   tracing counters; no Prometheus or OpenTelemetry anywhere; the UAF registry describes
   "metrics, structured logs, distributed traces" that do not exist. `KeyPurpose::BaselineSigning`
   is declared and never used for a signature.
9. **Low-Medium. UAF standards taxonomy and several views are stale**: `Sd-Tx.md` marks
   six built standards planned; the resource-to-standard matrix dates from 2026-09-04;
   `Op-Cn.md` admits its "carried today" column was not re-checked; `conforms_to` is
   hand-authored rather than derived from `SchemaCatalog`.

**What a higher echelon would need and does not get.** J-series over JREAP-C, VMF
K-series, OTH-Gold or NFFI blue-force feeds, USMTF, CoT, Cat 062; ACO/ACM ingestion and
ROZ or kill-box geometry; fire support coordination measures and an AFATDS exchange; a blue
order of battle, task organisation or unit identity (the built order of battle is a lineage
of detected entities); COP publication beyond a snapshot under agreement with every track
`Internal`; weapons-control or authority exchange with a neighbour; a disciplined time
source for a federation; and a classification model, which DN-17 §5 declines by design.

## 5. Security in contested and disconnected operations (C+)

**Verdict.** The transport and credential core is well built: mutual TLS required rather
than requested (`gungnir-api/src/tls.rs:174-178`); plaintext refused on any non-loopback
bind; an `https` link with no pinned roots refused rather than downgraded
(`gungnir-remote/src/link.rs:926-933`); argon2id PHC passphrases with constant-time,
enumeration-resistant verification; HMAC-SHA256 tokens verified in constant time before
parsing; a hash-chained audit log with heads anchored outside the file; no secrets in the
repository; `Debug` impls that redact; a non-root container; a release pipeline with
CycloneDX, `cargo auditable`, keyless cosign and dated advisory ignores. Health flags are
honest everywhere looked. What holds it at C+ is the disconnected-desktop threat model,
where the operator's own account is the trust root for everything on the machine.

**Findings.**

1. **High. Audit integrity on the desktop rests on an unkeyed chain anchored in a journal
   the operator unlocks.** `gungnir-security/src/audit/anchor.rs:12-16` states that the
   chain is unkeyed and anyone who can write the file can recompute it, and that the head
   lives in the journal; the journal's sealing key is unlocked from the interactive
   operator's OS keystore. Recommendation: sign anchors with a key purpose the operator
   session cannot unlock, and push anchors to the node when linked.
2. **High. Reconciliation arbitration ranks a self-asserted role string from the local
   journal.** `gungnir-model/src/arbitration.rs:27-30,52-54` takes the role "the side was
   decided under" as recorded; `gungnir-command/src/lib.rs:149-156` records it as the
   caller supplied it. A forged desktop journal can win a decision conflict on reconnect.
   Replay is caught by the request-id key; fabrication is not. Recommendation: the node
   re-derives the role for a forwarded decision from its own account store and refuses a
   mismatch.
3. **High. Configuration baselines are not signed.** `ARCHITECTURE.md` §8.5 says a node
   needs signed baselines; `KeyPurpose::BaselineSigning` exists; nothing produces or
   verifies a signature. The baseline carries the authority rules, the sensor allow-list,
   the exchange agreements and the escrow public key, making it the highest-value tamper
   target on a disconnected host.
4. **Medium. Issued TLS identities never expire and nothing revokes them.**
   `gungnir-remote/src/identity.rs:356` builds certificate parameters with no validity
   window; the verifier has no CRLs; identities are self-signed and pinned.
5. **Medium. The node's token signing key is an environment string** with a length check
   only (`gungnir-node/src/auth.rs`, `token.rs:79-83`); no generation guidance in the
   deployment README.
6. **Medium. Session tokens carry the role and are not re-checked** against the account
   store (`token.rs:43-51,154-159`); no revocation set; not bound to the client
   certificate.
7. **Medium. The disconnected session never expires by default**, and every expiry is
   on the wall clock; a set-back clock re-accepts an expired token; no idle timeout.
8. **Medium. Sign-in back-off is keyed by operator id, not source**, and the session route
   is the one route reachable without a token, so an unauthenticated caller with a valid
   certificate can keep a named operator in back-off.
9. **Medium. Five of seven ingest parsers have no fuzz target.** The three targets cover
   the ingestion parser, the ASTERIX feed and cost-matrix construction; SAPIENT (78 KB of
   parser), MISB, ADS-B, AIS and the Cat 129/205 decoders are not fuzzed. The nightly
   found a real defect on its first run (closed as GAP-103) and the MISB BER-length stall
   was found only in human review. `AllowAllAuthenticator` is documented as never for a
   deployed profile but nothing refuses it in a node baseline.
10. **Medium. Sealed files inherit directory permissions on Windows**; the only
    permission handling is Unix-only for the PHC file; nothing is zeroized in memory; the
    default data directory is under the working directory.
11. **Low-Medium. Transport hardening.** No request or idle timeouts, no connection cap,
    the WebSocket upgrades before any credential and waits for the subscribe frame
    without a deadline, rustls defaults (TLS 1.2 still negotiable, no FIPS provider). All
    behind mutual TLS, which is why this is not higher.
12. **Low. No separation of duties.** `Supervisor` holds submit, decide, override and
    set-control-status together; no two-person rule exists. The matrix itself is complete
    and tested cell by cell.
13. **Low. Journal lines are sealed but not chained**; the `Ephemeral` key mode on a node
    warns that the record is unreadable after exit but is not refused.
14. **Low. Outbox drops are counted, not audited.**
15. **Low. Releasability is enforced at the boundary and lowering is prevented only because
    no lowering path is built**; the model states it is not a classification system.
16. **Low. Supply chain residuals.** GitHub actions pinned by tag rather than SHA; the
    container image is built but not pushed or signed; no rebuild-and-compare.
17. **Low. One stale honesty claim.** `ARCHITECTURE.md:755` says the tokens and TLS are
    not built and the node serves loopback only; both are built. No health flag or test
    was found claiming a subsystem works when it does not.

## 6. Small-unit operations (C-)

**Verdict.** The disconnected desktop is real: one binary embeds the services layer,
journals to local disk with a torn-tail-tolerant log, runs the queue, authority, expiry
and escalation locally, and needs no node. The 2D picture is the default, so a discrete
GPU is not actually required. Resilience is the strongest area: mid-session failover on
the node's heartbeat is built (`gungnir-app/src/failover.rs`), store-and-forward exists for
detections, exchange items and the outage's decisions, and an interrupted session is
reported as such. What stops a field trial is the edges.

**Findings.**

1. **High. Organic small-unit sensors reach it only through SAPIENT or MISB KLV.** The
   built adapters are ASTERIX, SAPIENT, AIS, ADS-B, MISB, peer, recorded and simulated.
   EO/IR enters only as platform metadata, not detections; a vendor radar on its own UDP
   format, an RF detector with its own JSON, or a camera with analytics has no adapter.
   A lone RF detector yields bearing rays that never become a track (DN-27, by design).
   Consequence: a SAPIENT-conformant kit can feed the picture; a kit that is not cannot
   without an adapter nobody has written.
2. **High. Weapons control status is not operable and the effector handoff is a POST or a
   radio call.** `WeaponsControlStatus` defaults to `Hold`; the only write path is a
   baseline apply that "takes effect when the desktop restarts" (`gungnir-app/src/main.rs:1039`);
   there is no API route and no panel control (US-07 is blocked). Delivery is `Manual` or
   a generic JSON endpoint; no vendor driver exists; the desktop binds no listener, so no
   effector outcome ever comes back by data in the disconnected profile. One layer at
   `Hold` refuses every plan on every layer (GAP-183, kept by D-114). An operator who must
   go from Hold to Tight when a raid starts edits JSON and restarts.
3. **High for a mobile unit, Low for a fixed site. No own-position and no GNSS concept.**
   `WallClockAuthority::now` is `SystemTime::now()`; skew is estimated per source and a
   source beyond the late-data buffer (default 1 s) is dropped. The frame origin and
   every sensor position are static baseline entries. A free-running laptop clock against
   a GPS-disciplined radar drifts past 1 s within hours.
4. **Medium. Account provisioning needs the node binary.** The only writer of the
   accounts file is `gungnir-node account add`; the release ships the desktop for Windows
   and the node for Linux only. The desktop runs with nobody signed in and can decide in
   embedded mode, but those decisions record no role, which later makes arbitration unable
   to rank them.
5. **Medium. Operator load is designed for a command post.** Nine roles, twenty-one
   panels, personas that all sit behind a supervisor. A Commander account holds decide,
   override, control-status and apply-config and can run the live loop alone; sensor
   tasking needs a second role.
6. **Low. Hardware is fine; power is unaddressed.** No "battery" or "low-power" text
   anywhere in `docs/`; 30 Hz redraw regardless of activity.
7. **Low. Deployment prose is stale** in four `ARCHITECTURE.md` §8 sentences and
   `deploy/README.md:47-48`; a field installer reading them would set the system up wrong.

**Minimum closure for a fixed-site counter-UAS field experiment** (SAPIENT sensors, a
radio-called effector): the control-status write path; desktop account provisioning or
shipping the node binary beside it; clock-discipline guidance and a wider late-data
buffer; one adapter for the trial's radar if it is not SAPIENT or ASTERIX; the stale
deployment prose. Nothing else in the open register blocks that experiment.

## 7. Combined-unit operations in integrated command structures (C)

**Verdict.** Between instances of Gungnir the shared picture is built and gated: one node
holds one queue for every linked desktop, first valid decision wins with full attribution,
every decision streams to every desktop, operators authenticate by token and machines by
mutual TLS, peers link with receiver-assigned quality, and exchange products are gated per
party. The moment the partner is not Gungnir, integration stops. Viable today as a sector
or site-defense cell sharing one node among its own desktops; not yet a credible node under
a brigade, division or joint headquarters, nor a coalition participant.

**Findings.**

1. **High. Higher and adjacent integration exists only Gungnir-to-Gungnir** (section 4,
   findings 1 to 4). The built order of battle is an enemy order of battle of observed
   entities; no `Unit`, `Echelon` or friendly task-organisation type exists.
2. **High. Airspace control measures and rules of engagement are thin.** `Geofence` is a
   circle with a `no_go` flag whose `contains` ignores altitude
   (`gungnir-geo/src/lib.rs:41-50`); `LayerKind::NoFireArea` exists but the fires chain
   receives airspace measures and no-fire areas as `None` with "no source"
   (`gungnir-approval/src/chain.rs:173-192`), so those two checks are permanently "could
   not be evaluated". Weapons control status is per effector layer, not per zone; no
   engagement zone, ROZ, coordinating altitude or kill-box type exists.
3. **High. The common operational picture cannot be published to higher in any standard.**
   Outbound is the JSON stream, exchange items and a JSON report file.
   `docs/mission/roles-and-stakeholders.md:117` promises partners STANAG 4676 and
   ASTERIX; neither exists.
4. **Medium. Planning is laydown and baseline, not orders.** Laydown options with coverage
   comparison and rehearsal, battle-rhythm products and handover, and collection
   requirements and tasking are built; no OPORD/FRAGO, named area of interest, decision
   point or collection plan concept exists.
5. **Medium. Identity is four states and there is no IFF.** `Classification { Unknown,
   Neutral, Friendly, Hostile }` (`gungnir-model/src/lib.rs:301-307`); STANAG 1241's
   Pending, Assumed Friend, Suspect, Joker and Faker are absent; the lethality weight of
   `Unknown` is 0.8, which an allied interrogator would read as "suspect" without the
   word. IFF Mode 4/5 appears only in the test-track sensor models.
6. **Low. Multi-operator on one node is built and gated** (DN-31 §6, GAP-130 to 134).
   There is no per-cell scoping: an air cell and a fires cell on one node see one queue.

**Echelon verdict.** Viable now as a single site or sector air and counter-UAS cell.
Shortest path to a seat in an integrated structure: widen the identity vocabulary, then
the CoT bearer, then polygon airspace measures with altitude bands, in that order, since
each is a prerequisite for the next.

## 8. User interface maturity and usability (C)

**Verdict.** Design B+, operator-facing code C-, validation absent. The design set under
`docs/ux` is unusually complete and honest: eight personas, eight task analyses, a
21-panel information architecture, a token-level design system with WCAG contrast gated by
tests, and a usability plan that refuses to invent numbers. The panel code follows the UI
standards: every empty or unavailable state is a typed variant with a sentence that a
headless render probe (57 render tests) proves reaches the screen; panels return actions
rather than mutating state; the decision dialog's "accept is never the default" property
is enforced four ways and state-space tested; docking and multi-window are built properly
with per-role layouts persisted in the baseline; link loss, tracker health, clock skew and
the empty queue are all explained on screen (the `EmptyBecause` variant that
`ARCHITECTURE.md` §7.3 asks for is built, though that paragraph still says the pipeline is
not implemented). What is thin is the part an operator lives in.

**Findings.**

1. **High. The alert lifecycle is not wired; the alert panel is a string list.**
   `AppState.alerts: Vec<String>` (`gungnir-app/src/state.rs:265`); `AlertLifecycle` with
   new, acknowledged, escalated and closed exists in `gungnir-workflow` and is used only by
   its own test; no acknowledge, escalate or close control exists.
2. **High. No keyboard operation at all.** Zero hits for `egui::Key`, `Modifiers`,
   `KeyboardShortcut` or `consume_key` across the four UI crates; `docs/ux/accessibility.md`
   specifies five shortcuts and a tab order; the keyboard-only usability measure cannot be
   anything but "not attempted".
3. **High. The map cannot select or hook a track and draws no defended assets or
   effectors.** The viewport consumes drag and scroll only (`gungnir-viewport3d/src/interaction.rs:61-70`);
   the only click in the viewport is the 2D/3D toggle; selection originates only in the
   track table; no selection halo is drawn although the colour token exists; the layer
   inputs carry no asset or resource layer. The commander persona names "being asked to
   accept a gap without seeing it on the map" as a frustration.
4. **High. No usability data.** The round-1 report has every measured field blank; the
   one walkthrough was by the owner for all eight roles ("one person who also wrote the
   requirements is not eight users"); GAP-074 is in progress; no MOP-37 target has a value.
5. **Medium. The three-d scene is the default and has never been seen.** `gl.rs:29-31`
   says the drawn result has not been looked at; `ui_settings.rs:232` defaults it on;
   `main.rs:575` says the opposite of `lib.rs:18-20`.
6. **Medium. No basemap, no standard symbology, no declutter.** The picture is a top-down
   ENU projection with a metric grid; no tiles or imagery; no lat/lon or MGRS cursor
   readout; custom frame-by-affiliation glyphs rather than MIL-STD-2525/APP-6; labels
   overprint at a dense picture; 3D Tiles streaming is three `NotImplemented` stubs.
7. **Medium. The track table is not virtualised and the IA document says it is.**
   `track_table.rs:99-106` is an `egui::Grid` in a 320 px scroll area; the 8 ms egui
   budget test renders the commander workspace, which has no table.
8. **Medium. No weapons-control-status control and no manual escalation** (US-07 blocked).
9. **Low. Units are metric ENU only** (no lat/lon, MGRS, feet, knots, mils); time is
   implicitly UTC and never labelled Z; no localisation beyond the domain vocabulary table.
10. **Low. Theme and type are fixed**: day and night palettes with no runtime switch, no
    NVG or red-light mode, 14 pt fixed with no zoom control; accesskit is on but nothing
    adds accessible names beyond label text.
11. **Low. UX documents have drifted**: the wireframe index marks seven built panels as
    not in code; the IA marks PN-16 unbuilt.

**Against what a defense user expects.** A FAAD C2 or IBCS operator expects a geographic
map with standard symbols, hook-and-act on the symbol, engagement zones and assets drawn,
weapons control changeable from the console, an alert cadence with acknowledge, and a
keyboard-driven engage/hold path. Gungnir has the decision discipline those systems lack
(verdict, engines, caveats, degraded acknowledgement, who decided first on a shared queue,
honest empty states) and none of the map vocabulary. A TAK user would miss the map and
the plugin model; a Lattice user the fused-entity timeline and the assistant.

## 9. Documentation completeness for a Department of War customer (C+ / A-)

**Verdict.** From an engineering authority's seat, A-: a verification table naming the
oracle actually run and the worst measured disagreement per row; a 184-gap register and
133-decision ledger generated from YAML with append-only history enforced in CI; a
signature ledger; 227 never-edited record items; UAF views generated from manifests with
code paths checked to exist, exported and round-tripped through Sparx EA; a DoDAF
cross-reference; an SBOM, signing and advisory policy; and a generated list of what is
not built. From a program office's seat, C+: the milestone deliverables are mostly
missing, and a dozen customer-first documents describe built subsystems as scaffold.

| Expected deliverable | Present | Where, and the gap |
|---|---|---|
| CONOPS / OV-1 | Partial | Strong mission threads and vignettes approved by a subject-matter expert; no document titled CONOPS; no OV-1 graphic (the cross-reference maps OV-1 to a needline table) |
| Requirements traced to tests | Partial | 64 REQ ids and 67 "shall" statements across two documents; verification column generic; the spec admits 27 of 56 leaf capabilities have no requirement |
| Interface control documents | Partial | The node API is ICD-grade; sensor interfaces have spec pinning but no per-interface ICD (message set, field mapping, rates, limits) |
| System design description | Yes | Exceeds the norm; two ADD sections stale |
| Software version description, release notes | No | The release-governance document still says no gate result is recorded anywhere |
| Installation guide | Partial | Node container, identities and geoid grids thorough; the desktop gets one build line; account provisioning only in `CLAUDE.md`; no Kubernetes manifests |
| Operator manual | No | `docs/ux` is design, not a manual |
| Training material | No | `uaf/personnel/Pr-Rm.md:10` says so |
| Test plans, procedures, reports | Partial | The verification table is a spec with results overwritten in place, not a dated report; §2 rows draft; usability report blank |
| System safety / hazard analysis | No, and critical | Zero occurrences of MIL-STD-882, hazard analysis or FMEA; the mitigations exist, scattered across DN-09, GAP-183, contract C-01 and the no-execution-without-decision test, with no hazard list behind them |
| Cybersecurity accreditation package | No | Zero occurrences of NIST SP 800-53, RMF, ATO, STIG, FedRAMP or impact level; the mechanisms are better documented than most |
| SBOM and license posture | Yes | CycloneDX, `cargo auditable`, cosign; the AGPL network clause stated plainly; commercial path documented; the additional terms say they have not been reviewed by counsel |
| Data rights (DFARS 252.227-7014) | No | Zero occurrences |
| Export control | No | An outline item in the business plan; the test-track sourcing review deferred |
| Section 508 | Partial | An honest design-level accessibility statement; no VPAT |
| Sustainment plan | No | Pieces only |
| Glossary | Yes | Two, mapped to AV-2 |

**Findings.**

1. **High. No system safety assessment for the engagement authority path.**
2. **High. No accreditation package.** An SSP skeleton mapping the existing mechanisms to
   800-53 families with "not addressed" stated per control is the first step.
3. **High. No operator manual, training or desktop installation guide.** A role-keyed
   guide could be generated from the panel catalogue and task analyses, which already name
   every panel and field.
4. **High. No data rights, export control or counsel-reviewed license statement.**
5. **Medium. Customer-first prose contradicts the generated sources.** `README.md:123-129`
   lists CPHD, LMB, `gungnir-ml` and STANAG 4676 as scaffold (the register and
   `docs/unbuilt.md` say otherwise); `deploy/README.md:47-48`; the API contract's preamble
   says `serve` refuses and there is no TLS while its own endpoint table says every route
   serves; `ARCHITECTURE.md` lines 674, 690, 706, 724 and 755; the capabilities crate map
   ("All 51 crates", allocator "returns NotImplemented", filters "six trait surfaces",
   node "no network endpoints yet"), which the generated `uaf/resources/Rs-Sr.md` copies
   verbatim under a do-not-edit banner; the ADD ("Eighty-three gaps", "Fifty-eight
   requirements"); `uaf/summary-and-overview.md` ("4 fully implemented"). The CI document
   checks verify generated outputs and append-only history, never prose claims, which is
   why these survived.
6. **Medium. `GungnirOverview.md` and its PDF are a dated self-audit**, 46 days and
   roughly 130 commits behind; four of five spot-checked claims are stale.
7. **Medium. Interface documentation is one ICD plus spec-pinning notes**; `Sd-Tx.md`
   marks built standards planned.
8. **Medium. Verification evidence is strong; a dated test report is not produced.**
9. **Low. The capability assessment scores stakeholder engagement one**: "no operator has
   used the product, no buyer has quoted a price, and no accreditor has seen the evidence
   package." Honest, and the risk a program office will read first.
10. **Low. The writing is developer and agent facing**; ten of the twenty-one index rows
    name agents as an audience.

## 10. Cross-cutting observations

- **The honesty value holds, in the unusual direction.** Every area found the same thing:
  the code and the generated sources tell the truth, and the prose that is wrong
  under-claims. No health flag, test or generated document was found overstating what
  works. The failure is the one the 2026-09-16 rule predicted and could not retroactively
  fix: sentences written before it were never swept.
- **One edge, five views.** The releasability default, the four-state identity enum, the
  missing CoT bearer, the circle geofence and the unsigned baseline are each small; together
  they are the boundary between a system that works and a system that participates.
- **The governance is a selling point.** A generated unbuilt list, history-derived gap
  status, a dependency test, UAF views from manifests, and record items that are never
  edited are what a government engineering authority asks for and rarely gets. The gap is
  that the acquisition documents (safety, accreditation, manuals, rights) are not yet
  generated from, or even attached to, that apparatus.
- **Human-owned crates are where the test depth is thinnest in-crate** (`gungnir-approval`)
  and where the string-typed errors sit (`gungnir-remote` identity). Both are review-cost
  issues rather than defects, and both are cheap.

## 11. Consolidated actions, in order

The order is chosen so each step unblocks the next and the earliest steps are the smallest.

1. **Sweep the stale prose** in `README.md`, `ARCHITECTURE.md` §8, `deploy/README.md`,
   the API contract's preamble, the capabilities crate map, the ADD, `Sm-Ov`, `Sd-Tx`
   and `Op-Cn`; replace status sentences with pointers to the generated sources; ship an
   example node config that serves. One change, no engineering, and every later reader
   benefits.
2. **Make tracks releasable from configuration** per DN-17 §5 and §6, carried through
   provenance into the projection, with a two-node test that a real track crosses.
3. **Build the weapons-control-status write path** (US-07): live, audited, on the
   desktop and through the node, no restart. This alone blocks every small-unit use.
4. **Close the desktop trust gaps**: sign audit anchors with a purpose the operator cannot
   unlock; re-derive the role of forwarded decisions on the node; sign baselines on the
   purpose that already exists; put validity and revocation on issued certificates; move
   the token key into the key provider.
5. **Widen `Classification` to STANAG 1241**, then **build the Cursor-on-Target bearer**
   (DN-25, everything pinned), then **polygon airspace measures with altitude bands** feeding
   both chains, and weapons control by zone. File an explicit decision on Link 16/JREAP-C
   and VMF.
6. **Put a global identity and originating node on `TrackView`**, add the re-ingest guard,
   and fuse peer tracks through covariance intersection; write the cloud and multi-node
   design note or record that scaling is vertical.
7. **Operator interface**: wire the alert lifecycle; map hit-testing, selection halo and an
   asset/effector layer; the five keyboard shortcuts with render-probe tests; default the
   three-d scene off until it has been seen; virtualise the track table; then run a
   usability round with non-author participants and set provisional targets.
8. **Desktop provisioning**: an `account add` path reachable on Windows, or the node
   binary shipped beside the desktop, and a clock-discipline note for GNSS-denied sites.
9. **Acquisition documents**: the hazard analysis seeded from DN-09, DN-10, DN-31 and
   GAP-183; an SSP skeleton with an 800-53 mapping; a role-keyed operator guide generated
   from the panel catalogue; a rights, export and license-status page; a dated per-release
   test report and version description in the release evidence package.
10. **Code hygiene**: one lock-poisoning policy; split the three largest files; `thiserror`
    on the remote identity surface; unit tests in `gungnir-approval`; `unsafe_code = "forbid"`,
    `missing_docs = "warn"` and `unwrap_used`/`expect_used` denied in the workspace
    manifest; fuzz targets for SAPIENT, MISB, ADS-B, AIS and Cat 129/205; refresh the
    duplicate-linkage notes for `glow` and the RustCrypto rows.

## 12. Limits of this review

The GUI was not run; interface findings come from the code and the headless render tests.
No full workspace test was run on this machine (disk); the human-owned crates and the
architecture gates were run and CI on the same commit is green. No usability session was
held, and none has been. No penetration test was performed; security findings are from
reading the code. The business documents are excluded from the repository by the owner's
decision and were not assessed. Timing numbers from this machine are not reported as
findings, per the project's note on its hybrid cores.

# The GAP-067 walk of 2026-09-16

**The owner walked every remaining `verification-capability-table.md` §2 row that had a
test, one row at a time, and GAP-067 closes with it.** Group B's six rows were settled on
2026-09-15 (item 132). This sitting took the rest of the walk sheet: Group A's thirty-four
rows, Group C's three, Group D's one, the four Group E rows that name a test, the Group E
concurrence row whose blocker had expired, and the egui budget Group B left for the owner
to read. **Thirty-two rows gated** -- three of them the checked halves of rows split so that the unchecked half could wait -- and **eleven held**, each on a gap filed today. Group C's SSE and tileset row is unbuilt rather than untested, and stays engineering. Nineteen gaps were filed, GAP-109 to GAP-127, and two decisions, D-53 and D-54. GAP-067 closes.

## How the walk ran

**The evidence was read before the questions were asked.** Six read-only reviews took the
rows in sets and read each row's cited tests against its criterion on `main` at `5ea5e09`,
clause by clause: covered, partly covered or not covered, each with the assertion that
showed it, and for every row a verdict of gate, gate if amended, or hold. The claims the
questions leaned on were checked again before they were put: the replay's agreement and the
float round trip were measured, the desktop's pinned role and the identity re-minting were
read in the code, and the GPU dispatch's logs were read on the run itself.

**One question per row, four at a time, with a recommended answer first.** Every row got an
answer from the owner, who took the recommendation on every question but three, and the
three are the largest changes this walk makes: to **build D-03's arbitration rule into
reconciliation** rather than record that a person resolves every conflict; once the code
showed that a journaled decision carried no role and the desktop's role never followed
sign-in, to build both of those in the same change rather than file them; and to
**enforce the operator on a tasking concurrence and build the MT-08 replay** rather than
hold the row on a gap.

**"Write the test, then gate" meant exactly that.** A row the owner gated on a test that
did not yet exist was gated only when the test was written in this change and passed.
Every one of those tests passed in the end, and none by weakening: three passed only after the defect it found was fixed (below), and one criterion was checked more strictly than the first draft of its test did.

**The owner's signatures are in the ledger, not here.** Every row the owner gated or
whose criterion the owner amended is an entry in [`../../signatures.md`](../../signatures.md) dated
2026-09-16, naming `5ea5e09` -- the `main` the evidence was read against -- and this item.
A row held is not a signature and has no entry; the gap it waits on says what it waits
for.

## What each row came to

In the order the questions were put. "Gated" means gated in this change; "held" means the
row stays Draft on the gap named.

| Row | What the evidence showed | The owner's answer | What changed |
|---|---|---|---|
| `gungnir-ui` No duplicate state; no allocation in `ui()` | The egui pass had been measured once, 2026-09-15: p99 386 microseconds release against 8 ms | Gate in release | `the_egui_pass_is_measured` became `the_egui_pass_meets_its_budget`, asserted under `--release` in `ci.yml`'s budget step and printed in debug. **Gated** |
| Group C: `gungnir-render`, `gungnir-viewport3d` SSE and tileset, `gungnir-node` headless loop | Nothing counts per-frame resource creation; SSE and tileset traversal are unbuilt; only a person runs the node's loop | Stay Draft, and file two gaps | GAP-109 (the render counter), GAP-110 (the node's loop). SSE and tileset traversal are engineering, not a walk item |
| `gungnir-data-fusion` GPU path against CPU reference | `gpu-fusion.yml` run 35113741715 on `main` at `6374649` executed all four GPU tests on `gungnir-rtx-5060ti`, and they passed | Gate on that run | Gated the same day by #124 (`gpu-verification-row-gated-on-the-recorded-dispatch.md`); this change adds the annotation to the row's criterion cell |
| How to record the walk | The table of tests had an empty "Owner confirmation" column | The ledger, with the table pointing there | Each gate or amendment is a ledger entry; the table of tests moved here (below) |
| `gungnir-security` Authentication, authorization, audit | `AuthenticationFailed` is constructed nowhere; no role-by-action matrix test; no one-entry-per-act test; the node writes no audit entry | Hold, amend, file a gap | First clause amended to `AuthFailure::Rejected`, identical for an unknown operator and a wrong passphrase. Held on GAP-111 |
| `gungnir-api` Contract compatibility and authorization | The method named v1 and a `Security` error; the build speaks v2 and refuses with 401, 403 or 503; no populated snapshot is round-tripped | Hold, amend, file a gap | Method and criterion amended to the v2 interface. Held on GAP-112 |
| `gungnir-policy` No-go and authority enforcement | A role with no matching rule is `Denied { Authority }` under DN-09 §5, not `RequiresHumanApproval`; nothing escalates an under-authority plan (DN-09 §7) | Amend, gate, file a gap | Criterion amended to the three verdicts the tests assert. **Gated.** Escalation is GAP-113 |
| `gungnir-command` Decision recording | No test counted records per decision or checked a failed decide | Write the test, then gate | `every_decide_appends_exactly_one_record_carrying_that_decision`. **Gated** |
| `gungnir-decision` Alternatives and what-if | The table of tests named `lib.rs`, whose tests touch neither `recommend` nor `what_if`; the evidence is in `alternatives.rs` | Fix the pointer and gate | **Gated** on `alternatives.rs` and `gungnir-app/tests/alternatives.rs` |
| `gungnir-collab` Authority arbitration; stale envelopes | The expiry rule (DN-10 §9) is built and tested but missing from the criterion | Amend and gate | Criterion and method amended to put the expiry rule first. **Gated** |
| The desktop's authority role | `AppState.role` is fixed at `Operator`; signing in never changes it, and a test pinned that | File a gap -- superseded in batch 11 by building it here | The role follows the signed-in account (below) |
| `gungnir-config` Validation and version gating | "Nothing applied" was never asserted | Write the test, then gate | Three tests; and two defects the tests' author found and this change fixed: `load` parsed the whole file before reading its version, so a newer schema that reshaped a known field read as `Encoding`, and `validate` judged security before the version. **Gated** |
| `gungnir-modelops` Promotion gating and rollback | The promoted default's configuration was never compared with the one the baseline carried | Write the assertion, then gate | Asserted in `gungnir-app/tests/governance.rs` and in `gungnir-modelops`. **Gated** |
| `gungnir-workflow` Role layouts; alert lifecycle | Only `Analyst`'s docked panels and the history's length were asserted | Write the assertions, then gate | Both analyst roles may not open the approval queue; the history is compared entry by entry. **Gated** on the crate's own types |
| `gungnir-time` Late-data policy; replay determinism | Replay determinism is covered; nothing consumes `LateDataPolicy` | Split: gate the clock, gap the policy | Row split. Replay determinism **gated**; the late-data policy held on GAP-114 |
| `gungnir-data` Loading off the UI thread | "Structural" is not checkable; the tests that carry the method are in `gungnir-data/tests` and `gungnir-app/tests/pointcloud.rs` | Amend and gate | Criterion, data source and tests named. Checked by review for this change: `gungnir-app/src` reaches the loaders only through `spawn_loader` and reads results only with `try_recv`. **Gated** |
| `gungnir-ingest` ASTERIX, direction-finder and UAS-identification adapter | Through `tick`, only a NaN and an unlisted sensor were asserted refused; the fuzz target never reached the 205/129 mapping | Write the tests, then gate | Clause 1 amended to "zero malformed or unauthenticated payloads". `quarantine_rules.rs` feeds one payload for each of `validate_detection`'s eleven rules, and one from an unlisted sensor, through `tick`, with a guard that fails when a rule is added without a row. The fuzz target and `asterix_seeds.rs` bind a direction finder and a UAS gateway, and the two hand-built datagrams are seeds. **Gated.** `cargo fuzz` is not installed here, so the target was type-checked with libFuzzer's macro stubbed; the nightly runs the bindings |
| `gungnir-ingest` SAPIENT adapter | The `NotAPosition` clause was true only of the test's double since GAP-001 | Amend and gate | Criterion and method amended; the test's stale comments corrected. **Gated** |
| `gungnir-ingest` UAS KLV metadata adapter | 3 of 17 report fields checked; never a position-less frame; `frames_decoded` counts checksum-invalid frames | Amend, write the test, then gate | "Every decoded frame" amended to "every checksum-valid frame". The fixture with its checksum corrected to 0x3E1E delivers a report whose seventeen fields match klvdata's, and the frame rebuilt without tags 13 and 14 delivers one report with no platform position. **Gated** |
| `gungnir-sensor-management` Outbound control and acknowledgement | Every clause asserted; the refusal is built as `NotControllable`; the node never sweeps its tasks | Amend, gate, file the node gap | Criterion amended. **Gated.** The node's sweep is GAP-115 |
| `gungnir-data` Loader correctness per format | Bounds exact only for LAS and DEM; no LAZ loaded; no truncated COPC or LAZ | Write the tests, then gate | A LAZ file written from the LAS fixture; exact bounds for VTK, glTF and the COPC query, the COPC figures taken from an independent front-to-back read of all 10,653,336 points; and every truncation of the LAZ file and six of the COPC file, each a `DataError`. **Gated**, with a point cloud's bounds exact to the `f32` offsets `PointBuffer` stores |
| `gungnir-interop` Arrow, catalogue, ASTERIX decode | Seven clauses unasserted or partly asserted; the criterion said "all tested" | Hold, fix the pointer, file a gap | "All tested" struck. Held on GAP-116 |
| `gungnir-model` Schema round-trip and versioning | 20 of 21 event enums and three view types never round-tripped | Hold, add `machine.rs`, file a gap | Held on GAP-117, which names `machine.rs` for the older-version half |
| `gungnir-geo` Geofence containment | Every point kilometres from an edge; Scenario 5's near-pole geometry unused | Write the tests, then gate | A fence at 89.9 N on the antimeridian, and points a metre either side of five fences' edges on twelve bearings, each checked against a distance computed another way. **Gated** |
| `gungnir-analytics` Line-of-sight and coverage | Line of sight covered, in `lib.rs`; coverage accuracy untested, and `CoverageVolume` has no azimuth sector | Split: gate line of sight, gap accuracy | Row split. Line of sight **gated**; coverage accuracy held on GAP-118 |
| `gungnir-viewport3d` Glyph rebuild only on change | The comparator ignored classification, association confidence and velocity; no covariance case | Fix the comparator, test, gate | The tests came first, and three of the four new cases failed on the old comparator. It now compares whole glyphs, so a field added later is compared without anyone remembering to. **Gated** |
| `gungnir-reporting` Traceability | Marking, time span and summary never compared on recomputation; no Scenario 1 recording exists | Amend the data source, write the test, gate | Data source corrected. A report exported from a session journal equals, whole, the report regenerated from that journal reopened on disk. **Gated** |
| `gungnir-eventing` Broadcast delivery and ordering | All three clauses asserted | Gate | **Gated** |
| `gungnir-tracking-service` Whole-pipeline replay | Measured in this walk: all 78 final tracks bit-identical to `run_batch`, release and debug; `accepted` equals submitted and `too_late` is 0 in every scenario, and neither was asserted | Exact equality, the counters, the method amended, gate | Exact, finite states and both counters asserted in every scenario; method amended. **Gated** |
| `gungnir-intercept-service` Plan determinism and degradation | No solve budget exists; the stale plan is never compared with the last good one | Hold, file a gap | Held on GAP-119 |
| `gungnir-app` Backend switching | Nothing compares embedded and remote projections; "(planned)" is stale | Hold, fix the method, file a gap | "(planned)" dropped. Held on GAP-120 |
| `gungnir-remote` Store-and-forward | Order and capacity never asserted despite "(tested)" | Write the test, then gate | `store_and_forward.rs`: a detached outbox and a link whose node is down each hold the newest 100,000 in order and count the seven dropped, and a node that comes up receives the oldest held first. Forwarding a whole outbox takes 391 s at the link's 64-per-tick rate, so that test is ignored; it passed once, here. **Gated** |
| `gungnir-resilience` Bounded queue; reconciliation | Every clause asserted; `StoreAndForwardQueue` has no caller | Gate, file the unused-queue gap | **Gated.** GAP-121 |
| `gungnir-store` Journal round-trip; retention | Confirmed in Rust in this walk: 17,927 of 200,000 random doubles read back one ULP off; the purge is unbuilt and unlisted | Split: fix the floats and gate; gap the purge | `float_roundtrip` turned on. 4,224 random and seven named doubles round-trip bit for bit in both durability profiles, where 1,050 changed without the feature. Row split: the round trip **gated** for finite values, retention held on GAP-122. A non-finite float cannot be journaled faithfully, GAP-126 |
| `gungnir-identity` Identity persistence | Both binaries re-mint identifiers at every start | Hold, file the defect gap | Held on GAP-123 |
| `gungnir-identification` Evidence fusion | Covered | Gate | **Gated** |
| `gungnir-assessment` Risk scoring | The score reads range and whether a track is closing, never time to impact | Hold, file a gap | Held on GAP-124 |
| `gungnir-observability` Health and alert correlation | Health is set once, never toggled; neither binary uses `SnapshotHealthMonitor` | Hold, file a gap | Held on GAP-125 |
| `gungnir-replay` Deterministic playback | Only sequence numbers compared; no Scenario 1 recording exists | Amend the data source, compare envelopes, gate | Data source corrected. Two replays of a journal of non-dyadic track envelopes equal each other and the journal, whole. **Gated** |
| Cross-layer Disconnected reconciliation | Conflicts reported, never resolved by the rule D-03 locked; a person resolves every one; "every envelope once" unasserted | Build D-03's rule into reconciliation (against the recommendation); a person resolves what the rule cannot rank; build the role on decisions and the desktop's role here | @@RECONCILE@@ |
| `gungnir-config` Laydown validation | Refusal 4's resource half untested; refusal 1b's naming check vacuous; refusals 1a and 5 named less than the criterion asks | Fix the messages, write the tests, gate | Refusal 1a lists the laydowns; refusal 5 names each placement with a non-finite coordinate; the tests assert them. **Gated** |
| `gungnir-ui`, `gungnir-app` PN-16 laydown options table | The alternative's difference checked only by sign; 2 of 5 painted numbers checked; the "current" check vacuous | Write the assertions, then gate | The alternative's difference equals its figure less the current row's, exactly; the three painted strings are asserted, and the vacuous check replaced. **Gated** |
| `gungnir-tracking-service` Sensor-position resolution | Polar placement only on axis-aligned inputs; the vertical axis held to 1.0 m in one test and not at all in the binaries | Write the tests, then gate | An off-axis polar report against the closed form; `up` within 0.5 m of the WGS84 reference, -0.225 m by two independent derivations, in the service and both binaries; and a polar report through the live service placed where `place_polar` puts it. **Gated.** At this geometry 0.5 m cannot tell -0.225 m from zero, so the row does not test the earth's curvature; the §1 `coord` row does |
| `gungnir-workflow` Collection requirements and tasking concurrence | The operator-session reason expired; tasking still succeeded with nobody signed in; no MT-08 replay | Enforce the operator, build the replay, gate (against the recommendation) | `TaskingCase::concur` refuses a concurrence naming nobody, and the desktop checks for an operator before it issues a command (D-54, DN-11 amendment 3). TT-08's sample events script MT-08's first three steps, and `requirements_replay.rs` replays them and reads the requirements back from the journal. **Gated** |
| The walk's two sheets | Their counts go stale as rows move | Move both here | Below |
| New gaps | About eighteen findings | One gap per finding | GAP-109 to GAP-125 |

## What the walk found that no criterion asked about

**Journaled doubles did not read back exactly.** `serde_json` without its
`float_roundtrip` feature parses a number as its significand times one power of ten,
which is not correctly rounded. Measured in Rust while preparing the store's question:
17,927 of 200,000 random doubles came back one unit in the last place off, and
57744.670227102644 came back as 57744.67022710264. The journal, the wire and the recorded
feeds all go through it, and the store's round-trip test had used half-integers, which
survive. The feature is on workspace-wide and §2.9's `serde_json` row records the term. A
non-finite value is a different failure -- `serde_json` writes it as `null`, which cannot be
read back, and `read_session` then fails the session or drops the line as torn -- and it is
GAP-126.

**The replay was bit-identical, and nothing said so.** All 78 final tracks of the five
scenarios equal `run_batch`'s bit for bit in release and in debug, and every submitted
detection is accepted with none too late; the test asserted a 1e-6 tolerance and read no
counter. It now asserts what is true.

**The desktop's authority role never followed sign-in.** `AppState.role` was fixed at
`Operator`, and a test pinned that signing in left it so, while the node authorized every
call on the caller's token. The owner first had it filed as a gap, then had it built here,
because D-03's rule cannot rank a decision whose role was never recorded (D-53).
@@ROLE_BUILD@@

**D-03's rule was locked and never applied.** `RoleRankArbiter` was gated today on its own
tests, and nothing called it: reconciliation reported conflicts and a person resolved every
one. The owner chose to build the rule into reconciliation rather than record the practice
(D-53). @@ARBITER_BUILD@@

**The concurrence row's reason had expired.** DN-11 amendment 1 b allowed a concurrence
naming nobody because no build had an operator session; sign-in shipped, and tasking went on
succeeding with nobody signed in. The owner enforced the criterion rather than widen it
(D-54, DN-11 amendment 3). A desktop with no account store cannot task through a
requirement now, and says why. DN-23 §8's attribution row was written before D-54 and is
not amended here: a decline with nobody signed in still records that nobody was, and a
tasking concurrence with nobody signed in is refused rather than recorded.

**Defects the tests found, fixed in this change.** The glyph comparator ignored three of
the fields a glyph draws. `FileConfigStore::load` parsed a whole baseline before reading its
schema version, so a newer file that reshaped a known field read as `Encoding` rather than
`VersionTooNew`, and `validate` judged a newer baseline's security section before its
version. Laydown refusal 1a named no laydown and refusal 5 no placement. The SAPIENT
gateway test's double recorded only its own refusals, so "offered as a
`Measurement::Bearing`" was not asserted until the double kept what it was offered.

**Findings filed as gaps, one each.** The node never times out an unacknowledged sensor task
(GAP-115) and writes no audit entry (GAP-111). Identities are re-minted at every start
(GAP-123). `StoreAndForwardQueue` has no caller, though `ARCHITECTURE.md` §8.4 said it
carried envelopes (GAP-121). The retention purge is unbuilt, and `unbuilt.md` cannot list it
because nothing refuses (GAP-122). The risk score reads range and whether a track closes,
never time to impact (GAP-124). The desktop's `decide` and `task` name their permission on
the audit entry and check it nowhere (GAP-127). The rest are the held rows' own gaps:
GAP-109, GAP-110, GAP-112 to GAP-114, GAP-116 to GAP-120 and GAP-125.

**Stale text corrected on the way.** `conformance.rs` still called STANAG 4676 blocked on an
owner decision a day after D-44 descoped it; `tasking.rs` and `sensor_control.rs` said no
control adapter existed; `gungnir-app/src/misb.rs` cited the register's old generator
function; `gungnir-data`'s module doc listed three formats as `NotImplemented` that load;
`TrackingService::tracks` said confirmed and coasting where it returns tentative tracks too;
two layout citations named the wrong section of `information-architecture.md`; two doc
comments sat on the wrong item; and `testdata/asterix/SOURCE.md`, `testdata/misb/SOURCE.md`
and `design/external-standards.md` each misstated a count or a precision.

**What this change leaves for the owner to decide.** `scenarios.yaml` gained TT-08's script
without a new `version`: bumping it re-stamps all ten sample sets and changes the content
hash of any dataset built from them, which is a choice rather than a correction. The full
forward of a store-and-forward outbox is an ignored test at six and a half minutes.
`gungnir-fuzz/Cargo.lock` predates the fuzz crate's `rand` dependencies, which the nightly
does not notice because it does not build `--locked`.

## Where the walk's own sheets went

The table of which §2 rows had a test (prepared 2026-09-06) and the walk sheet (prepared
2026-09-15, with #124's correction to its Group D) were the walk's working papers. Their
counts, groupings and corrections were true of the day each was written and go stale the
moment a row moves, so the owner had both moved here, verbatim, as they stood on `main` at
`4724b50`. The verification table keeps the criteria; a gated row names its own test in its
method cell. The plan-11 section's two paragraphs about the rows this walk settled -- the
concurrence row that "cannot be gated by any build" and the outbound-control row that
"stays listed as not-a-gate until GAP-067 moves its status" -- went with them, since both
stopped being true today.

## The table of tests and the walk sheet, as they stood

Moved verbatim from `verification-capability-table.md` §2.

### Which §2 rows have a test (2026-09-06, prepared for the GAP-067 walk)

No criterion above was changed. For each row, the test that exists today; the owner
confirms, row by row, that the criterion is the one the test checks (D-16), and only then
does the row's status move in `architecture.md`. Rows with no test are listed as such.

| Row | Test | Owner confirmation |
|---|---|---|
| `gungnir-tracking-service` Whole-pipeline scenario replay | `gungnir-tracking-service/tests/whole_pipeline_replay.rs` (all five scenarios; 1e-6, the tightest §1 tolerance the pipeline exercises) | |
| `gungnir-tracking-service` Non-blocking snapshot and health | `mod tests` in `gungnir-tracking-service/src/lib.rs`; p99 measured against a populated snapshot in `gungnir-app/tests/frame_budgets.rs` (2026-09-06: 13 tracks, both calls at 100 ns against the 1 ms criterion) | |
| `gungnir-intercept-service` Plan determinism and degradation | `mod tests` in `gungnir-intercept-service/src/lib.rs` | |
| `gungnir-data` Loader correctness per format | `gungnir-data/tests/{dem,pointcloud,vtk_gltf}.rs` (LAS, DEM, VTK, glTF; **COPC's bounded reader landed 2026-09-07** through `las::copc` -- the refusal side is tested (a non-COPC file, malformed bounds, a missing file), the happy path is not: this workspace holds no COPC fixture of its own yet, so bounded reading against real octree-indexed points is unverified here, proven only by `las::copc::CopcReader`'s own doctest) | |
| `gungnir-data` Loading off the UI thread | `gungnir-app/tests/terrain.rs` (`spawn_loader`) | |
| `gungnir-data-fusion` CPU ICP reference | `mod tests` in `gungnir-data-fusion/src/{cpu_reference,transform_solve}.rs` (corrected 2026-09-07: not `lib.rs`, which holds the trait and the `GpuFusionEngine` stub and carries no tests of its own) | |
| `gungnir-data-fusion` GPU path against CPU reference | `gungnir-data-fusion/tests/gpu_vs_cpu.rs`, four `#[ignore]`d tests behind `gpu-tests` (2026-09-08, GAP-024): two check this row's exact criterion (transform within 1e-3 m/rad, inlier ratio within 0.01, against `CpuIcp`), one checks self-registration convergence, one checks voxel fusion's self-consistency (no CPU oracle for that stage). Passed against a real `wgpu` adapter the implementing agent's own sandbox unexpectedly had (an RTX 5060 Ti, the same model as `gungnir-rtx-5060ti`) -- real hardware execution, but not the recorded `gpu-fusion.yml` dispatch through GitHub Actions this row's own verification method names; see GAP-024's gap-register entry and its PR for whether that dispatch was attempted and what it returned. **The recorded dispatch ran 2026-09-16 and the row is a gate**: `gpu-fusion.yml` run 35113741715 on `main`, on `gungnir-rtx-5060ti`, all four tests executed and passed (`record/2026-09-16/gpu-verification-row-gated-on-the-recorded-dispatch.md`). Separately, all four WGSL kernels parse and validate under `naga` in plain `cargo test`, no GPU needed (`gungnir-data-fusion::gpu::validation`) -- real but partial evidence, not a stand-in for the row above | |
| `gungnir-render` Single device, no per-frame resource creation | none | |
| `gungnir-viewport3d` SSE and tileset traversal | none (corrected 2026-09-07: this row was listed as tested against `mod tests` in `lib.rs`, which does not exist; `screen_space_error` and tileset traversal live in `src/streaming/{sse,tileset}.rs` and neither file, nor any other in the crate, has a test of either) | |
| `gungnir-viewport3d` Glyph rebuild only on change | `gungnir-viewport3d/src/tracks.rs` `mod tests` (corrected 2026-09-07: not `lib.rs`) | |
| `gungnir-ui` No duplicate state; no allocation in `ui()` | `gungnir-app/tests/frame_budgets.rs` -- **and until 2026-09-15 that pointer was wrong**: this row's budget is an egui pass, and that file timed `update()` and the two snapshot calls and nothing else. The GAP-067 walk found it by going looking for the test. `the_egui_pass_is_measured` now times a full commander workspace tree through `RenderProbe` at Scenario 4 counts: **p99 386 microseconds release, 1.42 ms debug, against the 8 ms budget**. Measured, not asserted | |
| `gungnir-app` Backend switching | `gungnir-app/tests/failover.rs`, `failover_e2e.rs` (embedded and remote through one `AppState`) | |
| `gungnir-model` Schema round-trip and versioning | `gungnir-interop/tests/conformance.rs`; `mod tests` in `gungnir-model` | |
| `gungnir-eventing` Broadcast delivery and ordering | `mod tests` in `gungnir-eventing/src/lib.rs` | |
| `gungnir-remote` Store-and-forward and honest connection state | `gungnir-remote/tests/transport.rs`, `tls_link.rs` | |
| `gungnir-node` Headless loop | none automated (corrected 2026-09-07, GAP-067 walk): "the node smoke run on every batch" is a person running the binary, not a test, and `encryption_at_rest.rs` gates journal sealing, not this row's own criterion (start, run ticks, interrupt, exit cleanly). No test asserts that sequence today | |
| `gungnir-interop` Arrow round-trip; catalog negotiation; ASTERIX decode | `gungnir-interop/tests/{conformance,asterix_fixtures,ais_fixtures}.rs` | |
| `gungnir-analytics` Line-of-sight and coverage | `mod tests` in `gungnir-analytics/src/coverage.rs` (corrected 2026-09-07: no `los.rs` exists; `LineOfSight` and `FlatTerrainLineOfSight` are defined and tested in `coverage.rs`) | |
| `gungnir-resilience` Bounded queue; reconciliation | `mod tests` in `gungnir-resilience/src/lib.rs`; `gungnir-app/tests/failover.rs` | |
| `gungnir-collab` Authority arbitration; stale envelopes | `mod tests` in `gungnir-collab/src/lib.rs` | |
| `gungnir-workflow` Role layouts; alert lifecycle | `mod tests` in `gungnir-workflow/src/lib.rs` | |
| `gungnir-store` Journal round-trip; retention | `gungnir-store/tests/durability_policy.rs`; `mod tests` | |
| `gungnir-config` Validation and version gating | `mod tests` in `gungnir-config/src/lib.rs`; `gungnir-app/tests/baseline_validity.rs` | |
| `gungnir-mission` Session lifecycle and replay | `mod tests` in `gungnir-mission/src/lib.rs` (synthetic envelopes; no Scenario 1 recording) | |
| `gungnir-time` Late-data policy; replay determinism | `mod tests` in `gungnir-time/src/lib.rs` | |
| `gungnir-ingest` Validation and quarantine; the ASTERIX radar, direction-finder and UAS-identification adapter | `gungnir-ingest/tests/{asterix_feed,asterix_seeds,authentication_strength,fuzz_corpus,scenario_round_trip,test_track_samples,generated_set_replays}.rs`; `mod tests` in `gungnir-ingest/src/adapters/asterix.rs` (the category routing and the two `bind_feed` host-wiring tests, added 2026-09-08 with GAP-100 and GAP-101); `mod tests` in `gungnir-config/src/lib.rs`, `gungnir-app/src/radar.rs` and `gungnir-node/src/main.rs` (the baseline half of that wiring) | |
| `gungnir-ingest` The SAPIENT spotter adapter | `gungnir-ingest/tests/sapient_spotter.rs`; `mod tests` in `gungnir-ingest/src/adapters/sapient.rs` | |
| `gungnir-ingest` The UAS KLV metadata adapter | `gungnir-ingest/tests/misb_feed.rs`; `mod tests` in `gungnir-ingest/src/adapters/misb.rs`; `gungnir-interop/tests/misb0601_fixtures.rs`; `mod tests` in `gungnir-interop/src/misb0601/mod.rs` | |
| `gungnir-sensor-management` Mode transitions; coverage | `mod tests` in `gungnir-sensor-management/src/lib.rs` | |
| `gungnir-identity` Identity persistence | `gungnir-app/tests/order_of_battle.rs`; `mod tests` in `gungnir-identity` | |
| `gungnir-identification` Evidence fusion | `mod tests` in `gungnir-identification/src/lib.rs`; `gungnir-app/tests/cooperative_identity.rs`; `gungnir-app/tests/uas_identification.rs` and `mod tests` in `gungnir-app/src/uas.rs` (the third cooperative source, ASTERIX Category 129, added 2026-09-15 with GAP-101's consumer) | |
| `gungnir-geo` Geofence containment | `mod tests` in `gungnir-geo/src/lib.rs`; `gungnir-geo/tests/no_hazard_in_the_policy_chain.rs` | |
| `gungnir-policy` No-go and authority enforcement | `mod tests` in `gungnir-policy/src/{lib,authority}.rs` | |
| `gungnir-command` Decision recording | `mod tests` in `gungnir-command/src/lib.rs`; `gungnir-app/tests/no_execution_without_decision.rs` | |
| `gungnir-assessment` Risk scoring | `mod tests` in `gungnir-assessment/src/assets.rs` (MOP-28 monotonicity) | |
| `gungnir-decision` Alternatives and what-if | `mod tests` in `gungnir-decision/src/lib.rs`; `gungnir-app/tests/alternatives.rs` (corrected 2026-09-07, GAP-067 walk: not `sensor_plans.rs`, which tests DN-13 sensor re-tasking, GAP-037, and names neither `what_if` nor an alternative anywhere in it). This row also duplicates the already-gated §1 row of the same name below (`decision` Alternatives and what-if) -- worth the owner's eye on whether this draft row should be dropped rather than promoted | |
| `gungnir-modelops` Promotion gating and rollback | `mod tests` in `gungnir-modelops/src/lib.rs`; `gungnir-app/tests/governance.rs` | |
| `gungnir-security` Authentication, authorization, audit | `mod tests` in `gungnir-security/src/{authz,audit,session}.rs`; `gungnir-app/tests/{authentication,audit_trail}.rs`; `gungnir-node/tests/account_provisioning.rs` (the provisioning path: an account the binary creates is one the node authenticates; a duplicate, an empty passphrase, a corrupt file and an unknown role are each refused without changing the file); corrected 2026-09-07 to drop `authn.rs`, a 14-line file holding only the `Authenticator` trait and a comment that the concrete mechanism is not yet chosen, with no tests of its own | |
| `gungnir-api` Contract compatibility and authorization | `gungnir-remote/tests/transport.rs`; `gungnir-api/tests/{party,machine,mutual_tls}.rs` | |
| `gungnir-observability` Health and alert correlation | `mod tests` in `gungnir-observability/src/lib.rs` | |
| `gungnir-replay` Deterministic playback | `mod tests` in `gungnir-replay/src/lib.rs`; `gungnir-app/tests/sustainment.rs` | |
| `gungnir-reporting` Traceability | `mod tests` in `gungnir-reporting/src/lib.rs`; `gungnir-app/tests/measures.rs` | |
| Cross-layer System-of-systems interop | `gungnir-interop/tests/conformance.rs` (GAP-063; STANAG asserts `NotImplemented`) | |
| Cross-layer Disconnected reconciliation | `gungnir-app/tests/failover_e2e.rs` against a real transport (GAP-050) | |

### The walk sheet (prepared 2026-09-15, GAP-067)

**What this is.** The table above says which rows have a test. This says, for each,
what the walk actually has to settle -- because a row becomes a gate only when the
owner confirms the criterion is *the one the test checks* (D-16), and for six rows it
demonstrably is not. Rows are grouped by what the walk owes them, so the confirmations
that are quick are not mixed in with the ones that need a decision.

**What it is not.** Six rows below were read test-by-test against their criterion and
say what was found; the rest are **candidates**, where a test exists, its path
resolves, and the criterion is qualitative enough that confirming it means reading the
assertions with the owner rather than measuring anything. The distinction is stated
rather than blurred, because a sheet that claimed forty-four verifications it had not
done would be the same shortcut this section exists to refuse.

**Two blockers the 2026-09-06 preparation named have closed since.** GAP-063, the
interface conformance suite, was itself waiting on GAP-064's STANAG 4676 entry, and both
closed 2026-09-15 under D-44, which descoped 4676; GAP-057's node account store closed
2026-09-10. So neither the cross-layer interop row nor the `gungnir-workflow` concurrence
row is blocked by the gap its note names, and both want re-reading before the walk reaches
them.

**Every test path in the table above resolves** -- 81 paths checked 2026-09-15, none
missing. The three stale references that pass found (`gungnir-viewport3d/src/lib.rs`,
a `los.rs` that never existed, `sensor_plans.rs` for the wrong row) were corrected
2026-09-07 and nothing has drifted since.

#### Group A -- candidates to gate (34 rows)

A test exists and the criterion is qualitative (exact, expected, refused, recorded).
The walk reads the named assertions against the criterion, one row at a time; the
count is what is there to read.

| Row | Test functions behind it |
|---|---|
| `gungnir-tracking-service` Whole-pipeline scenario replay | 1 |
| `gungnir-intercept-service` Plan determinism and degradation | 8 |
| `gungnir-data` Loader correctness per format | 21 |
| `gungnir-data` Loading off the UI thread | 4 |
| `gungnir-viewport3d` Glyph rebuild only on change | 6 |
| `gungnir-app` Backend switching | 4 |
| `gungnir-model` Schema round-trip and versioning | 4 |
| `gungnir-eventing` Broadcast delivery and ordering | 4 |
| `gungnir-remote` Store-and-forward and honest connection state | 2 |
| `gungnir-interop` Arrow round-trip; catalog negotiation; ASTERIX decode | 21 |
| `gungnir-analytics` Line-of-sight and coverage | 8 |
| `gungnir-resilience` Bounded queue; reconciliation | 6 |
| `gungnir-collab` Authority arbitration; stale envelopes | 4 |
| `gungnir-workflow` Role layouts; alert lifecycle | 10 |
| `gungnir-store` Journal round-trip; retention | 11 |
| `gungnir-config` Validation and version gating | 108 |
| `gungnir-time` Late-data policy; replay determinism | 7 |
| `gungnir-ingest` Validation and quarantine; the ASTERIX radar, direction-finder and UAS-identification adapter | 144 |
| `gungnir-ingest` The SAPIENT spotter adapter | 27 |
| `gungnir-ingest` The UAS KLV metadata adapter | 26 |
| `gungnir-identity` Identity persistence | 5 |
| `gungnir-identification` Evidence fusion | 17 |
| `gungnir-geo` Geofence containment | 5 |
| `gungnir-policy` No-go and authority enforcement | 18 |
| `gungnir-command` Decision recording | 14 |
| `gungnir-assessment` Risk scoring | 12 |
| `gungnir-decision` Alternatives and what-if | 7 |
| `gungnir-modelops` Promotion gating and rollback | 19 |
| `gungnir-security` Authentication, authorization, audit | 39 |
| `gungnir-api` Contract compatibility and authorization | 4 |
| `gungnir-observability` Health and alert correlation | 2 |
| `gungnir-replay` Deterministic playback | 12 |
| `gungnir-reporting` Traceability | 6 |
| Cross-layer Disconnected reconciliation | 1 |

#### Group B -- a named change first (6 rows)

**Walked with the owner 2026-09-15, and all six are settled** (`../ARCHITECTURE.md` §10
item 132). In order: the snapshot p99 is **gated**, and the per-frame `update()` budget
beside it, on figures read first; the egui pass is **measured for the first time** and
stays ungated until the owner has read a figure; the CPU ICP now **asserts its own two
quantities** and is gated; the `gungnir-sensor-management` row is **split**, its mode half
gated and its coverage-accuracy criterion moved to `gungnir-analytics` where the geometry
lives; `gungnir-mission` is **gated on synthetic envelopes** with its data source
corrected; and the cross-layer interop criterion is **amended to the interfaces this
release speaks** and gated. What follows is what the walk found, kept as the record of why
each was not simply a confirmation.

Each of these has a test and the test does not check the criterion. None is a defect
in the code; each is a mismatch between a criterion agreed under D-16 and what was
built to check it, and the walk's job is to say which of the two moves.

1. **`gungnir-tracking-service` Non-blocking snapshot and health** (p99 under 1 ms).
   `gungnir-app/tests/frame_budgets.rs` **measures and prints this and deliberately
   does not assert it**, and says so in its own header: "promoting a Draft row to
   gated is the owner's walk (GAP-067), not a test's". Gating it is a one-line change
   the file has been shaped to accept. What is being confirmed is the criterion, not
   the code.

2. **`gungnir-ui` No duplicate state; no allocation in `ui()`** (egui pass p99 under
   8 ms at Scenario 4 track counts). **Nothing anywhere measures an egui pass** --
   `frame_budgets.rs`, which the table above names for this row, times `update()` and
   the two snapshot calls and contains no egui timing at all. This row needs a
   measurement built before it can be gated, and the table's pointer is misleading
   until then. `gungnir-app/tests/rendered_workspace.rs` is the harness that already
   drives a real egui pass, so it is where the measurement would go.

3. **`gungnir-data-fusion` CPU ICP reference** (translation within 0.05 m, rotation
   within 0.5 mrad). The tests assert something **stronger and differently shaped**:
   every source point, mapped through the solved transform, lands within 2e-3 m of
   where the known transform puts it. That implies the criterion comfortably, and it
   never reports translation and rotation error as separate quantities. Accept the
   stronger property as meeting the row, or ask for the two named errors to be
   asserted in the criterion's own terms.

4. **`gungnir-sensor-management` Mode transitions; coverage** (range within 1 percent,
   bearing and elevation within 0.1 degree of the fixture). The mode half is covered
   thoroughly. **The coverage-accuracy half is not tested at all**: the assertions
   check that a scanning radar covers and a standby one does not -- presence, not
   geometry -- and no test compares a coverage volume's range or angles against a
   fixture. Gate the mode half by splitting the row, or hold the whole row until the
   geometry is checked.

5. **`gungnir-mission` Session lifecycle and replay.** Tested against synthetic
   envelopes; the row's own data source names a Scenario 1 recording that does not
   exist, which the criterion text already admits in parentheses. Gate on the
   synthetic evidence and amend the data source, or hold the row for the recording.

6. **Cross-layer System-of-systems interop.** `gungnir-interop/tests/conformance.rs`
   asserts that STANAG 4676 returns `NotImplemented`. Until 2026-09-15 that was a
   placeholder waiting on GAP-064; **since D-44 it is the recorded consequence of a
   descope**, which is a different claim and a gateable one. The row's criterion still
   reads as though 4676 is coming. Amend it to the interfaces this build speaks, then
   gate.

#### Group C -- nothing to gate, and the reasons differ (3 rows)

- **`gungnir-render` Single device, no per-frame resource creation.** The crate has
  **zero test functions**. The criterion needs the debug counter its own method names
  and nothing counts anything today.
- **`gungnir-viewport3d` SSE and tileset traversal.** **Unbuilt, not untested**:
  `src/streaming/sse.rs` and `src/streaming/tileset.rs` both return
  `ViewportError::NotImplemented`. A criterion of agreement within 1e-6 has nothing to
  agree with, so this row is not a walk item at all -- it is engineering.
- **`gungnir-node` Headless loop.** Recorded 2026-09-07 and still true: the smoke run
  is a person running the binary. `gungnir-node/tests/` holds account provisioning and
  encryption at rest, neither of which is this row's criterion.

#### Group D -- one run away (1 row), gated 2026-09-16

**`gungnir-data-fusion` GPU path against CPU reference.** **Gated 2026-09-16** on the
recorded dispatch this group was waiting for: `gpu-fusion.yml` run 35113741715 on `main`,
on `gungnir-rtx-5060ti`, with all four `#[ignore]`d GPU tests executed and passed, two of
them asserting this row's criterion (`record/2026-09-16/gpu-verification-row-gated-on-the-recorded-dispatch.md`).

**Corrected the same day**: this paragraph said the tests had never executed on a GPU.
They had, on 2026-09-08, against a real adapter in the implementing agent's own sandbox,
as the test column above already recorded. What the row lacked was a recorded dispatch
through the registered runner, not an execution.

#### Group E -- the plan-11 and DN-25 rows (26 rows)

Written before their code (AP-17), so most stay draft by construction and are not walk
items yet. Four have tests and belong in the walk: `gungnir-sensor-management`
outbound control (noted 2026-09-05), `gungnir-config` laydown validation, the PN-16
laydown options table, and `gungnir-tracking-service` sensor-position resolution --
each names its test in the row itself. One more wants re-reading rather than walking:
the `gungnir-workflow` concurrence row is annotated "cannot be gated by any build"
because no build had an operator session, and **GAP-057 closed 2026-09-10**, so that
reason has expired even though the row's other blocker (the MT-08 replay, GAP-076)
may not have.

#### Order to walk it in

Group B first, six rows, because each needs a decision rather than a reading and two
of them (`gungnir-ui`, `gungnir-sensor-management`) may turn into engineering. Then
Group A, which is reading, and which can stop and resume at any row. Group C needs
nothing from the owner but a confirmation that it stays draft. Group D waits on the
dispatch. Group E after the four tested rows are separated from the rest.

## The plan-11 section's paragraphs about these rows, as they stood

Moved verbatim from the same table's "Rows added by plan 11" section.

Like every other row in this section they are **not gates yet**: a row becomes a gate when
its test lands and its status moves in `architecture.md`, under GAP-067. The design note
each row came from is `design/verification-rows.md`, which maps row to note to crate.

**One row cannot be gated by any build, and it is worth saying why (2026-09-05).**
`gungnir-workflow` / Collection requirements and tasking concurrence (CAP-2.12) is built
and exercised by `gungnir-app/tests/requirements.rs` -- a requirement is answered only
with evidence, a lapse is distinguished from a decline, and a requirement moves to tasked
only when a task exists. But its criterion asks for a concurrence **carrying an
operator**, and no build has an operator session (GAP-057): every concurrence this system
can produce records a role and states that nobody was signed in. The criterion is left
exactly as agreed rather than widened to accept a role, and `gungnir_model::Concurrence`
is shaped so that `operator()` is the predicate the row will be checked against once
GAP-057 lands. Its method is also an MT-08 replay, which needs the TT-08 integration
(GAP-076).

**One of them now has its test (2026-09-05).** `gungnir-sensor-management` / Outbound
sensor control and acknowledgement (CAP-1.3) is exercised by `mod tests` in
`gungnir-sensor-management/src/lib.rs` against a `StubAdapter` that accepts, refuses, or
ignores, and end to end through the desktop tick in `gungnir-app/tests/sensor_control.rs`.
The criterion was not touched: the `SensorControlAdapter` seam was built so that the
method this row already named -- a stub adapter -- is the method actually used, rather
than the row being reworded to match tests that called the registry directly. It stays
listed as not-a-gate until GAP-067 moves its status, which is that gap's job and not this
change's.

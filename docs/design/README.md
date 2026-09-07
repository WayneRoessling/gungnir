# Design notes

Deliverables of [plan 11, design gap closure](../plans/11-design-gap-closure.md): one
design note per open design gap, turning "the architecture does not name a component
responsible for this" into a specification an engineer can build from.

**Status: 2026-09-05.** Twenty-two notes and four consolidations, **all implemented**,
plus DN-23 (signed, desktop half implemented) and **DN-24 (signed and implemented
2026-09-05)**. DN-24 was raised later than the plan-11 set: GAP-053 could not be built
without it, and CAP-5.7 had no note. Three of its own sections needed correcting while it was
being built; those corrections were signed the same day.
The workspace carries 340 passing tests, and every safety rule each note names has a test
behind it.

Signed off by the owner on 2026-09-05: **all five human-owned notes** DN-08, DN-09, DN-10,
DN-17, and DN-22; option B for the one breaking change; and all twenty-three verification
rows, which have moved into
[`../verification-capability-table.md`](../verification-capability-table.md) §2 as agreed
criteria. Nothing in the set now waits on the owner.

**Reviewed 2026-09-05.** Both outstanding reviews are complete:

| Review | Outcome |
|---|---|
| Engineering reviewer, the five dependency edges | Accepted, including `gungnir-analytics` to `gungnir-sensor-management`, the one the set flagged as weakest. Each edge is now cleared to enter a manifest, drawn in `ARCHITECTURE.md` §7.1 in the change that adds it |
| Domain reviewer, each note against the thread step it claims | Accepted. Every note's section 1 was checked against the mission thread step it names |

The set is fully reviewed and signed. What remains is implementation.

## Implementation status

**Implemented** means the types exist with their own tests. **Implemented and wired**
means a binary constructs them and a panel draws the result. **Implemented, not wired**
means the first without the second, and names what would wire it. The distinction was
added on 2026-09-06 after nine of the batch's gaps turned out to be the first kind
(`ARCHITECTURE.md` §10 item 80).

Started 2026-09-05, in the order the plan's method sets: the keystone three first, because
six later notes read what they define.

| Note | State | Where |
|---|---|---|
| DN-01 Defended assets | **Implemented and wired** (GAP-026) (amendment 1 signed 2026-09-06) | `gungnir-model/src/assets.rs`, the asset section and its validation in `gungnir-config`, `gungnir-assessment/src/assets.rs`; `sustainment::asset_assessor` scores the picture against it and PN-06 draws the score |
| DN-04 Effector model | **Implemented and wired** (GAP-030, 2026-09-06); **amendment 1 (§9) signed 2026-09-06**, its field landed | `gungnir-model/src/effectors.rs`, the resource fields and their validation in `gungnir-config`, `ResourceView::is_adequate`; the planner filters on it and PN-05 draws what was withheld and why. `intercept_speed_mps` (amendment 1, D-26) is on both types, validated, and read by nothing until GAP-031's solver |
| DN-08 Policy configuration | **Implemented and wired** (GAP-052) | `gungnir-model/src/policy_settings.rs`, the policy section and its validation in `gungnir-config`; the desktop's policy chain, staleness rule and decision deadlines read it |
| DN-02 Prediction and approach | **Implemented and wired** (GAP-020, 2026-09-06) | `gungnir-assessment/src/prediction.rs`, the assessment section in `gungnir-config`, closest approach on `AssetExposure`; `gungnir-app/src/prediction.rs` predicts every frame, PN-04 lists approaches with the predictor named, the viewport draws dashed predicted lines. The filter predictor waits on GAP-011 |
| DN-09 Authority and control status | **Implemented and wired** (GAP-033, closed 2026-09-06) | `gungnir-policy/src/authority.rs`, two new denial reasons carrying the layer and the status; `decisions::submit` runs the chain with the baseline's control status and authority rules |
| DN-10 Queue expiry and escalation | **Implemented and wired** (GAP-034) | `gungnir-command/src/queue.rs`; `decisions::sweep` expires and escalates on the desktop tick |
| DN-14 Hazard layer | **Implemented and wired** (GAP-017, 2026-09-06) (amendment 1 signed 2026-09-06) | `gungnir-geo/src/hazard.rs`, two new layer kinds; `HazardConfig` in the baseline, `gungnir-app/src/hazards.rs`, drawn on PN-02, toggled on PN-11, listed on PN-14. The PN-04 note waits on GAP-020 and the PN-16 input on GAP-087 |
| DN-15 Anomaly detectors | **Implemented and wired** (GAP-021, 2026-09-06) | `gungnir-analytics/src/anomaly.rs`, six detectors as pure functions; `gungnir-app/src/anomaly.rs` feeds them from the tick and raises each finding once; PN-09 lists each detector's status |
| DN-05 Fires | **Implemented and wired** for the policy half (GAP-036, 2026-09-06) | `gungnir-model/src/plans.rs`, `gungnir-policy/src/fires.rs`, and the schema-version-2 migration. **This row said on 2026-09-06 that the chain evaluated a fires task; it did not** -- `FiresDeconflictionPolicy` was constructed by nothing until GAP-036 put it fourth in the desktop's chain the same day. PN-05 lists every check with its result. No planner proposes a fires task, and a fires decision opens no engagement (DN-06) |
| DN-06 Engagement and effect | **Implemented and wired on the desktop** (GAP-043, 2026-09-06) (amendment 1 signed 2026-09-06) | `gungnir-intercept-service/src/engagement.rs`, `DecisionId` in the model, engagement and expiry events; `gungnir-app/src/engagements.rs` opens on a decision and closes on track-lifecycle evidence; PN-17 and the report count the evidence sources apart. Effector reports arrive through the node's report route and move the engagement (GAP-040, 2026-09-06) |
| DN-24 Mission profiles and algorithm baselines | **Signed and implemented 2026-09-05**, with three §-corrections signed the same day | `gungnir-model/src/profiles.rs`, the profile schema and its six rules in `gungnir-config`, `gungnir-modelops` keyed on `AlgorithmBaselineId`, `gungnir-app/src/governance.rs`, the node's session record, PN-14's profiles section. GAP-053 closed 2026-09-06 and GAP-011 with it |
| DN-23 Operator authentication and sessions | **Amendment 1 (§10) written 2026-09-07 and unsigned**: the note gave the account file a format and never said who writes it, and `hash_passphrase` had no caller outside tests, so the file could not be produced by any shipped path; `gungnir-node account add|list` now writes it. **Signed 2026-09-05; desktop half implemented and wired** (GAP-057, 2026-09-06) **Session lifetime enforced on the desktop and PN-01's operator line drawn 2026-09-06** (GAP-057; `LocalAccountAuthority::with_lifetime` signed by the owner 2026-09-06) | `gungnir-security/src/session.rs` (`FileAccountStore`), `gungnir-config`'s `security.authentication`, `gungnir-app/src/session.rs`, PN-20. A sign-in establishes the node link. The node-issued token and `POST /v2/session` are not built |
| DN-11 Sensor control and tasking | **Implemented and wired** (amendment 1 signed 2026-09-05) | `gungnir-sensor-management/src/lib.rs` (`SensorControl`) and `tasking.rs`, `gungnir-model/src/requirements.rs`, `gungnir-workflow/src/tasking_case.rs`, PN-10 and PN-15 in `gungnir-ui`, `gungnir-app/src/requirements.rs`. A command travels to the node and is closed by the acknowledgement it streams back (GAP-004, 2026-09-06); the wire from the node to a sensor waits on GAP-001 |
| DN-12 Coverage and gaps | **Implemented and wired** (GAP-006, GAP-007) | `gungnir-analytics/src/coverage.rs`; PN-11 and PN-02 draw it, PN-10's recommendations score against it |
| DN-13 Sensor re-tasking | **Implemented and wired** (GAP-037, 2026-09-06) | `gungnir-decision/src/sensor_plan.rs`; `sustainment::sensor_plans` enumerates candidates from the registry's transition table and PN-10 draws them; edge (i) `gungnir-app` to `gungnir-decision` |
| DN-03 Warning | **Implemented and wired** (GAP-042, 2026-09-06) **Amendments 1 (§9, the pass-close distance) and 2 (§10, the due time from the closest approach) both signed by the owner 2026-09-06; amendment 3 (§11, how an acknowledgement arrives) written 2026-09-06 and unsigned.** **The code it specifies is signed by the owner 2026-09-06 and this amendment is not.** The two are kept apart deliberately: a signature on an implementation says the code does what it says, and a signature on a design says the design is the right one. Amendment 3 is the one that made §5 rule 2 reachable: `Warning::acknowledged` had existed with no caller, so a delivered warning went `Sent` then `Late` for ever | `gungnir-workflow/src/warning.rs`: `raise_due` and `mark_overdue` under a `WarningLedger` the desktop tick evaluates; PN-08 and PN-04 list warnings. No transport exists, so every warning fails loudly until D-08's endpoints carry one; the pass-close trigger waits on a distance the obligation does not carry |
| DN-19 Order of battle | **Wired on the desktop** (GAP-025, 2026-09-06) | `gungnir-reporting/src/order_of_battle.rs`; `gungnir-app/src/identity.rs` recovers the retained sessions into a resolver and PN-13 assembles the product over it. The node has tracks since GAP-011 closed 2026-09-06 and still has no resolver, because §7.1 draws no edge from it to `gungnir-identity`; that is now a decision rather than an impossibility (GAP-019) |
| DN-20 After-action review | **Implemented and wired on the desktop** (GAP-049, 2026-09-06) | `gungnir-workflow/src/review.rs`; `gungnir-app/src/review.rs` and PN-13's review section; `ReviewEvent` on the bus. Action assignment, PN-12 marks, PN-17 counts and the API routes are not built |
| DN-21 Battle rhythm | **Implemented and wired** (GAP-054) | `gungnir-reporting/src/rhythm.rs`; `rhythm::tick` produces the scheduled products, PN-17 draws the handover, the registry advances maintenance windows |
| DN-07 Handoff | **Implemented and wired** (GAP-040, 2026-09-06) | `gungnir-model/src/handoff.rs`; `gungnir-app/src/handoffs.rs` issues the handoff from the actionable record and posts it through `gungnir_remote::endpoint` to an `http` endpoint: delivered when accepted, refused with the body, retried and never dropped when unreachable. The node-side routes are `gungnir-api` write paths (human-owned) and wait on a machine identity |
| DN-16 Peer sources | **Wired** (GAP-009, 2026-09-06) | `gungnir-model/src/exchange.rs`, peer origin on `Provenance`; `gungnir-ingest/src/adapters/peer.rs` over `gungnir_remote::peer::PeerLink`, a machine link under the host's certificate, bound per peer on both binaries; PN-09 and the node's health line say whether each is linked. **Launch warnings as a distinct message (§5) built 2026-09-06**: `LaunchWarningReport` carries no kinematic state, which is §5's own reason for the message existing, and "it never creates a track" is held structurally -- warnings and detections ride separate queues, so nothing in the ingest path can make a detection from a warning. **Amendment 1 (§9) written the same day and unsigned**, because §3 named no type for one and §5 named no exchange item to send it under. **The code it specifies is signed by the owner 2026-09-06 and this amendment is not.** The two are kept apart deliberately: a signature on an implementation says the code does what it says, and a signature on a design says the design is the right one. **No producer**: nothing issues a launch warning |
| DN-17 Releasability | **Wired** (GAP-062, 2026-09-06) | `gungnir-model/src/releasability.rs`, marking on the markable types; `gungnir_reporting::combined_marking` marks the report; `gungnir-api` filters every collection response per party with the withheld count, the party read from the client certificate. All §7 rows drawn as of 2026-09-06 |
| DN-18 Coalition exchange | **Wired both ways for the picture** (GAP-065, 2026-09-06) | `gungnir-model/src/exchange.rs`, the two independent gates, composed in `gungnir-api` for a machine caller: `exchange` agreements in the baseline, a party with none refused with the reason. Inbound through the peer link since later the same day (GAP-009). **Amendment 1 (§9, three `/v2/exchange` routes) written 2026-09-06 and unsigned**, recording that §6's "no new endpoints" was diverged from -- and a divergence signed only through the code it produced would be a note that has quietly stopped describing the system: `Warnings`, `Reports` and `Handoffs` had no producer and no gate, and they cannot ride a snapshot. **The routes are gated and counted; nothing produces a product to send**, and a producer was deliberately not faked |
| DN-22 Key management | **Implemented and wired** for the provider surface (GAP-084); **amendment 2 (§11) signed 2026-09-06** **The keystore code, the escrow record at sign-in, and amendment 3 (§12, the passphrase-sealed keystore) are all signed by the owner 2026-09-06; the node's provider-issued TLS identity (D-29) is signed by the owner 2026-09-06** | `gungnir-security/src/keys.rs`, five new authorization actions; the desktop builds the provider the baseline names and PN-01 reports the sealing state honestly. The asymmetric provider (`P256KeyProvider`, signing and escrow per §11) is built and unwired (2026-09-06, signed by the owner the same day); the OS keystore is not built; no binary writes an escrow record yet |
| DN-27 Bearing-only detections | **Proposed 2026-09-06, unsigned. Built and gated 2026-09-06; the sign-off is still outstanding, and §7's display half is not built at all.** Unblocks the acoustic, passive-RF and spotter halves of GAP-001 and the sensor half of GAP-004 | `DN-27-bearing-only-detections.md`. The note exists to forbid one thing: **a bearing must never become a position by assuming a range**, in any of its three tempting forms -- a nominal range, a terrain intersection, or a projection onto a defended asset -- each of which produces a valid `DetectionView` at a place nothing is. `DetectionView.measurement` becomes an enumeration and every variant carries its error, because for a bearing the error *is* the information. Three rules: a bearing may update a track and **may not initiate one** (a fixed sensor cannot localise from bearings at all, and a filter given them converges confidently to the wrong range); two crossing bearings may initiate, refused below a minimum crossing angle rather than initiated with a huge covariance; an unmatched bearing is kept and drawn as a **ray**, never a symbol. Ghost resolution is named as `gungnir-association`'s open problem rather than specified. **Breaking: `SCHEMA_VERSION` must bump**, and it did, from 2 to 3. The note's own §1 records what landed where, including the one piece built and not wired: `gungnir-tracking-service` names a bearing `NotAPosition` rather than offering it to the pipeline, because nothing resolves the reporting sensor's position for it |
| DN-26 Laydown options | **Signed by the owner 2026-09-06. Nothing is built.** Unblocks GAP-087, and through it GAP-020's approach corridors and GAP-045's rehearsal record | `DN-26-laydown-options.md`. Gives a laydown a schema and an identity so a planning panel has alternatives to compare, which is the same shape of fix DN-24 made for algorithm baselines and for the same reason: an options table with one row is theatre with a map behind it. **Adds no dependency edge.** Three rules are about not showing a plausible number: a comparison carries the terrain model it was computed under, a laydown that could not be evaluated is not one that scored zero, and any ranking is labelled advisory. **Adopting a laydown is deliberately out of scope**: moving a sensor is a physical act with an authority chain this system does not model |
| DN-25 Cursor-on-Target exchange | **Design only; no code exists** (GAP-090, GAP-091, 2026-09-06). Not part of the plan-11 set and not signed | `DN-25-cursor-on-target.md`. Adds `ExchangeFormat::CursorOnTarget` and a `ReportedPosition` that is deliberately not a track; the codec in `gungnir-interop`, the feed in `gungnir-ingest` on edge (i), the sink in `gungnir-remote` behind proposed edge (s). **Both preconditions met 2026-09-06**: `external-standards.md` §5 pins the schema under D-33, and edge (s) is accepted. All three of its verification rows (CAP-7.4, CAP-3.8, CAP-1.6) were agreed 2026-09-06 and are in `../verification-capability-table.md` §2. It waits on a self-recorded corpus before the codec |

**All twenty-two notes are implemented as of 2026-09-05.**

**DN-25 and DN-26 are the notes in this directory that are design only.** DN-26 (laydown
options) was raised 2026-09-06 because GAP-087 had stayed open across three batches on a
blocker that was not the planning panel's own: `ConfigBaseline` carries one set of sensor
and resource positions, so an options table would have exactly one row. It is **signed by
the owner 2026-09-06** and nothing in it is built: the signature settles the design, and the
schema and the panel are GAP-087's engineering.

**DN-25 is the first note in this directory that is design only**, raised 2026-09-06 outside
plan 11 because two gaps it closes did not exist when the plan's set was drawn: GAP-091,
which is the discovery that GAP-009 and GAP-065 wired an exchange only participants with a
machine identity can use, and GAP-090, which is what that costs the fires deconfliction
check. It is not signed and nothing in it is built. The row below says so, and the sentence
above is about the plan-11 set, not about this directory.

What the keystone tranche put in place, with a test for each: an asset list that reports
itself unconfigured rather than scoring zero; an unknown priority or effector layer that
fails validation rather than defaulting; a resource at or below its reserve that is not
adequate; an unconfigured weapons control status that reads `Hold`; an action with no
authority rule that is denied; and a decision with no configured expiry that never expires.

The second tranche added prediction with its stated predictor, the hazard layer that is
descriptive and never a rule, six anomaly detectors that each publish what they cannot
know, weapons control status and the authority matrix, and queue expiry that no
configuration can turn into an acceptance.

**Two corrections implementation forced**, both recorded in the notes rather than quietly
absorbed, and both the same kind of error:

| Note | What the design said | What it had to be |
|---|---|---|
| DN-01 §3a | The assessor holds the asset list | The caller anchors each asset to the local frame, because the coordinate conversion lives in a crate the scoring path may not depend on |
| DN-15 §3a | The detectors take `TrackView` | They take a primitive snapshot, because taking a model type would need the very edge D-13 forbids and section 4 of the same note refuses |

Both are frame or boundary mismatches: the kind of thing a design pass is prone to missing
and an implementation always finds.

**The breaking change landed 2026-09-05**, all of it in one change as the design required:
`PlanView.solutions` became `PlanView.kind`, `SCHEMA_VERSION` went to 2, and the interface
module moved to `gungnir-api/src/v2/`. Ten call sites across six crates moved to
`PlanView::solutions()`. The migration was smaller than expected because `assignments()`
absorbed the difference: it yields a fires plan's firing unit and target, so every policy
that walks assignments covers fires without a second code path.

Two rules from the fires design proved to be the ones worth the code they cost. A
deconfliction result with no checks is **not** clear, and a check whose data is missing
**fails** with a stated reason rather than passing. Together they mean an unevaluated
safety check can never be reported as a passed one.

**The first three approved dependency edges entered manifests on 2026-09-05**, each drawn
in `ARCHITECTURE.md` §7.1 in the same change, per the rule. The acyclicity check was re-run
across all 152 crate-to-crate edges and found no cycle. Two of the five approved edges
remain unused, because the notes that need them are not built yet.

One edge arrived that the design did not anticipate: coverage names sensors by their model
identifier, so `gungnir-analytics` now depends on `gungnir-model` as well. Same direction,
same layer, and recorded in `dependency-edges.md` §4a rather than absorbed, because the
graph used to say analytics stood outside the model and that is no longer true.

**All five approved edges are in manifests as of 2026-09-05**, each drawn in
`ARCHITECTURE.md` §7.1 in its own change. The graph is acyclic across 154 crate-to-crate
edges.

One further design-versus-reality correction, the third of its kind and the same shape as
the first two: DN-20 assumed `SessionId` was a model type. It was in `gungnir-store`, and
the review case would have needed a sixth edge to reach it. Six crates share the type, so
it moved down to `gungnir-model` with the store re-exporting it. The standards already
prescribe that: a shared type lives in the lowest crate that needs it and is re-exported,
never redefined.

## What a design note is

Each note has the same eight sections: the gap and the thread step it blocks; the owning
component; types as Rust sketches; edges; behaviour including what happens when an input is
missing; configuration and interface delta; user-interface delta by panel; and the
verification row.

**A note without section 8 is not finished.** The criterion is agreed before the code
exists, because one written afterwards is written to fit the code (AP-17).

## The consolidations

Read these first. They exist so the three things most easily damaged by twenty-two separate
changes are each reviewed once.

| Document | Content |
|---|---|
| [`dependency-edges.md`](dependency-edges.md) | Five edges taken, four refused, the depth analysis, the acyclicity check, and the one edge to argue about |
| [`model-and-schema-deltas.md`](model-and-schema-deltas.md) | Every new model type, every baseline section, the defaults doctrine, five new authorization actions, and **the one breaking change**, decided |
| [`verification-rows.md`](verification-rows.md) | The map from each agreed verification row to its note and its crate, and the five criteria that carry the most weight |
| [`external-standards.md`](external-standards.md) | Where the ASTERIX Category 048 and STANAG 4676 (AEDP-12) specifications are, which editions to pin, what could not be verified, and what that means for GAP-064. Reference, 2026-09-06 |
| [`handoff-2026-09-06-radar-feed.md`](handoff-2026-09-06-radar-feed.md) | Handoff for the ASTERIX radar feed: state of each piece with its proof, the six facts the capture established, the data flow, the next six steps in order, and what looks like a bug and is not |

## The notes

| Note | Closes | Owning component | Edge |
|---|---|---|---|
| [DN-01 Defended assets](DN-01-defended-assets.md) | GAP-026 | `gungnir-model`, `gungnir-config`, `gungnir-assessment` | None |
| [DN-02 Prediction and approach](DN-02-prediction-and-approach.md) | GAP-020 | `gungnir-assessment` | None |
| [DN-03 Warning](DN-03-warning.md) | GAP-042 | `gungnir-workflow` | To `gungnir-assessment` |
| [DN-04 Effector model](DN-04-effector-model.md) | GAP-030 | `gungnir-model`, `gungnir-config` | None |
| [DN-05 Fires](DN-05-fires.md) | GAP-036 | `gungnir-model`, `gungnir-policy` | None |
| [DN-06 Engagement and effect](DN-06-engagement-and-effect.md) | GAP-043 | `gungnir-intercept-service` | None, and one refused |
| [DN-07 Handoff](DN-07-handoff.md) | GAP-040 | `gungnir-api`, `gungnir-model` | None |
| [DN-08 Policy configuration](DN-08-policy-configuration.md) | GAP-052 | `gungnir-config`, `gungnir-model` | None |
| [DN-09 Authority and control status](DN-09-authority-and-control-status.md) | GAP-033 | `gungnir-policy` | None, and one refused |
| [DN-10 Queue expiry and escalation](DN-10-queue-expiry-and-escalation.md) | GAP-034 | `gungnir-command` | None |
| [DN-11 Sensor control and tasking](DN-11-sensor-control-and-tasking.md) | GAP-004, GAP-005 | `gungnir-sensor-management`, `gungnir-workflow` | To `gungnir-sensor-management` |
| [DN-12 Coverage and gaps](DN-12-coverage-and-gaps.md) | GAP-006 | `gungnir-analytics` | To `gungnir-sensor-management` |
| [DN-13 Sensor re-tasking](DN-13-sensor-retasking.md) | GAP-037 | `gungnir-decision` | To `gungnir-analytics` |
| [DN-14 Hazard layer](DN-14-hazard-layer.md) | GAP-017 | `gungnir-geo` | None |
| [DN-15 Anomaly detectors](DN-15-anomaly-detectors.md) | GAP-021 | `gungnir-analytics` | None, per D-13 |
| [DN-16 Peer sources](DN-16-peer-sources.md) | GAP-009 | `gungnir-ingest` | None |
| [DN-17 Releasability](DN-17-releasability.md) | GAP-062 | `gungnir-model`, `gungnir-security`, `gungnir-api` | None |
| [DN-18 Coalition exchange](DN-18-coalition-exchange.md) | GAP-065 | None new: a composition | None |
| [DN-19 Order of battle](DN-19-order-of-battle.md) | GAP-025 | `gungnir-reporting` | To `gungnir-identity` |
| [DN-20 After-action review](DN-20-after-action-review.md) | GAP-049 | `gungnir-workflow` | None |
| [DN-21 Battle rhythm](DN-21-battle-rhythm.md) | GAP-054 | `gungnir-reporting`, `gungnir-sensor-management` | None |
| [DN-22 Key management](DN-22-key-management.md) | GAP-084 | `gungnir-security` | None |
| [DN-24 Mission profiles and algorithm baselines](DN-24-mission-profiles-and-algorithm-baselines.md) | GAP-086, and unblocks GAP-053 | `gungnir-model`, `gungnir-config`, `gungnir-modelops` | Both binaries to `gungnir-modelops`, and `gungnir-modelops` to `gungnir-model` -- the third missed by the note and added as edge (h) |

**Five notes are human-owned**: DN-08, DN-09, DN-10, DN-17, and DN-22. All five were
signed by the owner on 2026-09-05, DN-08 last because an earlier version of this index left
it off the list.

## Decisions taken 2026-09-05

| Item | Decision |
|---|---|
| The one breaking change to the plan type | **Option B.** Replace `PlanView.solutions` with `PlanView.kind` outright; schema version 1 becomes 2 and the path `/v1` becomes `/v2`, with no deprecated mirror. Removing `/v1` satisfies the contract's own rule rather than excepting it, because the set of known clients is empty |
| The verification rows | **Agreed.** All twenty-three are in the verification table as agreed criteria. Changing one is now a change request under phase H |
| DN-08, DN-09, DN-10, DN-17, DN-22 | **All signed.** The three that need no dependency edge (DN-08, DN-09, DN-10) are cleared to implement; DN-17 and DN-22 need no edge either |

## Decisions taken 2026-09-06

| Item | Decision |
|---|---|
| ASTERIX editions for GAP-064 | **Pinned.** Category 048 edition 1.32, Appendix A edition 1.13, Part I edition 3.1, Category 034 edition 1.29; recorded in [`external-standards.md`](external-standards.md) §1.6 and §1.7 and in the codec doc comments |
| Whether Category 034 is decoded | **Yes.** Service messages carry the sector timing and radar status that 048 target reports depend on; the decoder has its own boundary, `ServiceMessageCodec`, built the same day ([`external-standards.md`](external-standards.md) §1.7 and §1.8) |
| Whether the GPL-licensed public ASTERIX captures may be test fixtures | **Yes.** Test data only, never shipped, with a `SOURCE.md` recording origin, commit, and licence ([`external-standards.md`](external-standards.md) §1.5) |
| ASTERIX Category 048 decoder | **Built the same day**, `gungnir-interop/src/asterix/`, against the fixtures; what it does and does not do is in [`external-standards.md`](external-standards.md) §1.8 |
| The radar adapter (GAP-001, radar half) | **Built the same day**, `gungnir-ingest/src/adapters/asterix.rs`, with dependency edge (i) `gungnir-ingest` to `gungnir-interop` drawn in `ARCHITECTURE.md` §7.1 and recorded in [`dependency-edges.md`](dependency-edges.md). Host wiring and the service-report consumer are open ([`external-standards.md`](external-standards.md) §5) |

## Still open

| Item | Where | Recommendation |
|---|---|---|
| Whether `gungnir-analytics` may depend on `gungnir-sensor-management` | [dependency-edges.md](dependency-edges.md) §4 | Take it; if rejected, only one function moves |
| Escalation and expiry values per layer | [DN-10](DN-10-queue-expiry-and-escalation.md) | A new decision. The design carries no defaults for them, deliberately |
| Warning lead times per asset class | [DN-03](DN-03-warning.md), [DN-01](DN-01-defended-assets.md) | Configuration; the values are the owner's |
| Key custody in the cloud profile | [DN-22](DN-22-key-management.md) | Off-host managed service; whose service is contractual, not architectural |

## What the set is careful about

- **Silence about authority denies; silence about capability disables and says so.** One
  doctrine across every default in the set, with a single deliberate exception for decision
  expiry, argued in `model-and-schema-deltas.md` §5.
- **Nothing gains a path to act.** Five notes touch the decision loop and none adds a way
  for the system to execute without a recorded human decision. DN-10 goes furthest and
  states that no configuration can make an expiry accept.
- **Every closure says what it cannot know.** The anomaly detectors, the engagement
  outcomes, the order of battle, and the prediction all publish the limit of their own
  inference rather than presenting a confident answer.

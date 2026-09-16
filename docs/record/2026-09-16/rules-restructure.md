# The rules restructured: one source per current fact

On 2026-09-16 the owner asked whether the project's rules and constraints were hindering
development, sensing that they produced bad outcomes at times and good controls at others.
A review of the rule documents, the CI gates and the history on `main` found the split
clean, and the owner asked for all nine of its recommendations to be carried out. This
item records what changed and why; the pull request carries the rest.

## What the review found

The engineering rules were enforced by tests and had caught real defects: dependency
direction, one owning crate per type, the unwrap policy, citations, no reachable
`todo!()`, no execution without a decision, and the anti-vacuous-pass checks. Signature
reviews of human-owned code had found defects nobody else had. None of that changed.

The cost was in the record-keeping. `ARCHITECTURE.md` §10 held 5,200 of the file's 5,942
lines and grew with every change; §2.9 of the coding standards was 745 lines of sign-off
essays; the gap register lived in Python string literals; one decision, D-39, was recorded
in 25 files; 65 of 183 commits changed only documentation, and about twenty of those
repaired a record that had gone stale. Parallel changes collided on §10 item numbers and
gap numbers, two pull requests could each pass and leave `main` red, and the review named
five rules in the standards as followed nowhere.

Two of those five claims were wrong, and are corrected here. `gungnir-fusion-async`
follows §2.8's log-level scheme where it logs, and one of its two public async functions
already carried a cancellation-safety note.

## What changed

1. **One source per current fact** (`docs/README.md`, "Keeping the set consistent";
   `CLAUDE.md`). Status, signatures, edges, criteria, approved crates and what is unbuilt
   each live in one generated, checked or tested source; other documents cite it.
   Narrative lives in record items, commits and pull requests, and is not edited.
2. **§10's items moved to `docs/record/`**, one file each, verbatim; items 1 to 136 and
   96A keep their numbers so the 372 citations of them resolve. New items are named by
   date and subject. `record.py check --base` refuses an edit to a merged item.
3. **The gap register's data moved to YAML** under `docs/mission/gap-analysis/data/`. A
   gap has no status field; its status is its last history entry, so the summary table
   cannot disagree with the entry. `gen_gaps.py --base` refuses an edit to merged history.
4. **One signature ledger**, `docs/signatures.yaml`, rendered into `docs/signatures.md`,
   with 163 entries. Restatements were removed from code comments, design notes and the
   other documents, and kept verbatim in three record items beside this one. `record/`
   item `signature-ledger.md` says how the ledger was built and what it leaves out.
5. **The document checks re-run after every merge.** `cross-document-recheck.yml` merges
   each open pull request from this repository with the new `main`, runs the
   cross-document checks, and posts the result on the pull request. This was chosen over
   requiring branches to be up to date, which would make every open branch rebuild after
   every merge whether or not anything it depends on moved.
6. **The dead rules were deleted, and one was enforced.** Doc examples on every public
   trait, `#[instrument]` on every predict and update, the `trace-numeric` feature, and
   trace capture by the retired central harness are gone. The cancellation-safety note is
   now tested.
7. **§2.9 became a table** of what stays true about each crate, linking its sign-off essay,
   moved verbatim to `stack-sign-offs.md`. A pull request that changes
   `[workspace.dependencies]` must carry `Decision:` and `Duplicate linkage:` lines.
8. **`docs/unbuilt.md` is generated** from every `NotImplemented` error the code returns.
9. **Commit message bodies are capped at 120 words**, checked by `pr-rules.yml`.

## What did not change, and why

- Doc comments written before this date that restate a pass criterion inline were left as
  they are. The rule for new comments is to name the row; where an old one disagrees with
  the table, the table is right.
- `docs/GungnirOverview.md` still says what was signed and unsigned on 2026-09-09. Its own
  header says it is an audit at one commit, so it is history.
- Stale statements that are not about signatures were left where they were found, apart
  from the TOGAF phase C status table, which restated the repository's status and was
  replaced by a pointer. Two came to light through the generated list itself:
  `gungnir-viewport3d`'s intercept geometry still names GAP-022 as what it waits on,
  though GAP-022 is closed, and `SensorRegistration::correct` returns `NotImplemented` for
  a call made before any bias is estimated, which is a precondition rather than unbuilt
  work.

## Two mistakes made while testing, both undone

A local trial of the recheck script reached the real `gh` instead of a stand-in, because a
Windows drive letter's colon split the `PATH` entry, and posted two "failure" statuses on
open pull request 120, whose merged tree lacked the new script. Commit statuses cannot be
deleted; a third status on the same context now says the first two were posted in error.
The same trial ran `git config user.name` inside a worktree, which wrote the repository's
shared configuration. It was restored within minutes, and no commit was made with the
wrong identity. The script now takes a dry-run switch and passes its identity per command.

## Appendix: §10's "Open" list as it stood on 2026-09-16

This list was the last part of §10 that was not a numbered item. Its content is retired
rather than moved into a current document, because each of its bullets is now read from a
generated source; it is kept here verbatim.

**Rewritten 2026-09-07, in the development-status review.** The seven bullets this
section carried before that date named the tracking math, intercept geometry, the
three-d attachment, the API transport, and cross-session identity as unbuilt; by
2026-09-07 all of that was built, gated, and in five of those cases already recorded
as such by later numbered items in this very section (92 to 94, 98, 100) that this
list itself was never updated to agree with. The bullets below are what a reading of
the code on that date actually found still open; nothing here was verified by
re-reading the register alone.

- **The tracking math's last filters.** `gungnir-rfs`'s CPHD cardinality distribution
  and the GLMB/LMB labelled filters return `NotImplemented` naming themselves; the
  Gaussian-mixture PHD they would extend is built and gated (item 94). Everything
  else this bullet used to list -- the EKF, UKF, particle filter, IMM, square-root/UDU
  form, RTS smoother, JPDA, MHT, track-to-track fusion and registration, the
  allocator, and the out-of-sequence pipeline itself -- is built and gated;
  `PIPELINE_IMPLEMENTED` has been `true` since 2026-09-06 (item 92, GAP-011). (GAP-015)
- **The GPU point-cloud registration path, and the compute context that has never
  been created.** `gungnir-data-fusion`'s CPU reference ICP is built and tested
  (`src/cpu_reference.rs`, `src/transform_solve.rs`); the GPU step returns
  `NotImplemented` naming the WGSL pipeline of §3.4 it waits on, and the four
  `shaders/*.wgsl` files hold that section's stage comments and no code. Reviewed end
  to end 2026-09-08, the path is inert further back than the shaders:
  `GpuContext::new` has **no caller** -- neither `gungnir-app` nor `gungnir-viewport3d`
  references `gungnir_render` or `gungnir_data_fusion` in source, though §7.1 draws
  both manifest edges -- so no `wgpu` device exists at run time and the only GPU work
  the application does is the viewport's OpenGL drawing (§9). The `gpu-tests` feature
  is declared and empty, so `gpu-fusion.yml` would run zero tests and fails such a run
  on purpose. **The `gpu` runner is registered as of 2026-09-08** (`gungnir-rtx-5060ti`,
  on the drafting host's RTX 5060 Ti), which was GAP-061's remaining item, and the
  workflow **stays on manual dispatch permanently** (D-10 as amended the same day):
  dispatch on a self-hosted runner is local execution with a recorded log, and only a
  caller with write access can fire it, which a `pull_request` trigger on a public
  repository would undo. It stays dormant until GAP-024 writes the tests. (GAP-024, and
  GAP-098 for the input and display path that would make the result reachable)
- **Live protocol adapters beyond radar.** ASTERIX (Category 048 edition 1.32,
  Category 034 edition 1.29), SAPIENT spotter tasking and detection, and -- since
  2026-09-07 -- the SAPIENT acoustic and passive-RF node types are all built and
  gated: one adapter (`SapientDetectionAdapter`) gated by an `accepted_node_type`
  rather than three separate ones, since all three node types share SAPIENT's wire
  shape. Both hosts register the configuration for all of them
  (`ConfigBaseline.radar_feeds`, `ConfigBaseline.sapient_feeds`; no longer true is
  this bullet's older claim that neither host registers it). The STANAG 4676 codec
  still returns `NotImplemented`. **This bullet's older claim that EO/IR and
  ISR-video each have a pinned specification is corrected 2026-09-07**: motion
  imagery (STANAG 4609/MISB, the ISR-video feed) is surveyed and *deliberately not
  pinned*, since it is a video-transport concern for the viewport rather than a
  detection message for the gateway (`docs/design/external-standards.md` §8); a
  passive-RF alternative over ASTERIX Category 205 is surveyed and likewise not
  pinned, passed over because SAPIENT's node type already covers passive-RF more
  cheaply (§9). EO/IR has no survey and no pinned specification at all -- nothing
  in `external-standards.md` names it. (GAP-001, GAP-064)
- **Bearing-only detections do not reach the operator.** DN-27's tracker half is built
  and gated: a bearing is a separate type, no function anywhere accepts one and
  initiates a track, and one that gates into an existing track refines it. §7, the
  display, is unbuilt -- and the chain stops earlier than the drawing.
  `FusionPipeline::retained_bearings` and the pipeline's five bearing counters have no
  caller outside `gungnir-fusion-async`'s own tests, no view carries a retained
  bearing, `gungnir-app` holds `SapientFeedStatsSink` values it never reads, and
  `SensorHealthView` has lines for radar, AIS and peer feeds and none for a spotter,
  acoustic or passive-RF one. So an acoustic array's ordinary output -- a direction
  with no range, which DN-27 §5 rule 3 calls exactly the report an operator most needs
  -- is journaled, replayable, and invisible in the picture. Four `pipeline.rs` doc
  comments stated the drawing in the present tense; corrected 2026-09-08 and signed by
  the owner the same day, they now say the bearing is retained for a caller to draw and
  name the gap as the reason none does. (GAP-096)
- **Cross-session identity correlation on the node.** The desktop resolver is built
  and wired, correlating by kinematic and classification similarity across sessions
  (`gungnir_identity::similarity`) -- not by session track id alone, which is what
  this bullet said until this rewrite and what the module's own doc comments said
  until GAP-019 corrected them on 2026-09-06. The node has had tracks since GAP-011
  closed and still has no resolver, because §7.1 draws no edge from `gungnir-node` to
  `gungnir-identity`: a graph decision now, not a missing capability. (GAP-019 is
  closed for the desktop half; the node half is this bullet)
- **Plan 05 gap register, most recently updated 2026-09-08.**
  `docs/mission/gap-analysis/gap-register.md` carries 104 gaps against the mission
  capabilities, each with a closing action, a target increment, and an owner;
  `docs/mission/gap-analysis/technical-gap-map.md` maps every item above to the gaps
  that carry it. Engineering items the list above does not name are tracked there by
  identifier: sensing and time (GAP-002 to GAP-005, GAP-008, GAP-009, GAP-023);
  picture and identity (GAP-006, GAP-007, GAP-012, GAP-014, GAP-017, GAP-018,
  GAP-020, GAP-021, GAP-024, GAP-025); the decision loop (GAP-026 to GAP-028,
  GAP-030, GAP-032 to GAP-040, GAP-042, GAP-043); sustainment and metrics (GAP-045,
  GAP-047 to GAP-049, GAP-051 to GAP-054); security (GAP-058 to GAP-060, GAP-062);
  integration, verification, and the plans in execution (GAP-044, GAP-046, GAP-055,
  GAP-061, GAP-063, GAP-065 to GAP-067); decisions D-01 to D-15, resolved on
  2026-09-04 (items 16 to 22 above and `docs/mission/gap-analysis/decisions-needed.md`),
  added GAP-068 to GAP-070; D-16 (measure targets) was resolved the same day (item
  24). Plan 06 (`docs/ux/`) added GAP-071 to GAP-074 (the replay, reports, and
  configuration panels; the status strip; the evidence card, commander summary, and
  theme additions; the usability rounds) and raised D-17 (docking and multi-window),
  resolved the same day (item 25) and implemented under GAP-075. Plan 07
  (`docs/test-tracks/`) delivered the vehicle catalogue, class profiles, sensor
  models, scenario library, data format, reference generator, and ten validated
  sample sets under `testdata/tracks/samples/`; GAP-046 is in progress and GAP-076
  covers seeding the fuzz corpus, the benchmark inputs, and the end-to-end replay.
  Plan 09 (`docs/ml/`) added GAP-077 to GAP-080 (the `gungnir-ml` crate and the
  inference-runtime sign-off, model manifests as `gungnir-modelops` baselines, the
  dataset pipeline, and the first two models); plan 08 (`docs/ai/`) resolved D-14 and
  keeps GAP-044 as the single assistant item; plan 10 (`docs/architecture/togaf/`)
  ran the first compliance assessment against this workspace and added GAP-081 to
  GAP-083 (automating the five mechanical contract checks, proving no `todo!()` is
  reachable, and making requirement identifiers traceable). GAP-092 to GAP-095 and
  D-35 to D-38 were restored to (D-35 to D-38) or added to (GAP-092 to GAP-095) the
  generator on 2026-09-07: D-35 to D-38 had been dropped from `decisions-needed.md`
  by an unrelated commit and are recovered here from `ARCHITECTURE.md` item 89's own
  record of them; GAP-092 and GAP-093 were a second, separate loss the same commit
  caused and were likewise recovered; GAP-094 and GAP-095 are new, the second because
  the first collided with GAP-090's own renumbering (item 89, and this section's own
  entry above). GAP-096 was added 2026-09-08 from a trace of the whole
  bearing path and is the bullet above; it is the first item in I3's order, priority
  40, and nothing blocks it. GAP-097 (an unchanged plan re-proposed and
  re-queued every tick) and GAP-098 were both added the same day, by separate changes
  that each claimed the number 097 within hours of each other while neither was on
  `main` -- the collision this list already records for GAP-090, GAP-094 and GAP-095,
  and for the same reason. GAP-097 kept the number, being the owner-confirmed claim
  already cited from D-28, GAP-074 and the CAP-3.3 coverage row; GAP-098 is the
  younger one and moved. **The count above had also fallen behind**: it read 96 when
  GAP-097 landed and 97 when GAP-098 did, and was corrected to 98 then rather than by
  whoever noticed it next. **It had fallen behind again and reads 103 as of
  2026-09-09**: GAP-099 (MISB ST 0601), GAP-100 (ASTERIX Category 205), GAP-101
  (ASTERIX Category 129) and GAP-102 (the point-cloud CRS half of D-41) each landed
  without moving it, and GAP-103 is item 122 below, having been an open bullet here
  until D-43 settled it on 2026-09-09. The same correction, made the
  same way, by the change that noticed it. **GAP-103 is also the fourth number
  collision this list has had to record**, and it was resolved by the same rule: it
  was filed as GAP-102 on 2026-09-08 while the point-cloud CRS gap was claiming that
  number on a branch of its own, and moved to 103 on 2026-09-09 because the other one
  was already merged and already cited from D-41's resolution. **GAP-104 is the fifth,
  recorded the same day by the change that hit it**: it was filed as GAP-101 on
  2026-09-08, moved to 103 when ASTERIX Category 129 and the point-cloud CRS gap were
  found already merged under 101 and 102, and moved again to 104 when
  `solve_assignment`'s contract took 103 by the same rule while its branch was still
  open. Two moves for one gap is what the rule costs when four gaps land in two days;
  the count above reads 104 with it. GAP-098 came out of the
  GPU review, which also rewrote GAP-024's closing action as five items, moved it from
  I4 to I3 without touching its severity, and removed its GAP-023 dependency so the
  WGSL work is unblocked.

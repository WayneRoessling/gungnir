# The miri gate had never run miri

GAP-164 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
Gate 3 of [`../../agentic-workflow.md`](../../agentic-workflow.md).

## What was wrong

`miri.yml` installs a nightly toolchain with the `miri` component and then runs
`cargo miri`. The workspace's `rust-toolchain.toml` pins `1.98`, and a toolchain file
outranks the default the setup action sets, so `cargo` resolved to the stable release,
which has no `miri` component: `cargo miri setup` failed before anything was
interpreted. It was found on 2026-09-26 when a doc comment in GAP-124's pull request used
the word the gate scans for, which triggered the job three times, and all three runs failed at setup
with "the 'miri' component ... is not available for the '1.98-x86_64-unknown-linux-gnu'
toolchain". No workspace source holds an `unsafe` block, function or impl today, so no
pull request had needed the gate yet; the next one that does would have met a gate that
could not pass, whatever its code did.

## What changed

`cargo +nightly miri setup` and `cargo +nightly miri test`: an explicit `+toolchain`
outranks the toolchain file. The workflow also takes `workflow_dispatch`, and a dispatched
run skips the `unsafe` scan and runs miri, so the gate can be seen to work on a diff that
has no `unsafe` in it. Nothing about when the gate runs on a pull request changed: the scan
still matches the word anywhere on an added line, comments included, which is broader
than the code it protects and is left as it is, because narrowing a gate is the owner's
call.

The first dispatched run, on the fix branch, got past setup for the first time: every
test in the first crate passed under miri, and the next binary, `gungnir-allocation`'s
`bellman_diff`, stopped at `open` because miri's isolation refuses file access and the test
reads a committed fixture. The job now sets `MIRIFLAGS=-Zmiri-disable-isolation`. Marking
fixture-reading tests `#[cfg_attr(miri, ignore)]` was rejected: it would shrink what the
gate interprets without saying so, and isolation protects determinism, not memory safety,
which is the only thing the gate is there to check.

The second dispatched run reached `gungnir-association` and stopped at a Stacked Borrows
violation inside nalgebra: `Cholesky::new` copies a column through
`ViewStorageMut::as_mut_slice_unchecked`, which builds a mutable slice from a raw pointer
an earlier retag had invalidated. A standalone crate holding one 3 by 3 Cholesky
reproduces it under nalgebra 0.33.3, the workspace's version, and under 0.35.0, the
latest; under `-Zmiri-tree-borrows` both pass, as does the workspace's gating test. The
gate now runs under Tree Borrows (D-90): under Stacked Borrows it would fail on a
dependency before it reached anything a pull request adds. The finding is kept as GAP-166
so it goes upstream rather than being forgotten.

The third run passed `gungnir-allocation`, `gungnir-association` and `gungnir-coord` and
failed one `gungnir-core` test, `direct_branch_is_taken_above_the_threshold`, by three
ulp: miri deliberately perturbs each transcendental result by a small random error, so
two evaluations of `x.sin() / x` no longer agree bit for bit. The test pins a property of
the machine's arithmetic, which the ordinary test gate checks on real hardware; the job
now sets `-Zmiri-deterministic-floats`, under which all nine `gungnir-core` tests pass
locally.

The fourth run, with every flag above, ran out of its 180-minute bound, which GitHub
reports as "cancelled", with nothing in its log. Timing each crate locally, in parallel,
under a forty-minute cap, split the twelve three ways:

- **Passed:** `gungnir-core`, `gungnir-coord`, `gungnir-association`, `gungnir-track`,
  `gungnir-track-fusion` and `gungnir-allocation`, each in minutes.
- **Failed:** `gungnir-intercept-service`, on five tests GAP-119 had just added or
  touched. They built their planners on the monotonic clock with the 4 ms budget, so a
  test that expected a fresh plan was asserting that this machine, in this build, solved
  inside 4 ms. Under miri no solve did. On an ordinary runner a debug build under load
  can take that long too, so these were intermittent failures waiting for a busy day.
  Every test not about the budget now plans on a clock that stands still, as the
  budget's own tests already did, and so does `gungnir-app`'s failover test that
  builds a node planner. The crate passes under miri: 35 and 1.
  `gungnir-tracking-service` also failed locally, only because miri on Windows cannot
  emulate the I/O completion ports tokio uses there; CI runs Linux.
- **Unfinished at forty minutes:** `gungnir-filters` (in `imm_diff`), `gungnir-rfs` (in
  `cphd_diff`), `gungnir-fusion-async` (in `oos_convergence`) and `gungnir-metrics`, whose
  `every_truth_object_is_accounted_for_exactly_once` alone took thirty minutes. These are
  the oracle-difference suites, millions of floating-point operations each.

The job is now one job per crate, in parallel, none cancelling another, each bounded at
350 minutes, just under the hosted runner's ceiling.

The per-crate run (dispatched 2026-09-26, 350 minutes a crate) passed eight crates:
core, coord, association, track, track-fusion, allocation, metrics and intercept-service.
`gungnir-tracking-service` passed its unit and resolver tests and failed its three
whole-pipeline replays on their own 60-second deadlock guard, which counts wall-clock
seconds against a replay miri stretches to hours. `gungnir-rfs` and `gungnir-filters`
were still inside their own unit test binaries at the bound, and `gungnir-fusion-async`
inside `dense_group` after 5 hours. On CI, metrics' `motmetrics_diff` took 2 hours 3
minutes, allocation's unit tests 28 minutes, fusion-async's 22, core's
`motion_models_diff` 16 and coord's `invariants` 11.

Every unit test and integration binary of filters and rfs was then run alone under
miri, sixteen at once, capped at ten minutes. Most unit tests finish in seconds; nine do
not, and they are the long runs by name ("over a long run", "a hundred thousand cycles",
"a long dense run", convergence and bimodal-cloud runs), with the `imm_diff`,
`linear_kalman_diff`, `particle_diff`, `sqrt_diff`, `cphd_diff`, `lmb_diff` and
`lmb_label_continuity` suites. `rts_diff`, `nonlinear_diff` and `phd_diff` finish in one
to eight minutes.

The owner chose to interpret each crate's unit tests and the suites that finish, leaving
the long runs out by name (D-92). `miri.yml` lists every one left out with its measured
time, and a test is added back when it is measured to finish. The slowest job is now
about 35 minutes, and the bound is 120.

## Evidence

The first dispatched run on `main` after this merges is the evidence that the job
interprets the twelve crates; GAP-164 records its outcome.

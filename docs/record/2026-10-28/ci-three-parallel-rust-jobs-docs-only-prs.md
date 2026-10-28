# CI: three parallel Rust jobs, docs-only PRs, and two triggers

The owner asked on 2026-10-28 why CI took so long and what could go. The runs of that day
were measured first, and four changes were made on the owner's go-ahead.

## What the runs showed

- **`fmt, clippy, test` was every pull request's wait.** It took 21 to 40 minutes, and
  every other check finished inside it.
- **Its eight steps ran in series on one 4-core runner.** The longest were:
  - the tests, 13 to 16 minutes: about 7 compiling and 7 running 2,592 tests, bounded by
    three `particle_diff` oracle tests over 60 seconds each in the debug profile;
  - the release-profile budget gates, 2.4 to 10 minutes;
  - the benchmark build, 2.6 to 5.6 minutes.
- **A pull request that changed only documents paid all of it.** #240, a signature in the
  ledger, took 39 minutes.
- **Miri ran for comments.** The gate scanned every added line for `unsafe`, so GAP-215's
  comments explaining why it did *not* use `unsafe` ran it three times, about 3.5 hours.
  Its longest job is `gungnir-fusion-async` at 107 minutes.
- **`bench-regression` ran nothing.** Its self-hosted runner had stopped taking jobs, so
  each push to main queued a run that waited up to three hours and was then cancelled by
  the next push.

## What changed

1. **A `what changed` job classifies a pull request.** It is `docs` when every file is
   under `docs/` (outside `docs/test-tracks/`, which the replay tests read as input), a
   Markdown file at the root, or a Markdown file under `deploy/`. Anything else, and every
   push to main, is `code`.
   - Checked against the day's history: #240 classifies as documents; #238, which also
     reworded two messages in Rust, and #244 classify as code.
2. **The one long job is three jobs that run side by side:**
   - `fmt, clippy`;
   - `test` (nextest and the doctests);
   - `budget gates and benches, release`.

   The wait becomes the slowest of them, the tests. Each keeps its own cache, still written
   from main only. The debug and release builds no longer share a runner's disk, which is
   what ran it out twice on 2026-10-27.
3. **A documents-only pull request runs only the Rust tests that read documents.**
   - `architecture_compliance` (citations and the §2.9 table);
   - `dependency_graph`;
   - the rehearsal test that reads a usability report;
   - the desktop's unit tests, which read the gap register;
   - `role_matrix`.

   The libproj job and the Linux node build are skipped too, since a document cannot change
   either.
4. **Miri runs on `unsafe` in code, not in comments.** Text from `//` to the end of an
   added line is removed before the scan. Checked: GAP-215's diff no longer triggers it,
   and a planted `unsafe { }` block still does.
5. **`bench-regression` runs by dispatch only.** Dispatched on main, it writes the
   baseline, which its save step now allows without a push event. Dispatched from a
   branch, it compares.

## What was not changed

- **The full run on main after each merge.** It saves the cargo caches every pull request
  restores, and it is where the flaky `alternatives` failure of 2026-10-28 surfaced.
- **The warnings check beside clippy**, under 2.4 minutes.
- **The slow oracle tests and the Miri suite.** Running the numeric crates' tests with
  optimisation would shorten the test job further, but it changes how every local debug
  build behaves, so it is left for a decision of its own.

## How it was checked

- The workflow files parse.
- The two scans were run against the day's diffs with the same expressions.
- The new layout's own first run is this change's pull request, which is code, so it
  exercises the three parallel jobs.
- The documents-only path is exercised by the next documents-only pull request.

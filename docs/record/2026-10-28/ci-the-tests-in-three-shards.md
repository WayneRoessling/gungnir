# CI: the tests in three shards

After the CI rework of the same day (`ci-three-parallel-rust-jobs-docs-only-prs.md`), the
`test` job was the whole wait on a pull request. The owner chose to shard it.

## What the run showed

On main's run for #246, the test job took 14.8 minutes:

- about 1.5 minutes setting up the runner;
- 5.6 minutes compiling;
- 7.2 minutes running 2,594 tests.

The tests add up to 1,716 test-seconds, which is the four cores' 7.2 minutes almost
exactly. So the running part is bound by the CPU, not by one slow test: the slowest,
`particle_diff`'s three oracle tests, take 66 to 75 seconds each and finish well
before the run ends.

## What changed

- **The `test` job is a matrix of three shards**, `test (shard N of 3)`. Each runs
  `cargo nextest run --workspace --partition count:N/3`, which deals the tests out in
  turn, so every test runs in exactly one shard.
- **The running part divides by three; the compiling part does not**, since each shard
  builds the whole workspace. The expected job time is about 9.5 minutes, close to the
  release job's 7.2. A fourth shard would save about one more minute.
- **`fail-fast` is off**, so a failing shard does not cancel the others and hide their
  failures.
- **The doctests run in the first shard only.** They take seven seconds.
- **The first shard alone saves the `debug-tests` cache** on main, so one run does not
  try to save the same cache three times. The other shards and `docs-tests` restore it.

The repository is public, so the extra runners cost nothing. No branch protection or
ruleset names the old `test` check, so renaming it breaks no required check.

## What was not changed

Building once and handing the binaries to the shards (`cargo nextest archive`) would avoid
compiling three times. It was not chosen: the archive of about 150 debug test binaries
would have to be uploaded and downloaded, and the compiling would still come before any
shard could start.

## What the first sharded run showed

The pull request's first run passed, and the shards' 956, 862 and 776 tests add up to the
workspace's 2,594, so every test ran exactly once. The running part fell as expected, to
1.3 to 2.3 minutes per shard. The job did not fall to 9.5 minutes:

| Shard | Free disk | Compile | Run | Job |
|---|---|---|---|---|
| 1 | 1.8 min | 8.6 min | 2.3 min | 14.1 min |
| 2 | 3.2 min | 7.8 min | 1.3 min | 13.7 min |
| 3 | 0.6 min | 5.6 min | 1.8 min | 8.8 min |

- **Compiling varies by runner**, from 5.6 to 8.6 minutes for the same build from the same
  cache. Three shards wait on the slowest of three runners, so this is the cost of the
  sharding, and nothing in the workflow controls it.
- **Freeing disk varies more**, from 34 seconds to over three minutes, and every later
  step waited for it. Every runner started with 86 GB free and the deletion added 18 GB.
  It now runs in the background in both this job and the release job, overlapping the
  setup and the build. A `disk at the end` step prints what each build left, so whether
  the deletion is needed at all can be read from the runs rather than guessed.

## What the second run showed

With the deletion in the background, the shards took 13.0, 11.2 and 8.5 minutes, against
the unsharded job's 16.4 on #246. Compiling still varied by runner.

**The deletion is needed.** Each shard ended with 129 GB of the 145 GB disk used and 17 GB
free. The debug test build takes about 88 GB, more than the 86 GB a runner starts with.
The release job ended at 46 GB used. Most of the debug build is debug information; building
the tests with `line-tables-only` would cut both the disk and the link time, but it changes
the cache and what a failing test's debugger can see, so it is left for a decision of its
own.

## How it was checked

- The workflow parses.
- Both runs' figures are above, read from the jobs' step times and logs.

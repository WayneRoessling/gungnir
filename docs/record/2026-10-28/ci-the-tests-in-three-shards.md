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

## How it was checked

- The workflow parses.
- This pull request's own run is the first sharded run. Its three shards' test counts
  should add up to the whole workspace's.

# CI: a bound on a hung test, one document job, and the changed files from the API

The third batch from this date's CI review, after
`ci-mold-advisories-off-the-pull-request-path.md` and
`ci-the-from-source-image-and-the-libproj.md`. Four smaller items, chosen by the owner
together.

## What changed

- **`.config/nextest.toml` with a `ci` profile**, which `ci.yml`'s `test` and
  `docs-tests` jobs select with `NEXTEST_PROFILE=ci`. A test still running after five
  minutes is terminated and reported as TIMEOUT by name; before, it held its shard until
  the job's 60-minute timeout and the log said only that the job was cancelled. Five
  minutes is about four times the slowest test measured on CI, `particle_diff` at 39 to
  75 seconds. The profile writes a JUnit file that each shard uploads as an artifact,
  kept 30 days. There are no retries, on purpose: the one test in this repository
  investigated as a flake had a real defect (GAP-136). The `default` profile is
  untouched, so a local `cargo nextest run` behaves as before.
- **The four document jobs are one job, `documents`**, running
  `check_documents.sh all`. Each of the four spent 10 to 20 seconds on a runner, a
  checkout and Python for 3 to 5 seconds of checking, and held one of the twenty
  concurrent jobs a public repository gets. The script already runs every group and
  names each one that fails. Three documents cited the old job names and now cite the
  new one with its group: `docs/architecture/uaf/README.md`,
  `docs/architecture/togaf/preliminary/architecture-repository.md` and
  `docs/GungnirOverview.md`. The overview's line numbers for the job were already wrong
  and are dropped.
- **`what changed` reads the pull request's files from GitHub's API** instead of a
  full-history clone. Every Rust job waits for it. Checked before the change against two
  merged pull requests: for #251 the API's list and `git diff` were identical; for #231,
  which renamed `gungnir-node/src/account.rs` to `gungnir-security/src/provisioning.rs`,
  the API's list was `git diff`'s plus the old name of the rename, which can only make
  the job run more, not less. A pull request at the endpoint's 3,000-file limit is
  treated as touching everything.
- **The stale `crs-projection` workflow registration is disabled.** Its file was merged
  into `ci.yml`'s `proj-crs` job and deleted; GitHub kept listing the workflow as active,
  with its last run on 2026-09-09. Disabled with `gh workflow disable`, which a person
  with write access reverses with `gh workflow enable` if it is ever wanted.

## What the pull request's run showed

The run of the pull request carrying this change is the first under all four. Its
figures are in the pull request's checks; this item is not edited after merge.

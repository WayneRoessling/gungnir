# CI: mold, advisories off the pull-request path, and miri on the crates that changed

A review of the pipeline on this date, from the last 30 days of runs, found that the
warm pull-request critical path is one test shard at about 8 to 10 minutes, of which
compiling and linking is 84 percent and running the tests 1.5 minutes; that 7 of 48
pushes to main had gone red, four of the twelve red `ci` runs being an advisory
published that morning failing a pull request that touched no dependency; and that the
miri gate's fixed matrix ran twelve tracking crates whenever `unsafe` was added
anywhere, and never the crate it was added to unless it was one of the twelve. The owner
chose these three changes as the first batch.

## What changed

- **mold links the test, release-test and bench binaries** in `ci.yml`'s `test` and
  `release-gates` jobs, through `rui314/setup-mold`. Each shard links 252 test binaries
  against the whole dependency graph on every run, because rust-cache saves the
  dependencies' artifacts and never the workspace's. The action places mold where `cc`
  finds the linker; no `.cargo/config.toml`, `RUSTFLAGS` or `Cargo.toml` names it, so a
  local build is unchanged.
- **`cargo deny check advisories` moved to `advisories.yml`**, which runs on every push
  to main, daily, and on a pull request that changes a manifest, the lockfile or
  `deny.toml`. `ci.yml`'s deny job keeps `licenses bans sources`, which read nothing a
  pull request without a dependency change can alter, and it now skips a
  documents-only pull request for the same reason.
- **`miri.yml` builds its matrix from the diff.** `.github/scripts/miri_matrix.py`
  reads the added lines of `.rs` files with comments removed, maps each `unsafe` to
  its crate, and emits the matrix: a crate in the script's table runs with the targets
  and skips measured for it, any other crate runs its library and every integration
  test. `unsafe` added outside a workspace crate fails the detect job by name rather
  than passing on nothing. A dispatch runs every crate in the table, as before. The
  per-crate table and the list of long tests left out, with the reasons, moved from
  the workflow into the script, beside the data they describe.

## What the pull request's run showed

Compared with main's run for #248, the first warm run with line-tables-only debug
information, on the same cache:

| Shard, `test` step | main for #248, GNU ld | this change, mold 3.0.0 |
|---|---|---|
| 1 | 6.7 min | 6.1 min |
| 2 | 7.7 min | 4.9 min |
| 3 | 7.5 min | 5.1 min |
| whole `test` job, per shard | 7.7 to 8.6 min | 5.9 to 7.0 min |
| `release-gates`, budget gates then benches | 2.5 + 2.6 min | 2.6 + 2.7 min |

Both are single runs on hosted runners, which vary by a minute or more between
instances for the same work (`../2026-10-28/ci-the-tests-in-three-shards.md`), so a
difference inside that band is not a result. Two of the three shards fell by 2.4 to
2.8 minutes, which is outside it; the third fell by 0.6, inside it. The release job
did not move: it links a few release test binaries and the bench binaries, not 252,
so there was little for a faster linker to take. The pull-request critical path, the
slowest shard, went from 8.6 to 7.0 minutes on these two runs. Every other check was
green; `advisories.yml` ran on the pull request because its own file is in the paths
filter, and `miri` detected no `unsafe` and ran no crate.

## What was checked without a run

`miri_matrix.py` against the branch with no `unsafe` added emits `has_unsafe=false` and
an empty matrix; with `--all`, the twelve crates of the former workflow matrix with their
targets and skips unchanged; against a synthetic commit adding `unsafe` to
`gungnir-core` and `gungnir-security`, with a comment mentioning the word beside them,
a two-crate matrix and no row for the comment; and against a further commit adding it
under `benches/`, which is not a workspace member, the named error and exit 1.

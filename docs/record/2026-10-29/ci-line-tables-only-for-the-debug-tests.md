# CI: line tables only for the debug tests

The sharded test job (`../2026-10-28/ci-the-tests-in-three-shards.md`) showed that the
debug test build takes about 88 GB of a runner's 145 GB disk. Each shard ended with 17 GB
free, and only because a background step deletes 18 GB of preinstalled toolchains. Most of
that space is debug information. The owner chose to try building the CI tests without it.

## What changed

- **`CARGO_PROFILE_DEV_DEBUG=line-tables-only`** on the `test` job and on `docs-tests`.
  The tests build in the `test` profile, which takes its debug setting from `dev`, and the
  setting reaches dependencies too.
- **`docs-tests` sets the same.** It reuses the `test` job's cache, and a different
  setting would make it rebuild everything.
- **Local builds are unchanged.** The workspace's `Cargo.toml` sets no profile, so a
  developer's debug build keeps full debug information.

## What is kept and what is lost

- **Kept:** a failing test's backtrace still names the file and line of each frame.
- **Lost:** variables, types and inlined-frame detail in a debugger. No CI run attaches
  one.

## The cache

The setting is part of the cache key, so this pull request's runs build from nothing: a
pull request reads main's cache and never writes one. They show the disk the new build
takes. How long a warm build takes shows only after main's first run saves the new cache.

## What the pull request's run showed

Every check passed, with all 2,594 tests across the three shards (956, 862 and 776).

| | Full debug (#247, warm cache) | Line tables (#248, no cache) |
|---|---|---|
| Disk used at the end of a shard | 129 GB of 145 | 74 GB of 145 |
| Of which the build | about 88 GB | about 33 GB |
| Free at the end | 17 GB | 72 GB |
| Compile, per shard | 5.6 to 8.6 min | 6.5 to 6.8 min |
| Job, per shard | 8.5 to 14.1 min | 9.2 to 10.4 min |

- **The build takes 55 GB less.** A shard now ends with more free space than the runner
  starts with before the background deletion, so the deletion is a margin, no longer a
  necessity. It is kept: in the background it costs nothing.
- **Compiling from nothing was as fast as the full-debug build was from a warm cache.** The
  time saved is in writing and linking the debug information, which was most of the
  work for about 150 test binaries. A warm run should be shorter still; main's first run
  after the merge saves the cache that shows it.

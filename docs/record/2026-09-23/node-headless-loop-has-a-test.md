# Node headless loop has a test

GAP-110 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
filed by the GAP-067 walk of 2026-09-16
([`../2026-09-16/gap-067-walk.md`](../2026-09-16/gap-067-walk.md)).

## What was wrong

`gungnir-node`'s own run cycle -- start on the default config, journal every envelope,
report health, exit cleanly on interrupt -- was checked only by a person running the
binary. `gungnir-node/tests/` held account provisioning and encryption at rest, and
nothing that started the process itself.

## What was found building the test

Before the test could assert anything, it had to read the node's own log lines, and on
this Windows machine it read none. `tracing_subscriber::fmt::init()`'s default writer
silently drops every `tracing::info!`, `warn!` and `error!` call once the process has no
console attached -- which is exactly how a test spawning the binary with piped
stdout/stderr looks to it, and exactly how a service manager or supervisor starts it too.
A raw `eprintln!` placed beside those calls printed fine the whole time; only the
`tracing` writer went dark. `RUST_LOG` was irrelevant -- the calls were never filtered,
they produced no bytes. Fixed with `.with_ansi(false)` before `.init()` in both
`gungnir-node/src/main.rs` and `gungnir-app/src/main.rs`, which carried the identical bare
`fmt::init()` call for the identical reason: colour escapes have no place in a log a
collector reads, and a headless binary should log the same way whether or not a terminal
happens to be watching. This is a real, previously unknown defect on this platform, found
incidentally while building an unrelated test, not part of GAP-110's own scope.

## What was tried and abandoned for the interrupt, on Windows

The gap's own action anticipated this: "On Windows the interrupt cannot rely on a signal
from Git Bash" (`CLAUDE.md` says the same). Three mechanisms were tried before the
Windows half was dropped rather than shipped broken:

1. `CREATE_NEW_PROCESS_GROUP` plus `GenerateConsoleCtrlEvent(CTRL_C_EVENT, child.id())`
   targeting the child's own group. Compiles, reports success, the child never reacts --
   `CTRL_C_EVENT` only supports group 0; a nonzero group is documented for
   `CTRL_BREAK_EVENT`, which `tokio::signal::ctrl_c()` does not listen for.
2. Broadcasting to group 0 with `SetConsoleCtrlHandler(NULL, TRUE)` protecting this
   test's own process first. The protection did not hold in practice: this test's own
   process was killed, `STATUS_CONTROL_C_EXIT`.
3. `CREATE_NEW_CONSOLE` for the child, then `FreeConsole`/`AttachConsole(child pid)`,
   broadcast, `FreeConsole`/`AttachConsole(ATTACH_PARENT_PROCESS)`. Both processes
   survived this time, and the child still never reacted to the broadcast.

Shipping any of these as the Windows path would have been a test that looks like it
covers a platform and does not, which this workspace's culture refuses. The file is
gated `#![cfg(unix)]` instead, with a doc comment recounting all three attempts so the
next person does not repeat them. CI gates this row on `ubuntu-latest`, where
`kill -INT <pid>` delivers exactly the `SIGINT` `tokio::signal::ctrl_c()` waits on -- the
standard, reliable mechanism, and the same idiom `gungnir-node/tests/account_provisioning.rs`
already uses to locate the binary (`CARGO_BIN_EXE_gungnir-node`).

## What the test holds

`starts_journals_and_stops_cleanly_on_interrupt`
(`gungnir-node/tests/headless_loop.rs`) spawns the real binary on the default config in a
scratch directory, waits for its own "opened live session" log line, sends `SIGINT`,
waits for "gungnir-node stopped; journal flushed and session closed", asserts a clean
exit status, then opens the journal it wrote and asserts exactly one session with at
least one envelope in it. Compiled and confirmed excluded (`0 passed; 0 failed`) on this
Windows machine, where `#![cfg(unix)]` applies; not runnable here otherwise, so the Unix
path itself is gated by CI rather than by anything run on this machine.

## What it left open

The Windows interrupt path remains unverified by any automated test; a person running
the binary from a real console, not Git Bash, is still how it is checked, unchanged from
before this gap.

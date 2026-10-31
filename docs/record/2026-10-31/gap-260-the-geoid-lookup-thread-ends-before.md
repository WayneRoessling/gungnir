# GAP-260: the geoid lookup thread ends before the process does

The `proj-crs` job failed twice on 2026-10-31 (run 41167956948 attempt 1, on PR #299, and
run 41169458675, on PR #301) in the same place: `cargo test -p gungnir-app --features crs
--test uas_identification` reported all six tests passed, and then the process died on
SIGABRT with glibc's `corrupted size vs. prev_size in fastbins`. It passed on #299's re-run
and on #300. Safe Rust cannot corrupt the heap, so this was C code, and it was not a flake.
None of the DN-36 terrain code those pull requests added runs in that binary.

**The reproduction.** On WSL Ubuntu 26.04 with the distribution's libproj 9.7.1 (above
`proj-sys` 0.27's 9.6.2 minimum, so it links the system library rather than building one),
the test binary looped 300 times aborted in 80, after `6 passed` each time, with
`corrupted size vs. prev_size while consolidating` or `double free or corruption (out)`.
CI builds PROJ 9.6.2 from source; the same failure on a different PROJ build and a
different glibc is a sign the fault is in how PROJ is used, not in one build.

**Which thread.** Run under gdb until one run aborted, the stacks named it. The aborting
thread was `geoid-undulation`, inside `proj_cleanup`, called from `proj::Proj`'s `Drop` at
the end of `UndulationService::start`'s thread closure. The main thread was in `exit`,
under `_dl_fini`, running the shared libraries' destructors. The desktop's own
`geoid-grid-check` thread, the other suspect, only hashes a file and never calls PROJ.

**Why.** `UndulationService` (GAP-196) keeps one PROJ pipeline on a thread of its own
because `proj` 0.31's `Proj` is not `Send`, and the service handed out is the channel's
sending end. The thread ended when the last clone was dropped, but nothing waited for it:
the drop closed the channel and returned. In the one test that starts the service,
`with_the_verified_grid_a_uas_height_reaches_the_ellipsoid`, the state and the adapter
holding the last clones drop as the test returns, the harness prints its summary, and
`main` returns. The lookup thread, woken by the closed channel, is meanwhile dropping its
`Proj`, and `proj` 0.31's `Drop` calls `proj_destroy`, `proj_context_destroy` and then
`proj_cleanup`. PROJ 9.6.2's `proj_cleanup` (`src/malloc.cpp`) closes the database of
`pj_get_default_ctx()`'s context, a function-local C++ static, and clears PROJ's static
caches. The exit handlers destroy those same statics. Two threads freeing one structure is
the double free glibc reported. It was intermittent because the window is the few
microseconds between the summary and the exit handlers.

**The fix.** The thread's `JoinHandle` and the channel's sender now live in one private
`Worker`, held in an `Arc` by every clone of the service. Its `Drop` closes the channel
and joins the thread, so the last clone's drop returns only once the `Proj` is gone and
`proj_cleanup` has finished. Anything that drops the service before `main` returns leaves
no PROJ work running into the exit. A refusal from `start` holds the worker too, so the
thread that failed to open the grid is joined before the error is returned. The join
cannot deadlock: the thread holds no clone of the service, only the receiving end.

A clone that is never dropped (one leaked into a static, or held by a thread that runs
until the process ends) still leaves the thread blocked in `recv` at exit. That is not
this race, because the thread is waiting, not inside PROJ. The node and the desktop hold
their clones in values that drop before `main` returns.

**What shows it.**

- `geoid::tests::dropping_the_last_clone_waits_for_the_lookup_thread_to_end` (Linux only,
  since it reads `/proc/self/task`) starts a service on the committed EGM2008 clip, finds
  its thread by name, drops one clone and sees the thread still there, drops the last and
  sees it gone. Against the old detached thread it failed 50 times in 50; with the join it
  passed 200 in 200. It runs in the `proj-crs` job's `cargo test -p gungnir-data
  --features crs --lib`.
- The `uas_identification` binary with the fix ran two loops of 1000 with no abort, where
  it had aborted in 80 of 300 before. The node's own `crs` tests and `gungnir-data`'s
  `crs` unit tests pass.

**What else the loop found: GAP-261.** Looping `gungnir-data`'s own `geoid` test binary on
eight test threads, to check the fix had not moved the fault, found a second crash, in the
middle of the run rather than at exit: 43 of 200 runs before this change, 68 of 200 after
it, and none in 200 on one test thread. Under gdb one test thread was closing PROJ's
SQLite database (`sqlite3BtreeClose`) while another parsed SQL on it and a third aborted
in `proj_destroy`. That is the same `proj_cleanup` in `proj`'s `Drop`, running while
another thread is inside PROJ, which PROJ's documentation rules out: it is for process
end, once nothing else is in use. Joining the lookup thread does not touch it, so it is
filed open as GAP-261 rather than folded in. Its fix reaches every `Proj` in `gungnir-data`,
by one process-wide lock around construction, conversion and drop, or by a teardown that
skips `proj_cleanup`. The second is `unsafe` and so human-owned. The `proj-crs` job runs
that binary once per pull request on a four-core runner, which is why CI has not shown
it.

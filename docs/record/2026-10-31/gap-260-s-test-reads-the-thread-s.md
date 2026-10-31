# GAP-260's test reads the thread's own word, not /proc

This corrects the test described in `gap-260-the-geoid-lookup-thread-ends-before.md`. The
fix that item records, joining the geoid lookup thread when the last `UndulationService`
clone drops, is unchanged.

**What failed.** `main`'s CI run on `485f6a86` (PR #305's merge, run 41222763467) failed
the `proj-crs` job on `geoid::tests::dropping_the_last_clone_waits_for_the_lookup_thread_to_end`
with "the thread outlived the last clone". Every other test in the run passed, and the same
test had passed on the runs of PR #303, #304 and #305. No other test in `gungnir-data`'s
library starts a lookup thread, so the thread it saw was its own.

**Why.** The test dropped the last clone, which joins the thread, then looked for the
thread by name in `/proc/self/task` and expected it gone. A join does not promise that.
glibc's `pthread_join` waits on the thread-id futex the kernel clears and wakes in
`exit_mm`, early in the exiting thread's `do_exit`; the thread is unlisted from `/proc`
later, when the task is released. So for a short time a thread that has been joined, and
whose code, `Proj` drop and `proj_cleanup` included, has all run, is still listed. On the
author's twenty-core machine that window never showed in 200 runs. On a four-core runner
running fifty library tests at once it did. The fix was right and the test checked
something the kernel does not guarantee.

**What the test reads now.** In test builds only (`#[cfg(test)]`), `Worker` holds an
`Arc<AtomicBool>` that the thread sets, with release ordering, after it drops its
pipeline. The test keeps a handle to it, drops one clone and sees it still false, drops
the last and sees it true. The join is what makes that read deterministic: everything the
thread did happens before `join` returns. Without the join, the last drop returns while the
thread may still be tearing PROJ down, and the flag can read false. The test no longer reads
`/proc`, so it is no longer Linux-only; it runs wherever `crs` builds.

**What shows it.** On WSL Ubuntu 26.04 with libproj 9.7.1, looping the library's test
binary:

- `main`'s `/proc` test, join in place: 0 of 300 runs failed on an idle machine and 1 of
  300 with every core held busy, the CI failure reproduced at the rate a shared runner
  would see it.
- The new test, join in place: 0 of 300 alone on an idle machine, and 0 of 300 with all
  51 library tests running and every core busy.
- The new test with the join removed (the thread detached, as before GAP-260): 100 of 100
  failed, so it still catches the defect it was written for.

# GAP-261: every PROJ call in gungnir-data holds one lock

GAP-261 was filed open from GAP-260's verification
(`gap-260-the-geoid-lookup-thread-ends-before.md`): `gungnir-data`'s `geoid` test binary
under `--features crs`, looped on eight test threads, aborted partway through its tests
in about a third of runs, with one thread closing PROJ's SQLite database while another
parsed SQL on it.

**The cause, read from PROJ 9.6.2's source** (`proj-sys` 0.27's bundled tarball).
`proj` 0.31's `Drop` for `Proj` calls `proj_destroy`, `proj_context_destroy` and then
`proj_cleanup`. `proj_cleanup` (`src/malloc.cpp`) takes `pj_get_default_ctx()`, sets its
`iniFileLoaded` false and closes its database, then clears the `+init` cache, the network
chunk cache and the grid-name sets of `hgridshift`, `vgridshift` and `gridshift`. Nothing
locks the default context while that runs, and `proj_context_create` (`src/ctx.cpp`),
which every `Proj::new` and `Proj::new_known_crs` begins with, is a copy of that same
default context. So any thread's drop could run while another thread copied or used it.

**The two ways out.** One lock in `gungnir-data` around every PROJ call, or a teardown
that destroys the object and context and skips `proj_cleanup`. The second calls
`proj-sys` directly in `unsafe` code, which `docs/agentic-workflow.md` makes human-owned.
The first needed neither, so it was taken.

**What changed.** `gungnir-data/src/proj_lock.rs` holds one process-wide lock and
`LockedProj`, a `proj::Proj` in an `Option` whose `new`, `new_known_crs`, `convert`,
`as_wkt` and `Drop` each take the lock for their PROJ call. The drop takes the inner
object out under the lock, so `proj::Proj`'s own drop, `proj_cleanup` included, runs
inside it. Every PROJ object the crate built (`geoid`'s `vgridshift` pipeline and the
`UndulationService` thread's copy of it, `nad83`'s NADCON5 pipeline, its Helmert height
step, its WKT1 datum read and its `new_known_crs`, and `geospatial::crs::to_wgs84`) is now
a `LockedProj`. `pointcloud::crs` reaches PROJ only through `nad83` and `geoid`.

**Why an idle object survives another thread's cleanup.** The geoid lookup thread keeps
its pipeline for the life of the process, so another thread's `proj_cleanup` still runs
while that object exists, only not while it is in a call. What `proj_cleanup` clears does
not belong to it: it has its own context and database handle, `vgridshift` keeps the
grids it opened in the object itself, and the cleared grid sets hold names only, behind
their own mutex. PROJ's documentation asks for `proj_cleanup` at process end; the lock
makes the calls it races against impossible, and the loop below is what shows the
remaining use is sound.

**What it costs, and why the lock is a ticket lock.** PROJ calls on different threads no
longer run in parallel. The first version used `std::sync::Mutex<()>`, and the crate's
own `--lib` suite then failed:
`geoid::tests::dropping_the_last_clone_waits_for_the_lookup_thread_to_end` got no
undulation, because the geoid lookup thread waited past `UNDULATION_LOOKUP_TIMEOUT`'s
250 ms while eight test threads converted in a loop. `std`'s mutex is not fair: a thread
that releases it and asks again at once usually gets it back. The lock is now a ticket
lock on a `Mutex` and a `Condvar`, served in the order it was asked for. It is held per
call (one point, not one file), so a live feed's lookup waits for the calls already
queued, at most one from each other thread using PROJ, however long their files take.
The suite has passed since.

**What shows it.**

- On WSL Ubuntu 26.04 with libproj 9.7.1, the `geoid` test binary on eight test threads
  aborted in 27 of 100 runs on PR #303's code (25 on SIGABRT, 2 on SIGSEGV), and passed
  500 of 500 with the lock as `std`'s mutex and 500 of 500 again with the ticket lock.
- The `--lib` suite on eight test threads, with the lookup test above among it, passed
  100 runs in 100 with the ticket lock.
- `proj_lock::tests::concurrent_builds_conversions_and_drops_stay_correct` builds,
  converts through, describes and drops a compound CRS, a transformation from its WKT
  and a `vgridshift` pipeline on eight threads, while a ninth converts through a
  pipeline it keeps throughout. **It does not reproduce the corruption without the
  lock**: with the lock compiled out it passed 100 runs in 100, as did two simpler
  versions before it. What it checks is that the lock keeps every answer right, never
  deadlocks, and lets a kept pipeline answer between other threads' cleanups. The
  `geoid` binary is the test that tells the fix from its absence, and the `proj-crs`
  job runs it once per pull request on four cores, where it has not shown the fault.
- `proj_lock::tests::no_other_module_builds_a_proj_object` fails if a code line outside
  `proj_lock.rs` names `proj::Proj`, so a new conversion cannot go around the lock
  without a test saying so.

**A trap met on the way.** The first run after the change looped the old binary: `rsync
-a` copied the edited files with their Windows modification times, older than the
previous build, and cargo did not rebuild. gdb's stacks still showed `geoid.rs:805`
calling `proj::Proj::new` directly, which is what gave it away. Syncing without
preserving times fixed it.

**Found on the way, not fixed here.** `proj` 0.31's `as_wkt` asserts that PROJ returned a
string, so a PROJ object with no WKT1 form panics rather than returning an error. A
transformation is one: the first draft of the test above asked a UTM transformation for
its WKT1 and panicked at `proj.rs:183`. `nad83::horizontal_datum` hands
`Proj::new` whatever source it is given and asks for its WKT1, so a source naming a
coordinate operation rather than a CRS would panic there instead of being refused.

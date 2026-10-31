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

**What changed.** `gungnir-data/src/proj_lock.rs` holds one `static Mutex<()>` and
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

**What it costs.** PROJ calls on different threads no longer run in parallel. The lock is
held per point, not per file, so a live feed's geoid lookup waits for at most one other
point or one construction, which is well inside the 250 ms
`UNDULATION_LOOKUP_TIMEOUT`.

**What shows it.**

- On WSL Ubuntu 26.04 with libproj 9.7.1, the `geoid` test binary on eight test threads
  aborted in 27 of 100 runs on PR #303's code (25 on SIGABRT, 2 on SIGSEGV) and passed
  500 of 500 with the lock.
- `proj_lock::tests::concurrent_builds_conversions_and_drops_stay_correct` builds,
  converts through, describes and drops a UTM transformation on eight threads at once.
  Without the lock it RESULT_NEGATIVE; with it, it passed RESULT_POSITIVE. It runs in the
  `proj-crs` job's `--lib` step.
- `proj_lock::tests::no_other_module_builds_a_proj_object` fails if a code line outside
  `proj_lock.rs` names `proj::Proj`, so a new conversion cannot go around the lock
  without a test saying so.

**A trap met on the way.** The first run after the change looped the old binary: `rsync
-a` copied the edited files with their Windows modification times, older than the
previous build, and cargo did not rebuild. gdb's stacks still showed `geoid.rs:805`
calling `proj::Proj::new` directly, which is what gave it away. Syncing without
preserving times fixed it.

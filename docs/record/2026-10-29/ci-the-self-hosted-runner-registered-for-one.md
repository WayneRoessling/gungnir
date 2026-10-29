# CI: the self-hosted runner registered for one job at a time

A follow-up to this date's CI review, which found the self-hosted runner offline. The
owner chose a runner registered on demand over reviving the standing one, running it as
a service, retiring it, or a cloud machine.

## What was found

- **The runner was not offline; it was gone.** The repository listed no self-hosted
  runner at all. `gungnir-rtx-5060ti` was an interactive `run.cmd` listener, as D-63
  keeps it, and its last session on the workstation ended on 2026-09-17. GitHub removes
  a self-hosted runner that has not connected for fourteen days, so every later dispatch
  queued for a runner that no longer existed. Two `bench-regression.yml` runs from
  2026-10-28 were still queued and were cancelled.
- **The two workflows that use it** are `gpu-fusion.yml` (dispatch only, D-10) and
  `bench-regression.yml` (dispatch only since 2026-10-28, advisory by D-62).

## What changed

- **`.github/scripts/run_self_hosted.ps1`**, run on the workstation, asks GitHub for a
  just-in-time runner configuration valid for one job, with the same name and labels
  the standing runner had, dispatches the workflow, runs the runner until that job ends,
  and removes the registration and its files. Nothing stands, so nothing expires. It is
  still started by hand, which is what D-63 decided; it is not a service. It cancels its
  own run if that run is still waiting for a runner when the script stops, since a
  queued run would otherwise be taken by the next dispatch's runner.
- The two workflow headers, `CONTRIBUTING.md` and `docs/agentic-workflow.md` say how to
  fire them now. Two stale statements were corrected on the way: `gpu-fusion.yml` and
  `docs/GungnirOverview.md` said the recorded dispatch had never happened, and
  `docs/agentic-workflow.md` said the GPU path could not pass yet, when the dispatch of
  2026-09-16 (run 35113741715) passed and closed GAP-024.

## How it was checked, and what it found next

Four dispatches of `gpu-fusion.yml` on main, each fixing what the one before showed:

1. PowerShell turned the comma-separated `--json` field list into an array, and the
   script stopped after dispatching; the runner registration was removed, and the
   queued run was cancelled by hand. The fields are quoted now.
2. The new run was looked for by creation time, and this machine's clock is a few
   seconds off GitHub's, so it was missed. It is found from the URL `gh workflow run`
   prints, or failing that by id against the runs listed before the dispatch.
3. The runner exited at once: it writes its configuration into `.runner` and
   `.credentials`, the expired registration's copies were still there, marked hidden,
   and .NET refuses to overwrite a hidden file. Leftover files are moved to
   `_previous-config` in the runner folder before it starts. The script also took the
   runner's exit for a finished job and left the run queued; it now cancels any run of
   its own still waiting for a runner.
4. Run 40924778849: the runner registered, took the job within a second, ran it, and
   was removed. **The job failed in `actions/checkout`**, which calls Git for Windows'
   `sh.exe`, and that fails on this workstation with
   `add_item ("\??\C:\Program Files\Git", "/", ...) failed, errno 1`, inside and outside
   any sandbox. Git is the 2026-08-26 install (2.55.0.windows.5), unchanged since
   `gpu-fusion.yml` passed on 2026-09-16; exploit protection is at its defaults, and the
   one system change since is the 2026-10-14 cumulative update. The cause is not
   established. Upgrading Git for Windows, three releases behind with security fixes,
   is the first thing to try; it is the owner's machine and the owner's call.

The runner also left the one-job configuration files behind after run 4. The script now
deletes them when it finishes, a step that has not yet run.

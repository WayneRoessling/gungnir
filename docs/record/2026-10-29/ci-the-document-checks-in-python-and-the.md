# CI: the document checks in Python, and the failures the bash script hid

A follow-up to this date's CI review and its three batches, after
`ci-a-bound-on-a-hung-test-one.md`. The review listed porting
`.github/scripts/check_documents.sh` to Python so the checks run on a Windows machine
where Git Bash does not; the owner asked for it.

## What changed

- **`.github/scripts/check_documents.py` replaces the bash script**, with the same
  groups (`uaf`, `gaps`, `tracks`, `records`, `cross`, `all`), the same steps in the same
  order, and the same `BASE_REF` variable, plus a `--base` option, which is easier to
  pass from PowerShell. It finds the repository root from its own path, so it runs from
  any directory. `ci.yml`'s `documents` job and `recheck_open_prs.sh` call it, and the
  documents that named the old script now name the new one. Merged record items and the
  2026-10-26 review keep the old name, since they are history.
- **A failing step now fails its group.** The bash script ran each group as
  `if ! "$group"`, and bash ignores `set -e` inside a function called in that position.
  So a step that failed without leaving a diff behind failed nothing: `record.py check`,
  `signatures.py check`, `validate_tracks.py` and `gen_gaps.py --base`, which are the
  history and validation checks, could not fail the job on their own. The only step
  that decided a group was the last one, the comparison with what is committed.
- **An untracked generated file now counts as a difference.** The comparison was
  `git diff`, which does not see a file git does not know about, so a generator that
  wrote a new file without it being committed passed. It still compares the working tree
  with the index, so `git add` is what makes a local run pass, as before.

## How it was checked

- In `bash:5.2`, a function that runs `false` and then `true`, called as `if ! g`, runs
  on past the `false` and reports success, with `set -euo pipefail` in force.
- On a fresh clone of main in `python:3.12`, with a line appended to an already merged
  record item and committed, `BASE_REF=origin/main bash check_documents.sh records`
  printed "document checks passed: records" and exited 0; the Python script, on the same
  tree, reported that `record.py` exited with 1 and failed the group. The clone was
  shallow, so `signatures.py check` also failed in both, on a commit the shallow history
  lacked; CI clones the full history, so that one does not arise there.
- On main's tree on Windows, every step of every group passed and no generated file
  changed, so the defect was not hiding a failure on main when it was fixed.

# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""The miri matrix: which crates `.github/workflows/miri.yml` runs, and on what.

    python3 .github/scripts/miri_matrix.py <base-ref>   # the crates whose added code holds `unsafe`
    python3 .github/scripts/miri_matrix.py --all        # every crate in the table (a dispatch)

Prints `has_unsafe=<true|false>` and `matrix=<json>` on stdout, in the form
`$GITHUB_OUTPUT` takes; what it found goes to stderr. Needs a clone deep enough to hold
the base.

**Why the crates come from the diff** (2026-10-29). Until this date the workflow held a
fixed matrix of twelve tracking crates and ran all twelve whenever `unsafe` was added
anywhere, which was two hours of runner time (gungnir-fusion-async alone 1 h 47 min on
the 2026-10-28 run for GAP-215), and never the crate the `unsafe` was added to unless it
was one of the twelve. GAP-215's trigger was in gungnir-security, which the matrix did
not contain. The gate is "cargo miri on any PR whose diff contains `unsafe`"
(docs/agentic-workflow.md), and the code miri has to see is the code that was added, so
the matrix is now the crates that gained it. A crate in TABLE below runs with the
targets and skips measured for it; any other crate runs its library and every
integration test, and is given a row here once its long tests are measured.

`unsafe` is looked for in added lines of `.rs` files with everything from `//` to the
end of the line removed, so a comment that mentions it does not count (the scan read
comments until 2026-10-28, and GAP-215's pull request ran the gate three times on one).
"""

import json
import os
import re
import subprocess
import sys

# crate -> (targets, skip). One job per crate, in parallel, none cancelling another
# (GAP-164). miri interprets about a thousand times slower than native code, so each
# crate runs what finishes: its unit tests and the integration suites that finish, with
# the long-run tests left out (D-92). Every code path those tests reach is still reached
# by the unit tests beside them; what is left out is the length of the run, not the
# code. Times are measured under miri (CI, or locally with 16 at once where marked).
#
# Left out, with why:
# - gungnir-metrics motmetrics_diff: 2 h 3 min on CI.
# - gungnir-allocation one_step_oracle: over 90 min on CI on 2026-10-28, when the job
#   reached its 120 min limit and was cancelled with every suite before it passed.
# - gungnir-tracking-service sample_set_replay, scenario_truth_replay,
#   whole_pipeline_replay: hours each; they fail their own 60 s deadlock guard. And the
#   lib test in its skip, which runs the dense-group pipeline and waits ten seconds for
#   its group: added on 2026-10-26 (GAP-015), it first ran under miri on 2026-10-28 and
#   failed at that wait, the same pipeline as fusion-async's dense_group below.
# - gungnir-fusion-async dense_group: over 5 h on CI.
# - gungnir-filters imm_diff, linear_kalman_diff, particle_diff, sqrt_diff: over 10 min
#   each (local), and the lib tests in its skip, over 10 min each (local).
# - gungnir-rfs cphd_diff, lmb_diff, lmb_label_continuity: over 10 min each (local),
#   and the lib tests in its skip.
# A test is added back here when it is measured to finish.
TABLE = {
    "gungnir-core": ("--lib --tests", ""),
    "gungnir-coord": ("--lib --tests", ""),
    "gungnir-association": ("--lib --tests", ""),
    "gungnir-track": ("--lib --tests", ""),
    "gungnir-track-fusion": ("--lib --tests", ""),
    "gungnir-allocation": ("--lib --test bellman_diff --test horizon_first_step", ""),
    "gungnir-intercept-service": ("--lib --tests", ""),
    "gungnir-metrics": ("--lib", ""),
    "gungnir-tracking-service": (
        "--lib --test sensor_position_resolver",
        "--skip tests::a_dense_scan_puts_a_group_on_the_view_and_it_ages_stale_with_nothing_arriving",
    ),
    "gungnir-fusion-async": ("--lib --test bearing --test oos_convergence", ""),
    "gungnir-filters": (
        "--lib --test rts_diff --test nonlinear_diff",
        " ".join([
            "--skip imm::tests::mode_probabilities_stay_a_distribution_over_a_long_run",
            "--skip imm::tests::probability_moves_to_the_mode_that_explains_the_motion",
            "--skip kalman::tests::covariance_stays_psd_over_a_long_run",
            "--skip particle::tests::a_bimodal_cloud_keeps_both_plausible_lumps_and_drops_the_impossible_one",
            "--skip particle::tests::an_update_pulls_the_cloud_toward_the_measurement_and_shrinks_it",
            "--skip sqrt::tests::the_square_root_form_tracks_the_standard_form",
            "--skip sqrt::tests::zero_psd_violations_over_a_hundred_thousand_cycles",
        ]),
    ),
    "gungnir-rfs": (
        "--lib --test phd_diff",
        " ".join([
            "--skip tests::cphd_shows_less_cardinality_swing_than_phd_under_frequent_missed_detections",
            "--skip tests::the_mixture_stays_bounded_over_a_long_dense_run",
        ]),
    ),
}

# A crate not in the table: its library and every integration test.
DEFAULT = ("--lib --tests", "")

UNSAFE = re.compile(r"\bunsafe\b")


def git(*args):
    return subprocess.run(["git", *args], capture_output=True, text=True, encoding="utf-8",
                          check=True).stdout


def workspace_members():
    """The directories `Cargo.toml`'s `members` names."""
    text = open("Cargo.toml", encoding="utf-8").read()
    block = re.search(r"^members\s*=\s*\[(.*?)^\]", text, re.M | re.S)
    if not block:
        raise SystemExit("::error::Cargo.toml has no members block this script can read")
    return set(re.findall(r'"([^"]+)"', block.group(1)))


def package_name(directory):
    text = open(os.path.join(directory, "Cargo.toml"), encoding="utf-8").read()
    m = re.search(r'^\[package\].*?^name\s*=\s*"([^"]+)"', text, re.M | re.S)
    return m.group(1) if m else None


def files_with_added_unsafe(base):
    """Paths of the `.rs` files whose lines added since `base` hold `unsafe` outside a comment."""
    diff = git("diff", "--unified=0", f"{base}...HEAD", "--", "*.rs")
    found, current = {}, None
    for line in diff.split("\n"):
        if line.startswith("+++ "):
            current = line[4:].strip()
            current = current[2:] if current.startswith("b/") else current
            continue
        if not line.startswith("+") or line.startswith("+++") or current is None:
            continue
        code = line[1:].split("//", 1)[0]
        if UNSAFE.search(code):
            found.setdefault(current, []).append(code.strip())
    return found


def crates_for(files):
    members, crates, outside = workspace_members(), {}, []
    for path, lines in sorted(files.items()):
        directory = path.split("/", 1)[0]
        name = package_name(directory) if directory in members and os.path.isfile(
            os.path.join(directory, "Cargo.toml")) else None
        if name is None:
            outside.append(path)
            continue
        crates.setdefault(name, []).append((path, lines))
    if outside:
        print("::error::`unsafe` was added outside a workspace crate, where this gate has no "
              "way to run miri on it; a person decides what checks it: " + ", ".join(outside),
              file=sys.stderr)
        raise SystemExit(1)
    return crates


def matrix(names):
    include = []
    for name in sorted(names):
        targets, skip = TABLE.get(name, DEFAULT)
        include.append({"crate": name, "targets": targets, "skip": skip})
    return {"include": include}


def main(argv):
    if len(argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    if argv[1] == "--all":
        names = set(TABLE)
        print(f"a dispatch: every crate in the table, {len(names)} of them", file=sys.stderr)
    else:
        files = files_with_added_unsafe(argv[1])
        crates = crates_for(files)
        for name, hits in sorted(crates.items()):
            print(f"{name}:", file=sys.stderr)
            for path, lines in hits:
                for code in lines:
                    print(f"  {path}: {code}", file=sys.stderr)
        if not crates:
            print("no `unsafe` in the added code", file=sys.stderr)
        names = set(crates)
    print(f"has_unsafe={'true' if names else 'false'}")
    print(f"matrix={json.dumps(matrix(names), separators=(',', ':'))}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

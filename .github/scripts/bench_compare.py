# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Compare criterion's latest estimates against the saved `main` baseline (gate 6).

Lifted out of `.github/workflows/bench-regression.yml` on 2026-09-16, where it was a
forty-line program embedded in a YAML block scalar behind a bash heredoc. Three things
that buys, and the first is why it moved:

1. **It can be run and tested.** A program inside a workflow is exercised only by
   pushing to main and reading a log; this one takes its root as an argument, so a
   fixture directory proves the comparison arithmetic without a CI round trip.
2. **It no longer depends on which shell the runner resolves.** The heredoc needed
   bash, and on a self-hosted Windows runner `bash` resolved to the WSL shim until the
   runner's `.path` put Git's bash first (`ARCHITECTURE.md` §10 item 134). A plain
   `python3 <script>` invocation has no such dependency.
3. The workflow reads as steps rather than as a program.

Nothing about the comparison itself changed, and the comments below are the ones the
inline version carried, because each records a defect this gate actually had.

Usage: python3 .github/scripts/bench_compare.py [--wrote-baseline] [criterion-root]

`--wrote-baseline` says this run saved the baseline it would compare against, which
is what a run on `main` does. There is then nothing to compare: criterion has already
overwritten `main/` with this run's own numbers, so every ratio is the run against
itself and comes out at exactly 1.000x. Printing those was how the 2026-09-17 run on
`fe97fa3` came to show eleven perfect ratios in a log that reads like a clean
comparison. This counts the benchmarks and says so instead.

Environment: BENCH_REGRESSION_THRESHOLD (fraction, default 0.10),
BENCH_REGRESSION_ENFORCE ("1" to make a reported regression fail; see the workflow
header for why it is off).
"""

import json
import os
import pathlib
import sys


def main(argv: list[str]) -> int:
    threshold = float(os.environ.get("BENCH_REGRESSION_THRESHOLD", "0.10"))
    args = [a for a in argv[1:] if a != "--wrote-baseline"]
    wrote_baseline = len(args) != len(argv[1:])
    root = pathlib.Path(args[0] if args else "target/criterion")
    failed = False
    compared = skipped = 0
    # A run that saved the baseline cannot compare against it: `--save-baseline main`
    # replaced `main/` with this run's own estimates before this script ran, so each
    # ratio would be 1.000x by construction and would say nothing about a regression.
    # Counting what was measured is the honest output for that run.
    if wrote_baseline:
        benchmarks = sorted(root.glob("**/new/estimates.json"))
        for est in benchmarks:
            print(est.relative_to(root).parent.parent.as_posix())
        print(
            f"saved the baseline from {len(benchmarks)} benchmark(s); NOT COMPARED: this "
            "run is the baseline, so a ratio here would be the run against itself. The "
            "comparison happens on the next branch dispatch against this baseline."
        )
        return 0
    # Recursive: criterion writes <group>/<bench>/new/estimates.json for a grouped
    # benchmark and <bench>/new/estimates.json for an ungrouped one. This globbed one
    # level only, matched nothing at all against the real layout, and so compared
    # nothing and passed unconditionally -- a gate that could not fail.
    for est in sorted(root.glob("**/new/estimates.json")):
        name = est.relative_to(root).parent.parent.as_posix()
        base = est.parent.parent / "main" / "estimates.json"
        if not base.exists():
            skipped += 1
            continue
        new = json.load(open(est))["mean"]["point_estimate"]
        old = json.load(open(base))["mean"]["point_estimate"]
        ratio = new / old if old else 1.0
        print(f"{name}: {ratio:.3f}x")
        if ratio > 1.0 + threshold:
            print(f"  REGRESSION: over the {threshold:.0%} threshold")
            failed = True
        compared += 1
    # Say what was measured. A run that compares nothing is the expected state only
    # until main has saved a baseline; after that it means the wiring broke again, and
    # silence is how that goes unnoticed for months.
    print(f"compared {compared} benchmark(s); {skipped} had no baseline to compare against")
    if compared == 0:
        print("no baseline yet: this is expected until a run on main saves one")
    # Advisory unless enforcement is switched on. The ratios above are the output; a
    # regression is still named and still printed, and the only thing that changes is
    # whether an unattributable difference blocks a pull request.
    enforce = os.environ.get("BENCH_REGRESSION_ENFORCE", "0") == "1"
    if failed and not enforce:
        print(
            "NOT FAILING: read the ratios above rather than the exit status; see the "
            "header of bench-regression.yml for what enforcement is waiting on."
        )
    return 1 if (failed and enforce) else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

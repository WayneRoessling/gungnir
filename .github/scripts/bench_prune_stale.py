# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Drop criterion results this run did not produce (gate 6).

The baseline is restored from a cache into `target/criterion` and saved back from it,
so a benchmark that no longer exists keeps its directory for ever: restored, never
rewritten, saved again. Found 2026-09-17, after `snapshot_calls/tracks` and
`snapshot_calls/is_healthy` were replaced by `snapshot_calls/panel_read`. Every run
after that reported "compared 13 benchmark(s)" for eleven benchmarks, and the two
removed ones printed exactly `1.000x`: their old `new/` estimates compared against
their old `main/` estimates, which is a measurement of nothing reported as a pass.

Criterion rewrites `new/estimates.json` for every benchmark it runs, so a benchmark
whose `new/estimates.json` is older than the marker the workflow touches before
`cargo bench` was not run this time. Its whole directory goes, before the comparison
and before the save.

Usage: python3 .github/scripts/bench_prune_stale.py <marker-file> [criterion-root]
"""

import pathlib
import shutil
import sys


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print("usage: bench_prune_stale.py <marker-file> [criterion-root]")
        return 2
    marker = pathlib.Path(argv[1])
    root = pathlib.Path(argv[2] if len(argv) > 2 else "target/criterion")
    if not marker.exists():
        # Without the marker there is no way to tell this run's results from the
        # cache's, and guessing would delete the measurement or keep the ghosts.
        print(f"no marker at {marker}: refusing to guess which results are stale")
        return 2
    started = marker.stat().st_mtime
    kept = dropped = 0
    for est in sorted(root.glob("**/new/estimates.json")):
        bench = est.parent.parent
        name = bench.relative_to(root).as_posix()
        if est.stat().st_mtime < started:
            shutil.rmtree(bench)
            print(f"dropped {name}: not run this time, so its results are the cache's")
            dropped += 1
        else:
            kept += 1
    print(f"kept {kept} benchmark(s) run this time; dropped {dropped} left over from the cache")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

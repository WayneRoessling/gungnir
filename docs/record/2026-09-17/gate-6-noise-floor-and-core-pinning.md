# Gate 6 noise floor and core pinning

Gate 6 (`bench-regression.yml`) runs on `gungnir-rtx-5060ti` and cannot enforce its 10
percent threshold there. This item records the three measurements that show it, a
pinning change that was tried and not adopted, and two defects the measurements found.
`BENCH_REGRESSION_ENFORCE` stays `"0"` and GAP-093 stays open.

Every table below compares code identical to the commit its baseline was saved from, so
every ratio is noise. A ratio is the new mean over the baseline mean.

## 1. Unpinned, the benchmarks as they were (2026-09-16)

Three branch dispatches at `8e03a78` against the `8e03a78` baseline. Ten of twelve
benchmarks crossed 10 percent at least once; the widest moved 0.83 (`ekf`, 1.08 to 1.91).
The full table is in a record item written for the pinning branch below and not merged
with it (`docs/record/2026-09-16/gate-6-benchmarks-pinned-to-performance-cores.md` on
`claude/gate6-pcore-pinning`).

## 2. Pinned to performance cores: tried, not adopted (2026-09-16)

The host is a hybrid CPU, so the first hypothesis was the scheduler moving benchmarks
between core classes. Branch `claude/gate6-pcore-pinning` (`ecaaa41`) added
`run_on_performance_cores.ps1`, which reads the performance cores from
`GetSystemCpuSetInformation` (logical 0, 1, 6 to 9, 18 and 19 here) and runs `cargo bench`
restricted to them. The pinning was verified: a grandchild process reported the mask.
Three dispatches against the `4724b50` baseline:

| benchmark | r1 | r2 | r3 | range | unpinned range |
|---|---|---|---|---|---|
| ekf | 1.576 | 1.690 | 0.908 | 0.78 | 0.83 |
| hungarian 100x100 | 1.395 | 1.893 | 0.946 | 0.95 | 0.49 |
| hungarian 100x400 | 1.302 | 1.197 | 1.318 | 0.12 | 0.33 |
| hungarian 200x200 | 1.152 | 3.065 | 1.178 | 1.91 | 0.56 |
| journal_append | 1.037 | 1.198 | 1.078 | 0.16 | 0.11 |
| node_ingest | 1.176 | 1.459 | 1.207 | 0.28 | 0.49 |
| node_journal sync | 1.030 | 0.936 | 1.038 | 0.10 | 0.04 |
| phd | 1.769 | 3.002 | 1.647 | 1.36 | 0.29 |
| is_healthy | 1.104 | 0.906 | 0.753 | 0.35 | 0.61 |
| tracks | 1.305 | 1.134 | 0.892 | 0.41 | 0.22 |
| startup | 1.081 | 1.988 | 1.050 | 0.94 | 0.19 |
| tick | 1.122 | 1.220 | 0.865 | 0.36 | 0.23 |

Eight of twelve varied more pinned than unpinned. Run 2 was slow across many benchmarks
at once, which is the signature of other load on the machine, not of one benchmark on
the wrong core, and restricting the benchmarks to eight cores leaves everything else free
to run on those same eight. **Not adopted**: a PowerShell launcher that does not narrow the
spread is complexity with no return. The branch stays unmerged as the evidence.

Two things the launcher taught are worth keeping for any later Windows wrapper. Under
`powershell -File`, a `param()` block with `ValueFromRemainingArguments` refused the
wrapped command, and splatting an argument array with `@rest` made Windows PowerShell 5.1
split `--version` into `-` and `-version`.

## 3. The benchmarks were part of the problem

The pinned run's raw criterion output showed four benchmarks at a nanosecond or less: two
placeholders and two field reads, one of an empty snapshot. #125 replaced them with real
work (`2026-09-16/sub-nanosecond-benchmarks-replaced-with-real-work.md`).

## 4. Re-measured on the real benchmarks (2026-09-17)

Three dispatches at `8ec262c` against the `8ec262c` baseline. A first attempt at
`97d7861` finished two runs before #126 merged and moved main; its third run was cancelled,
because it would have compared against the new baseline.

| benchmark | r1 | r2 | r3 | range |
|---|---|---|---|---|
| ekf | 1.202 | 0.926 | 1.712 | 0.79 |
| hungarian 100x100 | 0.970 | 0.873 | 1.488 | 0.62 |
| hungarian 100x400 | 1.320 | 1.782 | 1.750 | 0.46 |
| hungarian 200x200 | 1.196 | 1.040 | 1.740 | 0.70 |
| journal_append | 0.842 | 0.909 | 1.150 | 0.31 |
| node_ingest | 1.338 | 1.721 | 1.952 | 0.61 |
| node_journal sync | 1.046 | 1.504 | 1.582 | 0.54 |
| phd | 1.120 | 1.346 | 1.261 | 0.23 |
| panel_read | 1.548 | 1.748 | 1.030 | 0.72 |
| startup | 0.990 | 1.165 | 0.899 | 0.27 |
| tick | 1.193 | 2.229 | 1.610 | 1.04 |

The two `97d7861` runs agree: ratios from 0.35 to 1.60 there. So replacing the timer-only
benchmarks did not narrow the spread either. Every one of the eleven crossed 10 percent at
least once. Two things are visible in the table:

- **Whole runs move together.** Run 3 was 15 to 95 percent slower on nine of eleven. The
  noise is the state of the machine during the run, which is also the owner's
  workstation.
- **The baseline is one sample.** 25 of these 33 ratios are above 1, so the run that saved
  `8ec262c` was probably a fast one, and every comparison inherits that one sample.

## 5. Two defects found, fixed with this item

**`panel_read` read a different number of tracks each run**: 17, 36, 36, 36 and 22 in
the five comparison runs above, and 36 in the run that saved `97d7861`. #125's wait took the first non-empty snapshot,
and how far the asynchronous pipeline had got by then varied. The benchmark now polls,
without advancing the clock, until the track count and the pipeline counters have not
changed for a second. Three local runs each read 36 tracks after 9 epochs. The budget
test, `snapshot_calls_meet_their_budget`, keeps taking the first snapshot, which is right
for a ceiling and wrong for a comparison.

**Removed benchmarks lived on in every baseline.** The cache restores `target/criterion`
and the save writes it back, so after #125 `snapshot_calls/tracks` and `/is_healthy` were
reported as "compared 13 benchmark(s)" for eleven benchmarks, at exactly 1.000x.
`.github/scripts/bench_prune_stale.py` now drops every benchmark whose
`new/estimates.json` predates a marker touched before `cargo bench`, before the
comparison and before the save. It was checked against a fixture tree: the stale
benchmark dropped, the fresh ones kept, and no marker refused.

## What this leaves

A 10 percent threshold is not enforceable on this machine as it is used. The ranges above
mean a regression under about 2x cannot be told from noise. What would change that is an
owner decision, not a fix: a baseline from several runs, criterion's own statistical
comparison in place of a single-sample ratio, a quiet machine, or a wider threshold,
which would widen a gate criterion and need a decision record of its own.

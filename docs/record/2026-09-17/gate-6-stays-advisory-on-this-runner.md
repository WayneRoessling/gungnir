# Gate 6 stays advisory on this runner

The owner decided on 2026-09-17 that gate 6 (`bench-regression.yml`) never enforces its
threshold on `gungnir-rtx-5060ti`, and that the runner is not installed as a service.
Filed as D-62 and D-63 in `../../mission/gap-analysis/data/decisions.yaml`. GAP-093
closes with them. `BENCH_REGRESSION_THRESHOLD` stays at `0.10` and
`BENCH_REGRESSION_ENFORCE` at `"0"`; no pass criterion in
`../../verification-capability-table.md` changes, because gate 6 owns none.

**D-62 was decided against a measurement, not an estimate.** The evidence is in
`gate-6-noise-floor-and-core-pinning.md`: three dispatches of unchanged code against one
baseline, after pinning to the performance cores had been tried and dropped and after the
four timer-only benchmarks had been replaced with real work. Ranges reached 1.04 in ratio
and every benchmark crossed 10 percent at least once, so nothing smaller than roughly a
2x regression is distinguishable here. The owner declined the three ways forward the
record listed. A wider threshold would make the gate say less than it says now; a
multi-run baseline and criterion's own statistical comparison are engineering spent
against a host whose noise is the owner using the machine; a dedicated quiet runner is
new hardware and out of scope for this release.

**What still catches a regression.** The ratios are printed, with every threshold breach
named, for a person to read. The frame budgets in
`../../../gungnir-app/tests/frame_budgets.rs` are assertions against absolute numbers and
are unaffected by this: they fail a build, and they are what `../../performance-budgets.md`
is checked by. Gate 6's job is the comparison and the baseline, and it keeps doing both.

**D-63 turns on what depends on the runner, which is less than it looks.** No release
artifact is built there: `release.yml` builds `gungnir-app` on `windows-latest` and
`gungnir-node` on `ubuntu-latest`, and its assurance and packaging jobs are hosted too.
The only two workflows that use the self-hosted runner are gate 6, now advisory, and
`gpu-fusion.yml`, which is manual dispatch only. So an offline runner delays
re-evidencing the GPU path row and any benchmark comparison, and blocks nothing that
ships. It went offline four times during the 2026-09-16 and 2026-09-17 runs, which is
the cost the owner accepted rather than install a service under an account of its own.

**Four documents said enforcement was coming, and now say otherwise.** The workflow
header had named the condition ("set BENCH_REGRESSION_ENFORCE to 1 once this runs on the
self-hosted machine") and promised the `pull_request` trigger back with it;
`agentic-workflow.md` gate 6 repeated that condition; `gungnir-capabilities.md` claimed a
"hard-fail p99 check"; and `gungnir-workspace-structure.md` still listed gate 6 as
running on every pull request, which stopped being true on 2026-09-09. All four now state
the decision. This is the half of such a decision that is easy to skip: a promise left in
four places reads as a plan, and someone would have tried to keep it.

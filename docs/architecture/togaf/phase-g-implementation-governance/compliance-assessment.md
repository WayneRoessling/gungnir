# Compliance assessment

Status: **run 2026-09-04** against the workspace as it stands. Phase G. The contract set
applied to the whole repository rather than to one change.

This is a real run, not a template. Each check below was executed; the counts are what
the tools returned. Findings are numbered CA-Fn and each one becomes a gap.

## 1. Results

| Contract | Check performed | Result |
|---|---|---|
| C-11 dependency direction | Walked every crate-to-crate edge in all 50 manifests and compared against the layer ordering and the drawn graph | **Pass.** 147 edges. Four raised by the first pass, all four correct against `ARCHITECTURE.md` §7.1: the 3D-data crates sit below the geospatial and analytics crates, and the verification crates depend on what they verify. Finding CA-F3 records that the prose in the standards document does not say this and the graph does |
| C-07 one owning crate | Scanned every definition of the nine shared types across all crate sources | **Pass.** Each of the nine defined exactly once, in the crate that should own it |
| C-14 recorded stack | Compared the workspace dependency table against the recorded stack | **Pass.** Twenty dependencies, all recorded. The identifier crate is decided and absent from the manifest, which is consistent with its gap being open |
| AP-12 and C-03, part one | Counted `unwrap()` and `expect()` outside test modules across 148 source files | **Pass.** Zero |
| AP-12 and C-03, part two | Counted the honest-status markers | **Observed.** Seventeen not-implemented variants; thirty-two `todo!()` bodies; the pipeline flag is false and gates the tracking service's health. Nothing proves the `todo!()` set is unreachable: finding CA-F2 |
| C-05 and doc integrity | Resolved every document citation and section citation from code | **Pass.** 143 section citations across five documents, all resolve to a real heading; ten distinct documents cited, all exist |
| Documentation integrity | Resolved every relative link in every Markdown file, and every backticked relative document path in the newest folders | **Pass.** 291 files, zero broken links; 213 backticked paths, all resolve |
| AP-16 gates | Checked whether the gates run | **Not verifiable.** The workflow files exist; no runner is hosted, so no gate has ever run. Tracked by GAP-061, not a new finding |
| The whole set | Checked whether any of the above runs automatically | **Fail.** Every check above was an ad hoc script written for this run. Finding CA-F1 |

Six passes, one observation, two findings, one not verifiable.

## 2. Findings

### CA-F1 The contract checks are not automated

**What.** Five contracts are mechanically checkable today, and today they were checked by
scripts written for this assessment and then discarded. Nothing in continuous integration
enforces dependency direction, single ownership of shared types, the unwrap policy, the
citation and link integrity, or the recorded stack.

**Why it matters.** A contract enforced by a person who remembers to run a script is a
preference. The capability assessment scores compliance verification at level 2 for
exactly this reason, and principle erosion is the risk it names.

**Closing action.** Add a continuous-integration job that runs the five checks and fails
the build. The job is small: each check is between ten and thirty lines. Filed as
GAP-081, increment 2, alongside the existing registry and test-track jobs.

### CA-F2 Nothing proves the `todo!()` bodies are unreachable

**What.** The standards document reserves `todo!()` for functions nothing calls and
requires a named error variant on any reachable path. Thirty-two `todo!()` bodies exist.
The claim that none is reachable from a runtime path rests on inspection.

**Why it matters.** It is contract C-03, and C-03 supports AP-02, the principle that is
not dispensable. A reachable `todo!()` is a panic in front of an operator, which is the
loudest possible violation of honest status.

**Closing action.** Either a call-graph check from each binary's entry point, or, more
simply, convert every `todo!()` that a public function can reach into a not-implemented
variant and assert that the remainder are private and uncalled. Filed as GAP-082,
increment 2.

### CA-F3 The standards prose does not place the 3D-data crates in the ordering

**What.** The standards document §1.1 describes the layer order as core, then the model,
then the facades, then the productization crates, then deployment, then the interface.
Two real edges, from the geospatial crate and from the analytics crate to the data crate,
fit the drawn graph in `ARCHITECTURE.md` §7.1 but not that sentence. The first pass of
the dependency check flagged them as violations, and they are not.

**Why it matters.** Low severity and worth fixing anyway. An automated check written from
the prose rather than from the graph produces false alarms, and false alarms are how a
check gets switched off.

**Closing action.** Add one sentence to §1.1 placing the 3D-data crates, and write the
automated check in CA-F1 against the drawn graph rather than a layer ranking. Documentation
change, folded into GAP-081.

## 3. What this assessment did not check

Stated so the six passes are not read as more than they are:

- **C-01, no execution without a decision.** Its test does not exist yet (GAP-039). Today
  the claim rests on the absence of an execution path, which is strong evidence and not a
  test.
- **C-02 and C-13**, honest status beyond the flag, and wiring-not-logic in the binaries.
  Both need a reader, not a script.
- **C-04, C-08, C-09, C-10**, audit attributability, provenance completeness, journal
  determinism, and releasability. Each depends on subsystems that are not wired yet, so
  the check would pass vacuously.
- **Whether the architecture is right.** Compliance says the code obeys the description.
  It says nothing about whether the description matches the mission, which is the
  stakeholder-engagement weakness recorded in the capability assessment.

## 4. Cadence

The assessment runs at every increment boundary and whenever a principle or contract
changes. Once GAP-081 lands, the mechanical part runs on every change and this document
records only what a person had to judge.

## 5. Comparison against the previous run

None. This is the first.

## Traceability

`architecture-contracts.md` for the contract set;
`../preliminary/architecture-principles.md` for the principles;
`../preliminary/capability-assessment.md` for the maturity these findings inform;
`../../../mission/gap-analysis/gap-register.md` for GAP-081 and GAP-082;
`../../../../ARCHITECTURE.md` §10 for the open-items ledger.

# Architecture contracts

Status: first draft 2026-09-04; **signed by the owner 2026-09-05.** Phase G. What every
change must honour, expressed as checks rather than as prose, because a contract nobody
can run is a preference.

Three of the seventeen are signed with their enforcement explicitly missing rather than
implied: C-01 and C-04 have no automated check yet (GAP-039, GAP-059), and C-16's merge
gate has no mechanism while the workspace is not under version control. C-07 and C-11
were run against the tree on the day of signature and both passed. The findings table in
`../preliminary/architecture-principles.md` carries the detail.

Seventeen contracts, one per principle. Each names how it is checked, whether that check
is automatable, and what a violation does.

## 1. The contract set

| Id | Contract | Principle | Check | Automatable | On violation |
|---|---|---|---|---|---|
| C-01 | No code path executes an engagement, an effector command, or a reserved identity declaration without a recorded human decision | AP-01 | `gungnir-app/tests/no_execution_without_decision.rs` (a source scan of every executing construction and a driven desktop proving the order on the record), plus the human-owned tier on the policy and command crates | Yes, on every `cargo test` since 2026-09-06 (GAP-039); reserved identity declarations have no execution path yet | Reject. Not dispensable |
| C-02 | No health flag, connection state, or test reports a subsystem as working when it is not | AP-02 | Reviewer checklist; the pipeline flag gates the tracking service's health | Partly | Reject. Not dispensable |
| C-03 | Unimplemented capability returns a named error variant on every reachable path; `todo!()` only where nothing calls | AP-02, AP-12 | `gungnir-app/tests/no_reachable_todo.rs` (no `todo!` in shipped source) and `architecture_compliance.rs` (the unwrap policy) | Yes, on every `cargo test` (GAP-082, GAP-081) | Reject |
| C-04 | Every decision, override, promotion, and configuration change is attributable to a person, a role, and a mission time | AP-03 | `gungnir-app/tests/audit_trail.rs`: a decision, a sensor command, a requirement and a review each leave a row, attributed to the verified operator or to nobody | Yes, on every `cargo test` since 2026-09-06 (GAP-059); promotion joins with its control (GAP-053) | Reject |
| C-05 | No controlled specification, proprietary material, or customer data enters the repository | AP-04 | Reviewer checklist; sourcing policy in the test-track documents; document citations resolve (`architecture_compliance.rs`) | The citation half on every `cargo test` (GAP-081); the sourcing half stays a review item | Reject and remove |
| C-06 | Profile differences are configuration and wiring, never conditional logic in a domain crate | AP-05 | Reviewer checklist | Partly | Reject |
| C-07 | A shared type has exactly one definition; everything else re-exports | AP-06 | `gungnir-app/tests/architecture_compliance.rs`, a definition scan over eighteen shared primitives | Yes, on every `cargo test` (GAP-081) | Reject |
| C-08 | Detections, tracks, and derived products carry source, receipt time, and producer | AP-07 | Schema review; the identification evidence source string | Partly | Reject |
| C-09 | Journal writes are sequenced and mission-timed, and replay reproduces the picture | AP-08 | The journal round-trip test; replay determinism | Yes | Reject |
| C-10 | Releasability is carried on the data, not inferred from the channel | AP-09 | Schema review; the egress check on the assembled request | Partly | Reject |
| C-11 | No dependency edge exists that `ARCHITECTURE.md` does not draw | AP-10 | `gungnir-app/tests/dependency_graph.rs`: the manifests against a layer table transcribed from §7.1, acyclicity, direction, and every recorded edge present | Yes, on every `cargo test` (GAP-081) | Reject and stop for a human |
| C-12 | A capability in the verification table has a trait, and the harness is written against the trait | AP-11 | Reviewer checklist against the table row | Partly | Reject |
| C-13 | The two binaries contain wiring, not domain logic | AP-13 | Reviewer checklist, explicitly listed for both binaries | No | Reject |
| C-14 | Every dependency is recorded in the standards document §2.9 and pinned once | AP-14 | `architecture_compliance.rs`: every `[workspace.dependencies]` crate named in a standards document; `cargo deny` for licence, advisory, and source | The recorded-list half on every `cargo test` (GAP-081); `cargo deny` waits on hosted runners | Reject |
| C-15 | The graphics and compute contexts stay separate and separately pinned | AP-15 | Manifest and feature-flag review | Partly | Reject |
| C-16 | No verification gate is waived by either agent | AP-16 | The gates are merge conditions | Yes, once hosted | Reject |
| C-17 | No pass criterion or measure target is widened to make a test pass | AP-17 | Diff review of the verification table and the measures catalogue | Yes, a diff check | Reject. Change request instead |

Fourteen of seventeen are automatable in whole or in part. Three, C-05, C-13, and the
reviewer's part of C-06, rest on judgement, and they are the three where a second reader
would help most.

## 2. How the contracts are applied

They are not a separate audit. They are the reviewer agent's checklist, run on every
change, described in
[`../../../agentic-workflow.md`](../../../agentic-workflow.md):

| Stage | Contracts it applies |
|---|---|
| Implementer agent, before writing code | C-12, C-17: the capability-table row first |
| Reviewer agent, adversarial pass | C-02, C-03, C-05, C-06, C-07, C-08, C-11, C-13, C-14 |
| Verifier gate | C-09, C-16, and C-01 once its test exists |
| Human sign-off, low-trust tier | C-01, C-02, C-04, C-10 |

Per increment, the whole set runs across the repository rather than one change: that is
[`compliance-assessment.md`](compliance-assessment.md).

## 3. The two that cannot be dispensed

C-01 and C-02. Every other contract can in principle be suspended by a written
dispensation with an expiry. These two cannot, because the product's claim rests on them
and a temporary exception to either is indistinguishable, from the outside, from the
product not being what it says it is.

A change that needs either is a product decision taken as a change request under phase H,
with the owner deciding whether the product is still the same product.

## 4. Contracts and the two new subsystems

The assistant and machine-learning inference each add one contract-shaped constraint that
is enforced structurally rather than by review:

| Subsystem | Constraint | Mechanism |
|---|---|---|
| Assistant | No tool changes state, and the crate cannot reach a crate that would | A dependency-list test, which is C-11 applied to a specific pair |
| Inference | A model produces evidence, never a decision | The consuming crate's type: identification evidence and alerts, not verdicts |

Both reduce to C-01. Neither introduces a new principle, which is the point: a new
subsystem that needed a new principle would be a signal that it does not belong.

## 5. What a violation actually does

Reject means the change does not merge. It does not mean the change is wrong: three of
the four dependency-direction alarms raised during today's assessment were the checker's
model being wrong rather than the code. **A contract violation stops the change and
raises it to a human; it never silently rewrites either the code or the contract.**

## Traceability

`../preliminary/architecture-principles.md` for the principles;
`../../../agentic-workflow.md` for the pipeline that runs these;
`../preliminary/governance-framework.md` for decision rights and dispensations;
`compliance-assessment.md` for the first full run.

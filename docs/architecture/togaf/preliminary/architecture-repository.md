# The architecture repository

Status: first draft, 2026-09-04. Where architecture information lives, in TOGAF's own
classes, and the rule that keeps it from being duplicated.

**There is no architecture tool.** The repository is the Git repository. Every class
below is a folder or a file in it, versioned with the code it describes. That is the
whole point: an architecture description that lives beside the code and fails the build
when it disagrees with it.

## 1. The classes

| TOGAF class | Where | Owner | What it holds |
|---|---|---|---|
| Architecture Metamodel | [`../../uaf/model/elements.yaml`](../../uaf/model/elements.yaml) and [`relationships.yaml`](../../uaf/model/relationships.yaml) | Architecture agent, generated in part | Ten element kinds with stable identifiers, and typed relationships between them |
| Architecture Landscape | [`../../uaf/`](../../uaf/) | Architecture agent | Fifty-eight views across ten UAF domains, plus five traceability matrices |
| Standards Information Base | [`../../uaf/standards/Sd-Tx.md`](../../uaf/standards/Sd-Tx.md), the schema catalogue in `gungnir-interop`, and [`../../../release-governance.md`](../../../release-governance.md) | Owner | Fifteen standards with status; the licence, advisory, and source policy |
| Reference Library | [`../../../README.md`](../../../README.md) reading order, items 1 to 17 | Owner | The existing design, standards, and verification documents |
| Governance Log | `ARCHITECTURE.md` §10, [`../../../mission/gap-analysis/decisions-needed.md`](../../../mission/gap-analysis/decisions-needed.md), and [`../phase-h-change-management/change-management.md`](../phase-h-change-management/change-management.md) | Owner | Resolved defects, open items, decisions D-01 to D-17 and D-B1 to D-B12 with outcomes |
| Architecture Capability | [`capability-assessment.md`](capability-assessment.md), [`governance-framework.md`](governance-framework.md), [`../../../agentic-workflow.md`](../../../agentic-workflow.md) | Owner | Who does architecture work, how it is reviewed, what maturity is claimed |
| Solutions Landscape | The crate manifests and [`../../../architecture.md`](../../../architecture.md) | Generated from code | What is actually built, per crate, with its verification row and status |
| Requirements Repository | [`../requirements-management/`](../requirements-management/) | Architecture agent | Requirements with identifiers, sources, and traceability |

## 2. The rule against duplication

**A fact has exactly one home, and every other document links to it.** The homes:

| Fact | Home |
|---|---|
| The dependency graph | The crate manifests, drawn in `ARCHITECTURE.md` |
| Pass criteria and tolerances | `verification-capability-table.md` |
| What is broken or undecided | `ARCHITECTURE.md` §10 |
| What is missing against the mission | The gap register |
| Element identifiers and relationships | The UAF registry |
| Measure targets | `mission/measures.md` and the measures catalogue |
| Business figures and assumptions | `business/financial-model.md` and its spreadsheet |
| Approved dependencies | `agentic-coding-standards.md` §2.9 |

A TOGAF document that needs one of these cites it. Where a phase document appears to
restate a fact, it is summarizing for a reader and the citation is on the same line, so
the summary can be checked against its home.

## 3. What is generated and what is written

| Generated | By | Check |
|---|---|---|
| The registry's resource section and `uses` edges | `../../uaf/tools/build_uaf.py` from the crate manifests | Continuous integration job `uaf-registry` |
| Eight UAF views and five traceability matrices | The same tool | The same job |
| Ten operational process views and ten scenario views | The same tool from the mission documents | The same job |
| The gap register, coverage matrix, technical gap map, closure roadmap | The gap generator | Cycle and increment-ordering checks inside the generator |
| Test-track catalogue, scenarios, and sample data | `../../../test-tracks/tools/` | Continuous integration job `test-tracks` |
| The financial model | The workbook builder | A formula evaluator that recomputes all 196 formulas |

Everything else is written. The distinction matters because a generated document must
never be edited by hand: the next run silently reverts it.

## 4. Retention and versioning

- Nothing is deleted. An element that is no longer used gets `status: retired` and stays,
  because a retired identifier that is reused is worse than an obsolete one that is kept.
- Identifiers are never renumbered. `ARCHITECTURE.md` §1 to §10 in particular are cited
  from Rust doc comments; the citation check in the compliance assessment verifies all
  143 of them resolve.
- A document that is superseded is deleted only together with re-pointing every citation
  to it, in the same change.
- The pre-consistency snapshot of the original document set is archived under
  `docs/old/` and is cited by nothing.

## 5. What the repository does not hold

No customer data, no controlled specification, no key material, and no credentials
(AP-04). Model weights are not stored here either: the machine-learning plan keeps
training data and artefacts in a separate repository with its own retention policy, and
only the manifest that names a model version enters this one.

## Traceability

`../../uaf/README.md` for the model and the generator;
`../../../README.md` for the reference library;
`../../../plans/README.md` for the plan set that produced most of it.

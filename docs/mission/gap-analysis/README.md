# Capability design gaps

Deliverables of [plan 05](../../plans/05-capability-design-gaps.md): one prioritized
register of the gaps between what the missions need (`../capabilities/`, plan 04) and
what Gungnir is designed and built to provide. The register's data is YAML under
[`data/`](data/); `tools/gen_gaps.py` renders every document in this folder from it, and
technical gaps cite `../../record/` items (the former `ARCHITECTURE.md` §10) rather than
duplicating them.

Status: first draft 2026-09-04. Severity, effort, and target values are proposals;
the owner scores and the engineering reviewer sizes before the register is used for
planning.

| File | Content |
|---|---|
| [`gap-register.md`](gap-register.md) | The register: a summary table and one entry per gap with type, capability, description, evidence, scores, impact, closing action, target increment, owner, status, references, dependencies |
| [`coverage-matrix.md`](coverage-matrix.md) | Every leaf capability with its design coverage and implementation coverage, the evidence, and the gaps that follow |
| [`technical-gap-map.md`](technical-gap-map.md) | The technical gaps by architecture layer, each pointing at its `../../../ARCHITECTURE.md` §10 item or verification-table row; the §10 open items and the gaps that carry them; integration gaps |
| [`closure-roadmap.md`](closure-roadmap.md) | Gaps by target increment in dependency-respecting priority order, the critical path, counts |
| [`decisions-needed.md`](decisions-needed.md) | Scoping, policy, and architecture decisions that gaps wait on, with the gaps each unblocks and a suggested order |

## Method

1. **Coverage.** For each of the 56 leaf capabilities, design coverage was assessed
   from the architecture and the crate map (does a component own every part of the
   capability) and implementation coverage was taken from the capability-to-crate
   matrix. Evidence is recorded per capability.
2. **Mission gaps.** Where design coverage is less than full, a mission gap was
   written; where closure needs a decision rather than engineering, the decision is
   in `decisions-needed.md` and the gap depends on it.
3. **Technical gaps.** Every `../../../ARCHITECTURE.md` §10 open item, every draft row in
   `../../verification-capability-table.md` §2 that a capability needs, the risks in
   `../../gungnir-capabilities.md` §9, and the integration gaps the coverage pass
   exposed (components that exist but are not connected) became technical gaps.
4. **Scoring** per the scheme below; the values are the drafting agent's proposals.
5. **Roadmap.** Closures were placed against increments I2 to I4 consistent with the
   capability roadmap, checked for cycles and for dependencies that would cross an
   increment boundary the wrong way.
6. **Reconciliation with §10.** The gaps that §10 did not already list are referenced
   from a §10 bullet by identifier, so the open-items ledger stays complete without
   duplicating the register.

## Scoring scheme

- **Severity** (1 to 5): consequence for the mission thread if the gap remains.
  5: the thread cannot be executed; 4: a step fails or a measure of effectiveness
  cannot be met; 3: degraded with a workaround; 2: quality or workload; 1: cosmetic.
- **Reach**: the number of mission threads (of ten) that the gap's capability serves,
  from `../capabilities/capability-to-thread-matrix.md`; roles affected are named in
  the impact statement.
- **Effort**: S (days), M (one to three weeks), L (one to two months), XL (more than
  two months), agent-assisted with the human-owned reviews in
  `../../agentic-workflow.md`.

  **Calibration note, 2026-09-05.** Four gaps have now been closed or half-closed, and
  the measured times sit well inside their bands: GAP-085 (S) took about an hour;
  GAP-048 (M), the five engineering scenarios of GAP-016 (M), and the desktop tick
  harness half of GAP-056 (M) each took roughly one working session. The technical lead
  chose to record these rather than recalibrate on a sample of four. The likely
  explanation is that the band widths assume the human-owned review cost in
  `../../agentic-workflow.md`, and none of those four sits in a low-trust tier -- so the
  scale may be about right for human-owned gaps and generous for the rest. It matters
  because the roadmap orders by priority and then by effort, so a systematically wide
  band mis-orders work *within* a priority tier rather than across tiers. Revisit when
  six to eight closures are on the record.
- **Priority** = severity times reach. The roadmap orders by priority, then by
  effort smaller first, subject to dependencies.

  **Limitation, recorded 2026-09-05.** Reach counts mission threads, which is not the
  same as blast radius. A gap whose capability serves one thread but whose failure is
  cross-cutting scores low: GAP-081 (architecture compliance checks not automated) has
  reach 1 and therefore priority 3, the lowest in its increment, while its consequence
  is that no architecture contract is mechanically enforced anywhere in the workspace.
  The technical lead kept the formula, because it stays comparable across all 85 gaps
  and a special case would make priority no longer derived. Read governance and
  verification gaps against this note rather than against their priority alone.

## Identifiers

- `GAP-nnn`: a gap. Stable; never reused. New gaps are appended.
- `D-nn`: a decision the owner must take. Stable likewise.
- Types: **Mission** (the design does not cover, or covers part of, a needed
  capability), **Technical** (designed, but implementation, verification, or
  integration is incomplete). Decision-type entries live in `decisions-needed.md`.
- Status: Open, Planned (a plan in `../../plans/` or a scheduled increment owns it),
  In progress, Closed. A gap has no status field: its status is the `status` of the last
  entry in its `history`, and the entry's date says when.

## Maintenance

Edit the YAML in `data/` and run `python tools/gen_gaps.py`; never edit the generated
documents. CI runs the generator with `--base` against the base branch, and refuses a
change that edits or removes a merged history entry.

- When a gap's status changes, append an entry to its `history`: the date, the new
  status, and a note of at most 120 words that names what changed and links the
  `../../record/` item or pull request holding the explanation. The closing action and
  evidence fields are current state and are edited in place.
- When a decision is taken, set its `resolved` date and `outcome` in `data/decisions.yaml`
  (an amendment appends to `amended`), append a history entry to each gap it moves, and
  explain a technical decision in a `../../record/` item.
- When a new gap is found, append it to `data/gaps.yaml` with the next identifier, one
  history entry, and its evidence; the coverage matrix and roadmap follow from the data.
  Check the identifier against `origin/main` before pushing: two branches can take the
  same one, and the generator refuses a duplicate only once both have merged.
- The coverage matrix must keep the property that every capability with less than
  full design or implementation coverage has at least one gap.
- The register stays in Markdown until the repository is hosted (plan 05 open
  question); it remains the source if an issue tracker mirrors it.

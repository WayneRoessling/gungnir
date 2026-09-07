# Requirements repository

Status: first draft, 2026-09-04. How requirements are stored, traced, and changed, and
what is deliberately not built.

## 1. Where a requirement lives

There is no requirements tool, for the same reason there is no architecture tool. A
requirement lives in exactly one of four places, and the specification points at it:

| Kind of requirement | Home | Why there |
|---|---|---|
| What a capability must do | `../../../mission/capabilities/capability-statements.md` | Written with the mission, indexed by the taxonomy everything else uses |
| How well it must do it | `../../../mission/measures.md` and the measures catalogue | Targets agreed by the owner in one place |
| How fast it must be | `../../../performance-budgets.md` | Budgets change together; one number, one home |
| What standard it must meet | `../../uaf/standards/Sd-Tx.md` and the schema catalogue | The standards information base |

[`architecture-requirements-specification.md`](architecture-requirements-specification.md)
adds the identifier and the traceability. It restates a statement only in the compressed
form needed to be readable, always with the source on the same row.

## 2. The identifier scheme

`REQ-<category>-<nn>`, categories F functional, D data, I interoperability, S security, P
performance, U usability, C constraint. Sixty-four allocated (the specification's §8 total said fifty-eight while its tables held sixty-four; corrected 2026-09-06 when the UAF registry began generating its requirements from those tables, GAP-083).

Rules, the same three that govern every other identifier in this project:

1. Never renumber.
2. Never reuse. A withdrawn requirement gets a withdrawn status and stays.
3. A requirement identifier is not a work item. Work items are gaps; a requirement points
   at the gap that will satisfy it.

## 3. The traceability chain

The chain that already existed, generated and checked by the registry tool:

```
mission thread -> operational activity -> capability -> service -> resource (crate) -> standard
```

The chain this specification adds:

```
requirement -> source document
           -> UAF view
           -> crate or gap
           -> verification gate or measure
```

The join between them is the capability identifier, which both chains carry. That is why
the specification's source column names a capability wherever one exists: it is what makes
the two chains one chain.

## 4. What is not yet traced, and the gap for it

**Code does not cite requirement identifiers.** A crate's doc comment cites the
verification-table row and the design document section, which the compliance assessment
checked and found sound across 143 citations. It does not cite a requirement, because
requirement identifiers did not exist until today.

Two ways to close it, and the cheaper one is chosen:

- Add the requirements to the element registry as an eleventh element kind, with
  `satisfies` relationships to capabilities and crates, so the existing generator checks
  them and the existing traceability matrices extend to cover them.
- Or add requirement citations to doc comments and check them the way document citations
  are checked.

The first is preferred: it reuses a generator and a continuous-integration job that
already exist, and it keeps the requirement text out of the code. Filed as GAP-083,
increment 3.

## 5. Change history

| Date | Change |
|---|---|
| 2026-09-04 | REQ-F-01 to REQ-C-10 created, 58 requirements, from the capability statements, measures, budgets, standards, decisions, and principles that already existed |

Subsequent changes are recorded here with the change request that caused them, per
[`../phase-h-change-management/change-management.md`](../phase-h-change-management/change-management.md).
A requirement changes only through that process, because a requirement that drifts
silently is worse than one that is wrong: the tests still pass and nobody notices.

## 6. Requirements that conflict

None recorded today. The two tensions most likely to become conflicts:

- **Authority enforcement against usability under saturation.** REQ-S-02 wants authority
  enforced per role, class, and layer; REQ-U-01 and the queue requirements want a decision
  made in seconds. D-15 resolved one case, hostile uncrewed aircraft at the point layer,
  by pre-delegation. The general case is unresolved and will surface as a conflict when
  the queue is built.
- **Releasability enforcement against the assistant's egress.** REQ-D-07 and REQ-S-09
  both constrain what leaves the system, from different directions. They agree today
  because both are checked on the assembled outbound artefact rather than on intent.

Recording tensions before they become conflicts is the point of doing this at all.

## Traceability

`architecture-requirements-specification.md`; `../../uaf/README.md` for the registry and
its generator; `../preliminary/architecture-repository.md` for where every class of fact
lives; `../phase-h-change-management/change-management.md` for how a requirement changes.

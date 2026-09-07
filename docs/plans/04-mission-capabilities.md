# Plan 04: Mission capabilities

## Purpose

Define the mission capabilities Gungnir must provide, as statements an operator or
commander would recognize, with measures and maturity targets, and trace each to the
mission threads that need it and the crates that provide it. This is the operational
view of capability; `docs/gungnir-capabilities.md` remains the solution view of what
each crate does.

## Scope

In scope: a capability taxonomy, one statement per capability with measures of
effectiveness and performance, maturity targets per engineering increment,
traceability to mission threads (plan 02) and to crates, and a capability roadmap.

Out of scope: the design gaps themselves (plan 05) and solution descriptions (the
existing capability reference).

## Inputs

- `docs/mission/mission-threads.md`, `vignettes.md`, `measures.md` (plan 02).
- `docs/gungnir-capabilities.md` §5 and §8, `ARCHITECTURE.md` §7 and §8.
- `docs/performance-budgets.md` for measure targets already drafted.
- Capability-based planning practice: capability statements, measures, maturity
  levels, DOTMLPF-P style considerations where a capability is not purely material.

## Deliverables and target location

All under `docs/mission/capabilities/`:

| File | Content |
|---|---|
| `README.md` | Index and how capabilities relate to the solution capability reference |
| `capability-taxonomy.md` | The hierarchy: Sense, Understand, Decide, Act (recommend and authorize), Sustain, Secure, Integrate; two levels below each |
| `capability-statements.md` | One entry per leaf capability: statement, rationale, measures, maturity target per increment, threads it serves, crates that provide it, DOTMLPF-P notes |
| `capability-to-thread-matrix.md` | Capabilities against the ten mission threads, with coverage marks |
| `capability-to-crate-matrix.md` | Capabilities against crates and binaries, with a status mark from the crate map in `docs/gungnir-capabilities.md` §8 |
| `capability-roadmap.md` | Maturity per capability per increment, aligned with the strategic roadmap view (plan 03, St-Rm) |
| `measures-catalogue.md` | Every measure used, its definition, unit, method of measurement, and source |

## Capability taxonomy (initial)

| Area | Leaf capabilities (initial) |
|---|---|
| Sense | Ingest sensor observations from radar, electro-optical and infrared, acoustic, radio-frequency detection, and cooperative sources; validate and quarantine untrusted input; manage sensor modes and coverage; maintain time discipline across sources |
| Understand | Maintain a single track picture across sensors; estimate kinematics with stated uncertainty; classify and identify friend, foe, neutral, unknown; keep global identity across sessions; fuse point-cloud and terrain context; compute line-of-sight and coverage |
| Decide | Score threat and time to impact against defended assets; recommend resource-to-track assignments; generate alternatives and what-if; explain recommendations; enforce geofence and authority policy |
| Act (recommend and authorize) | Present recommendations for human decision; record every decision; never execute without a recorded decision; hand off to effector systems through the API |
| Sustain | Journal every event; replay and review sessions; report after action; operate disconnected and reconcile on reconnection; monitor health and alert |
| Secure | Authenticate operators and callers; authorize by role; audit; protect data in transit and at rest; assure the software supply chain |
| Integrate | Expose a versioned API to peer systems; speak ASTERIX, STANAG 4676, Arrow, JSON; run as a desktop, an on-prem node, or a cloud node |

## Capability statement template

```
CAP-xx.yy  <Name>
Statement:   The system shall enable <role> to <do what> in <conditions>.
Rationale:   Which threads need it and why it matters (from plan 02).
Measures:    MOE/MOP identifiers from measures-catalogue.md with target values.
Maturity:    Increment 1: none | partial | full ... Increment 4: ...
Threads:     MT-01, MT-03 ...
Provided by: gungnir-... (status from the crate map)
Considerations: doctrine, organization, training, materiel, leadership, personnel,
             facilities, policy items that the capability depends on beyond software.
```

## Method

1. **Derive.** From each mission thread step, extract the capability the step
   needs; merge duplicates; place in the taxonomy.
2. **State.** Write each statement in the template; assign measures from plan 02's
   catalogue, adding measures where none exist.
3. **Trace.** Fill both matrices; every leaf capability must serve at least one
   thread and be provided by at least one crate or be marked as a gap for plan 05.
4. **Target.** Set maturity per increment with the owner, consistent with the
   increments in `docs/gungnir-capabilities.md` §7.
5. **Register.** Enter every capability in the UAF element registry (plan 03) so
   St-Tx and St-Rm are generated from the same list.
6. **Review and publish.**

## Roles

- Owner: capability statements, measures, maturity targets.
- Writing agent: derivation, matrices, registry entries.

## Dependencies

Plan 02. Feeds plans 03, 05, 06, and 10.

## Effort and sequencing

5 to 8 agent-assisted days; 2 weeks elapsed after plan 02.

## Acceptance criteria

- Every leaf capability has a statement, at least one measure with a target, a
  maturity target per increment, at least one thread, and either a providing crate
  or a gap reference.
- Both matrices are complete and agree with the crate map.
- The capability list and the UAF registry are the same list.

## Risks

- Capabilities written as features rather than outcomes; mitigate with the template's
  role-and-conditions form and a review pass for verbs.
- Measure inflation; mitigate by requiring a measurement method for each.

## Open questions

- Whether effector handoff belongs in scope for the first release or remains an
  integration capability only.
- The maturity vocabulary: three levels as above, or a numeric scale.

# Mission capabilities

Deliverables of [plan 04](../../plans/04-mission-capabilities.md): the operational
view of capability, derived from the mission threads in `../mission-threads.md`.
`../../gungnir-capabilities.md` remains the solution view of what each crate does;
the capability-to-crate matrix here is the bridge between the two.

Status: first draft 2026-09-04; statements, measures, and maturity targets await
the owner's confirmation (open items in each file).

| File | Content |
|---|---|
| [`capability-taxonomy.md`](capability-taxonomy.md) | Seven areas (Sense, Understand, Decide, Act, Sustain, Secure, Integrate) and the 56 leaf capabilities under them |
| [`capability-statements.md`](capability-statements.md) | One entry per leaf: statement, rationale, measures, maturity per increment, threads served, crates that provide it, considerations beyond software |
| [`capability-to-thread-matrix.md`](capability-to-thread-matrix.md) | Leaf capabilities against the ten mission threads |
| [`capability-to-crate-matrix.md`](capability-to-crate-matrix.md) | Leaf capabilities against crates and binaries, with the crate's status |
| [`capability-roadmap.md`](capability-roadmap.md) | Maturity per capability per engineering increment, and what each increment delivers in mission terms |
| [`measures-catalogue.md`](measures-catalogue.md) | Every measure the statements use: definition, unit, method, source |

The capability list is also the `capabilities` section of the UAF element registry,
`../../architecture/uaf/model/elements.yaml`, so plan 03's strategic views and this
folder name the same things.

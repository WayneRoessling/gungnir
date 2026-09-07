# UX designs by role

Deliverables of [plan 06](../plans/06-ux-design-by-role.md): the operator experience
for each of the eight roles, designed for high-stakes, time-pressured decision
support with a human on the loop. Every screen traces to a role's tasks from the
mission analysis (`../mission/`) and to a capability from plan 04
(`../mission/capabilities/`); every element names the model field that feeds it.

Status: first draft 2026-09-04. The design principles below are proposed for the
owner's confirmation; the personas await validation with the subject-matter
reviewers from plan 02; the usability test plan has had one heuristic walkthrough
by the drafting agent and no session with participants yet.

| File | Content |
|---|---|
| [`personas.md`](personas.md) | One persona per role (P-01 to P-08): goals, environment, tempo, tools, frustrations, decisions owned |
| [`task-analysis/`](task-analysis/README.md) | One hierarchical task analysis per role: tasks from the threads, information needs, decisions, error modes, time pressure |
| [`information-architecture.md`](information-architecture.md) | The panel catalogue (PN-01 to PN-20), navigation, the always-visible strip, layouts per role extending `gungnir_workflow::WorkspaceLayout` |
| [`wireframes/`](wireframes/README.md) | Salt wireframes per panel and per role layout (`.puml`), each annotated with the `gungnir-model` field behind every element |
| [`interaction-design.md`](interaction-design.md) | The critical flows (FL-01 to FL-08) as sequence and state diagrams: alert lifecycle, plan decision, replay, sensor mode change, disconnected fallback and reconnection, assistant question and answer, identity declaration, weapons control status |
| [`design-system.md`](design-system.md) | Tokens from `gungnir-ui::theme`, typography, colour semantics for status and classification, track glyph iconography, density, the dark operations-room mode, and the theme changes proposed |
| [`accessibility.md`](accessibility.md) | Contrast, colour-independent encoding, keyboard operation, screen-reader targets within what egui offers |
| [`usability-test-plan.md`](usability-test-plan.md) | Scenario tasks per role from the vignettes, the measures (MOP-37), participants, protocol, reporting, the record of the first walkthrough, and (§7) round 1 re-planned onto the built panels |
| [`reports/round-1-2026-09-06.md`](reports/round-1-2026-09-06.md) | Round 1's report on the §7 template, pre-filled with the sessions (the owner for all roles, 2026-09-06), the seed hash, the ten task rows, and group C; every measured field blank until a session fills it |
| [`usability-round-1-session.md`](usability-round-1-session.md) | Round 1 on the built panels (D-28, 2026-09-06): what the owner supplies, which of the sixteen tasks can run today and which cannot and why, the session baseline and seed, the moderator script, the task cards, the scoring sheet, and the report template |
| [`ux-to-code-map.md`](ux-to-code-map.md) | Every wireframe element to the panel file and model field; every flow to the crate; the engineering items filed in the gap register |

## Design principles (proposed, for the owner to confirm)

1. **The picture is one thing.** Every panel reads the same `gungnir-model` views
   from `AppState`; no panel keeps its own copy (UI standards §2). Visible in every
   wireframe's data annotations.
2. **Recommendation is not action.** Every recommendation shows its policy verdict
   and waits for a recorded decision; the accept control is never the default and
   never where a habitual click lands. Visible in PN-05, PN-06, PN-07.
3. **Stale and low-confidence data look different from fresh data, always.** Muted
   glyphs, a staleness age, and a confidence figure on every track everywhere it
   appears. Visible in PN-02, PN-03, PN-04.
4. **Health is reported, never inferred.** The three health flags, the backend
   state, and the clock source are on the status strip of every layout. Visible in
   PN-01.
5. **Explain on demand.** Every score, assignment, verdict, and alert has a "why"
   that opens the evidence, the factors, or the rule that produced it. Visible in
   PN-04, PN-05, PN-08.
6. **Disconnected is a normal state, not an error.** The strip shows which backend
   is live, what is queued, and which delegation is in force; reconnection shows
   its conflicts for a decision, never silently. Visible in PN-01, PN-18.
7. **AI assistance is labelled, sourced, and never has authority.** The assistant
   panel is visibly separate, every answer carries its provenance, and no control in
   it changes state. Visible in PN-19.

Two principles were added by the task analyses and are proposed with the others:

8. **One decision per gesture.** A decision control acts on exactly one plan or
   alert; bulk actions exist only for acknowledgement, never for engagement.
9. **Time remaining is always visible on anything that expires.** Plans, alerts,
   and delegations show the time left, not the time created.

## How the designs map to code

- Panels are functions in `gungnir-ui/src/panels/` taking `&mut egui::Ui` and
  references into `AppState`; the design adds panels with the same shape
  (`ux-to-code-map.md`).
- Layouts are `gungnir_workflow::WorkspaceLayout::for_role`; the information
  architecture proposes the panel list per role including the three adopted roles
  (GAP-068).
- Tokens are constants in `gungnir-ui/src/theme.rs`; the design system proposes
  additions, not replacements, so the viewport's materials keep reading the same
  palette.
- Flows are the semantics of `gungnir-command`, `gungnir-policy`,
  `gungnir-workflow`, `gungnir-replay`, `gungnir-sensor-management`, and
  `gungnir-remote`; the interaction design adds no state machine the crates do
  not have, and names the gap where one is missing.

## Conventions

- Identifiers: P-nn personas, T-role-n.n tasks, PN-nn panels, WF-nn wireframes,
  FL-nn flows, DS-nn design tokens groups, US-nn usability tasks.
- Wireframes are PlantUML Salt (`@startsalt`), rendered by
  `../architecture/uaf/render.sh` when pointed at this folder or by any PlantUML
  renderer; Mermaid flows are inline and render on GitLab and GitHub.
- The roles are the eight of `../mission/roles-and-stakeholders.md`; the three
  adopted on 2026-09-04 are designed in full, with their panels flagged as needing
  GAP-068 before they can be wired.

## Open items

- Confirm the nine principles (owner).
- Validate the personas with the plan 02 subject-matter reviewers.
- D-17 (docking and multi-window) was resolved on 2026-09-04: adopted where
  appropriate; `information-architecture.md` §1 states the rules and GAP-075
  implements them.
- Run the usability test with participants (US-01 to US-16).

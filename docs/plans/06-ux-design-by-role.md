# Plan 06: UX designs by role

## Purpose

Design the operator experience for each role so that the panels in `gungnir-ui`,
the layouts in `gungnir-workflow`, and the viewport become a workflow designed for
high-stakes, time-pressured decision support with a human on the loop. Every screen
traces to a role's tasks from the mission analysis and to a capability from plan 04.

## Scope

In scope: personas and task analyses per role, information architecture, wireframes
per role and panel, interaction design for the critical flows (alert lifecycle, plan
approval, replay, sensor tasking, the AI assistant's surfaces), a design system on
top of the existing theme, accessibility, and a usability test plan.

Out of scope: visual branding beyond the design system tokens, and implementing the
screens (that is engineering work scheduled from the gap register).

## Inputs

- `docs/mission/roles-and-stakeholders.md`, `mission-threads.md`, `vignettes.md`
  (plan 02).
- `gungnir-workflow` (workspace layouts, alert lifecycle), `gungnir-ui` (panels,
  theme), `gungnir-viewport3d` (2D fallback and the planned scene), `gungnir-command`
  (approval workflow).
- `docs/rust-ui-architecture-coding-standards.md` and
  `docs/rust-ui-tech-stack-summary.md` for what egui and three-d can do well.
- Human-factors practice for command-and-control displays: alert design, colour
  semantics, workload, mode awareness, explainability of automation.

## Roles covered

The five in `gungnir-security` (operator, supervisor, analyst, sensor manager,
administrator) plus the three proposed by plan 02 (intelligence analyst, planner,
commander). If plan 02 does not adopt the three, their designs are marked deferred.

## Deliverables and target location

All under `docs/ux/`:

| Path | Content |
|---|---|
| `README.md` | Index, design principles, how designs map to code |
| `personas.md` | One persona per role: goals, environment, tempo, tools, frustrations, decisions they own |
| `task-analysis/` | One file per role: hierarchical task analysis for the role's threads, information needs per task, decisions, error modes, time pressure |
| `information-architecture.md` | Panels, navigation, layouts per role (extending `gungnir-workflow::WorkspaceLayout`), what is always visible, what is on demand |
| `wireframes/` | Salt wireframes per role and panel (`.puml`) with rendered SVG; annotated with the data each element reads from `gungnir-model` |
| `interaction-design.md` | The critical flows as sequence and state diagrams: alert new to closed; plan proposed to decided (accept, override, reject) with the policy verdict shown; replay scrubbing; sensor mode change; disconnected fallback and reconnection; AI assistant question and answer with provenance |
| `design-system.md` | Tokens (from `gungnir-ui::theme`), typography, colour semantics for status and classification, iconography for track glyphs, density rules, dark operations-room mode |
| `accessibility.md` | Contrast, colour-independent encoding, keyboard operation, screen-reader targets where feasible in egui |
| `usability-test-plan.md` | Scenario-based tasks per role from the vignettes, measures (time to acknowledge, decision latency, error rate, workload rating), participants, protocol, reporting |
| `ux-to-code-map.md` | Each wireframe element to the panel file and model field that feeds it; each flow to the crate that implements it |

## Design principles (to be confirmed)

1. The picture is one thing: every panel reads the same model views; no panel keeps
   its own copy.
2. Recommendation is not action: every recommendation shows its policy verdict and
   waits for a recorded decision; the accept control is never the default.
3. Stale and low-confidence data look different from fresh data, always.
4. Health is reported, never inferred: degraded subsystems are visible on every
   layout.
5. Explain on demand: every score, assignment, and alert can show why.
6. Disconnected is a normal state, not an error: the layout shows which backend is
   live and what is queued.
7. AI assistance is labelled, sourced, and never has authority.

## Method

1. **Research.** Personas and task analyses from the mission threads; validate with
   the subject-matter reviewers from plan 02; contextual sessions if an operator is
   available.
2. **Structure.** Information architecture per role; reconcile with
   `gungnir-workflow::WorkspaceLayout` and propose changes to it.
3. **Wireframe.** Salt wireframes, one panel at a time, annotated with data sources.
4. **Flows.** Interaction design for the critical flows, checked against
   `gungnir-command`, `gungnir-policy`, and `gungnir-resilience` semantics.
5. **System.** Design tokens and semantics; propose the theme changes.
6. **Test.** Run the usability plan on the wireframes (paper or clickable), then on
   the implemented panels as they land; report.
7. **Publish and map** to code; file engineering items in the gap register.

## Roles

- Owner: principles, persona validation, priority of roles.
- UX agent: task analyses, wireframes, flows, design system, test plan.
- Subject-matter reviewers and test participants (human-owned): validation sessions.

## Dependencies

Plan 02 to start; plan 04 for capability traceability; feeds plan 08 (where the
assistant appears) and the engineering backlog.

## Effort and sequencing

12 to 20 agent-assisted days; 4 weeks elapsed, starting after plan 02.

## Acceptance criteria

- Every role has a persona, a task analysis, a layout, and wireframes for every
  panel in its layout.
- Every wireframe element names its data source; every critical flow names the
  crate that implements it.
- The design principles are confirmed and each is visible in at least one
  wireframe.
- The usability test plan has run at least once on wireframes with recorded
  results.

## Risks

- Designing beyond what egui renders well; mitigate by reviewing wireframes against
  the UI standards and the tech-stack summary.
- Role set changes late; mitigate by structuring files per role so additions are
  additive.

## Open questions

- Availability of operators for validation sessions.
- Whether multi-window or docking is required (open in
  `docs/rust-ui-tech-stack-summary.md` §5).

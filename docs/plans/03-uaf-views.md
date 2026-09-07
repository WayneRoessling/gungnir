# Plan 03: UAF views

## Purpose

Produce a Unified Architecture Framework (UAF) 1.2 description of Gungnir and its
mission context, as text and diagrams in the repository, so that the operational
need, the solution, the people, the standards, and the deployment can be read from
one consistent model and referenced by the TOGAF documentation (plan 10) and the gap
analysis (plan 05).

## Scope

In scope: a prioritized subset of the UAF grid covering every domain the project
needs, an element registry that all views share, traceability views between domains,
and a rendering pipeline. The subset is chosen for a small team and a scaffold-stage
product; views are added as the product grows.

Out of scope: a full grid of every view kind, simulation views, and tool-specific
model exchange (XMI), unless a customer requires them.

## Inputs

- `docs/mission/` (plan 02) for the strategic and operational content.
- `docs/mission/capabilities/` (plan 04) for the capability taxonomy.
- `ARCHITECTURE.md`, the crate manifests, `docs/gungnir-capabilities.md`, and
  `docs/gungnir-api-v1.md` for the resources, services, and information content.
- `gungnir-security` for the personnel and security content; `ARCHITECTURE.md` §8
  for actual resources; `docs/performance-budgets.md` for parameters.
- The UAF 1.2 specification (Object Management Group) for view definitions.

## Deliverables and target location

All under `docs/architecture/uaf/`:

| Path | Content |
|---|---|
| `README.md` | The grid: which views exist, which are planned, how to render |
| `model/elements.yaml` | The element registry: capabilities, operational performers and activities, services, resources, personnel types, standards, projects, information elements, each with an identifier, name, description, and owner |
| `model/relationships.yaml` | Typed relationships between elements (exhibits, performs, realizes, implements, conforms to, uses) |
| `summary-and-overview.md` | Sm-Ov: purpose, scope, context, findings |
| `strategic/` | St-Tx capability taxonomy, St-Sr capability structure, St-Cn capability connectivity, St-Rm capability roadmap (mapped to increments) |
| `operational/` | Op-Tx operational performer taxonomy, Op-Sr operational structure, Op-Cn operational connectivity, Op-Pr operational processes (one per mission thread), Op-St operational states (alert and plan lifecycles), Op-Is interaction scenarios (sequence diagrams per vignette), Op-If operational information |
| `services/` | Sv-Tx service taxonomy, Sv-Sr service structure, Sv-Cn service connectivity, Sv-Pr service processes, Sv-If service interfaces (the tracking, intercept, and API contracts) |
| `personnel/` | Pr-Tx role taxonomy, Pr-Sr organizational structure, Pr-Cn role connectivity, Pr-Rm competence and training roadmap |
| `resources/` | Rs-Tx resource taxonomy, Rs-Sr resource structure (crates, binaries, containers), Rs-Cn resource connectivity (dependency graph, channels, API), Rs-Pr resource functions (tick loops), Rs-If resource interfaces, Rs-St resource states (health) |
| `security/` | Sc-Tx security taxonomy, Sc-Sr security structure, Sc-Cn security connectivity (trust boundaries per profile), Sc-Pr security processes (authentication, authorization, audit) |
| `information/` | If-Tx information taxonomy, If-Sr information structure (the canonical model and events), If-Cn information exchange |
| `standards/` | Sd-Tx standards taxonomy, Sd-Rm standards roadmap (ASTERIX, STANAG 4676, Arrow, JSON, the API contract) |
| `projects/` | Pj-Rm project roadmap (increments and the plan set) |
| `actual-resources/` | Ar-Sr actual resource structure per deployment profile, Ar-Cn actual connectivity |
| `parameters/` | Pm-Me measurements (performance budgets and measures of effectiveness) |
| `traceability/` | Capability to operational activity, operational activity to service, service to resource, resource to standard, role to activity, each as a matrix |
| `render.ps1` and `render.sh` | Render every PlantUML and Mermaid source to SVG under `rendered/` |

Diagram sources live beside the Markdown as `.puml` and `.mmd` files and are
embedded by reference; the Markdown carries the narrative and the tables.

## Deliverable outline: each view file

1. View identifier and UAF definition in one sentence.
2. Purpose for this project and who reads it.
3. The diagram or table, embedded from its source file.
4. Elements used, by registry identifier, with one line each.
5. Notes: assumptions, what is planned versus real (matching `README.md` status).
6. Traceability: the views this one derives from and the views that derive from it.

## Method

1. **Select.** Fix the view subset above with the owner; record the rest of the
   grid as "not produced, reason".
2. **Registry first.** Populate `model/elements.yaml` from the capability taxonomy
   (plan 04), the mission threads and roles (plan 02), the crate manifests, the
   service traits, the API contract, and the standards catalogue in
   `gungnir-interop`. Identifiers are stable and are the only names views use.
3. **Derive resource, service, and information views from the code.** An agent
   generates Rs-Sr, Rs-Cn, Sv-If, and If-Sr from `Cargo.toml` edges, trait
   signatures, and `gungnir-model`, so they cannot drift from the truth
   (`CLAUDE.md`, "When a doc and the code disagree").
4. **Author operational and strategic views from the mission set.** Op-Pr and
   Op-Is per thread and vignette; St-Tx and St-Rm from the capability taxonomy and
   increments.
5. **Author personnel, security, standards, projects, actual-resource, and
   parameter views** from `gungnir-security`, `ARCHITECTURE.md` §8 and §9,
   `gungnir-interop`, the plan set, and the performance budgets.
6. **Trace.** Build the traceability matrices from `model/relationships.yaml`; a
   script checks that every capability is exhibited by some performer, every
   operational activity is realized by a service or a human, and every service is
   implemented by a resource, and reports what is not.
7. **Render and review.** Render all diagrams; owner review of the operational and
   strategic content; engineering review of the resource content.
8. **Maintain.** Add a check to `ci.yml` that renders the diagrams and runs the
   registry consistency script.

## Conventions

- Names: UAF view codes as directory and file prefixes (`Op-Pr-MT-01.puml`).
- Stereotypes: PlantUML stereotypes carry the UAF element kinds
  (`<<Capability>>`, `<<OperationalPerformer>>`, `<<Resource>>`, `<<Service>>`).
- Colours follow `gungnir-ui::theme` for status where a view shows status.
- Every element in a diagram exists in the registry; the consistency script fails
  otherwise.

## Roles

- Owner: view selection, operational and strategic content sign-off.
- Architecture agent: registry, generated views, traceability, rendering pipeline.
- Engineering reviewer: resource, service, and information views against the code.

## Dependencies

Plans 02 and 04 for content. The code-derived views can be produced immediately.

## Effort and sequencing

15 to 25 agent-assisted days; 4 weeks elapsed after plan 04. Precedes plan 10.

## Acceptance criteria

- Every view in the selected subset exists, renders, and cites its registry
  elements.
- The consistency script passes: no orphan capabilities, activities, or services.
- The resource and service views match the crate manifests and trait signatures
  exactly.
- Each mission thread has an Op-Pr view and each vignette an Op-Is view.
- The TOGAF documents (plan 10) reference these views rather than redrawing them.

## Risks

- UAF vocabulary can crowd out clarity; mitigate with the one-sentence definition
  at the top of each view and plain-language notes.
- Code-derived views go stale; mitigate with generation scripts and the CI check.
- The registry becomes a second architecture; mitigate by keeping it a list of
  names and relationships, never behaviour.

## Open questions

- Whether a customer or accreditor will require a specific UAF tool export later.
- Whether to include simulation and personnel-availability views in a later
  revision.

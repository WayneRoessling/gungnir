# Sm-Ov Summary and overview

**UAF definition.** The summary and overview view records the purpose, scope,
context, tools, findings, and assumptions of an architecture description.

**Purpose here.** The front page of the Gungnir UAF description: what it describes,
for whom, how it was produced, what it found, and what it does not cover. Read
first by anyone opening `docs/architecture/uaf/`.

Status: first draft, 2026-09-04 (plan 03).

## 1. Identification

| Item | Value |
|---|---|
| Architecture | Gungnir command-and-control desktop and services layer, one node in a system of systems spanning disconnected desktops, on-prem service nodes, and cloud service nodes |
| Framework | UAF 1.2 (OMG), the view subset in `README.md`; TOGAF ADM documentation (plan 10) references these views |
| Owner | Roessling Digital; product owner Wayne Roessling |
| Version | 1.0 draft, 2026-09-04; registry `model/elements.yaml` schema version 2 |
| Classification | Unclassified; open sources only; fictional geography (the Vell estuary) in every scenario |
| Tooling | Markdown, PlantUML, Mermaid in the repository; the element registry in YAML; `tools/build_uaf.py` generates the code-derived views and checks the registry; `render.sh` and `render.ps1` render diagrams |

## 2. Scope

The description covers the mission context (why the system exists, from
`../../mission/`), the capabilities it must provide (`../../mission/capabilities/`),
the operational performers and activities that use it, the services it exposes, the
software resources that implement them (the 50 crates, generated from the manifests),
the people, the security posture, the information model, the standards, the projects
that will deliver it, the actual deployments, and the measures that judge it.

Out of scope, per the plan: the full UAF grid, simulation views, tool-specific model
exchange. Views not produced are listed in `README.md` with the reason.

## 3. Context

Gungnir sits between sensors and effectors in a defended sector. It ingests
observations, maintains a track picture, identifies and assesses threats, recommends
engagements, presents them to a human who holds authority, records the decision, and
hands off. It never executes an engagement itself (CAP-4.3). It runs as a
disconnected desktop, an on-prem node, or a cloud node from one crate set
(`../../../ARCHITECTURE.md` §8). The lead mission is integrated air defense and
counter-UAS; maritime and land are supporting domains; intelligence and planning are
cross-cutting functions (`../../mission/mission-analysis.md`).

## 4. How the description was produced

1. The registry was populated from plan 04's capability taxonomy, plan 02's roles
   and threads, the service traits, the crate manifests, the interop catalogue, and
   the plan set. Identifiers are stable.
2. The resource, service, and information views were generated from the code by
   `tools/build_uaf.py`, so they cannot drift from `Cargo.toml`, the trait
   signatures, or `gungnir-model`.
3. The operational process and interaction views were generated from the ten
   mission threads and ten vignettes, with each step mapped to a registry activity.
4. The strategic, personnel, security, standards, projects, actual-resource, and
   parameter views were authored from the mission set, `gungnir-security`,
   `ARCHITECTURE.md` §8 and §9, the interop catalogue, the plan set, and the
   performance budgets.
5. The traceability matrices were built from `model/relationships.yaml`, and the
   consistency check confirms no orphan capability, activity, or service.

## 5. Findings

- **The operational need is broader than the implemented solution.** All 56
  capabilities have a home in the design; 4 are fully implemented, 32 partly, 20 not
  at all (`../../mission/gap-analysis/coverage-matrix.md`). The views mark planned
  elements and relationships as such rather than drawing them as real.
- **One vocabulary holds across every domain.** The same `gungnir-model` types
  appear in the information views, the service interfaces, the API, the journal,
  and the thread step tables, which is why the operational and resource views
  trace to each other without a translation layer.
- **The human-in-the-loop boundary is explicit in every domain.** Operationally
  (OA-07 Decide is performed by roles only), in services (SV-15 never returns
  Approved on its own, SV-16 records every decision), in security (Sc-Pr), and in
  the process views (every S+H step crosses into the human lane).
- **Connectivity is the largest single dependency.** SV-23's transport (GAP-041)
  gates peer exchange, handoff, mid-session failover, and caller authentication;
  Ar-Cn shows the connected profiles as intended, not fielded.
- **Eight roles, five in code.** The three roles adopted under D-05 are in the
  personnel views as adopted and in the registry as not yet in code (GAP-068).

## 6. Assumptions

- The mission analysis and capability set are first drafts awaiting subject-matter
  review; the views inherit that status.
- Deployment profiles and platforms are as `ARCHITECTURE.md` §8 and §9 state on
  2026-09-04; the container host and cloud provider are not yet chosen.
- Standards marked planned will be adopted as their gaps close (D-09, GAP-041,
  GAP-060, GAP-064, GAP-069).

## 7. Reading order

`README.md` (the grid) → `strategic/St-Tx.md` → `operational/Op-Tx.md` and one
`Op-Pr` → `services/Sv-Tx.md` → `resources/Rs-Sr.md` → `actual-resources/Ar-Sr.md`
→ the traceability matrices.

## Traceability

- Derives from: every source named in section 4.
- Feeds: plan 10 (the Architecture Vision and every phase B to D document reference
  these views), plan 06 (task analyses from Op-Pr and Pr-Cn), plan 01 (product
  story from St-Tx and St-Rm).

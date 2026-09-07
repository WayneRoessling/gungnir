# Rs-Tx Resource taxonomy

**UAF definition.** The resource taxonomy view presents the types of resource
(software, systems, physical assets) as a hierarchy.

**Purpose here.** The kinds of resource in Gungnir and how the generated resource
list (Rs-Sr) is organized: crates by layer, the two binaries, the container image,
and the external resources the crates use. Read first before Rs-Sr and Rs-Cn.

Status: first draft, 2026-09-04.

## The taxonomy

| Kind | Resources |
|---|---|
| Software crates, tracking core | RS-core, RS-coord, RS-filters, RS-association, RS-track, RS-rfs, RS-fusion-async, RS-track-fusion, RS-allocation, RS-scenario, RS-metrics, RS-oracle, RS-testkit, RS-fuzz (not a workspace member) |
| Software crates, foundation model | RS-model |
| Software crates, service facades | RS-tracking-service, RS-intercept-service |
| Software crates, productization | RS-eventing, RS-store, RS-config, RS-mission, RS-time, RS-ingest, RS-sensor-management, RS-interop, RS-identity, RS-identification, RS-geo, RS-analytics, RS-policy, RS-command, RS-assessment, RS-decision, RS-modelops, RS-security, RS-api, RS-observability, RS-resilience, RS-collab, RS-workflow, RS-replay, RS-reporting |
| Software crates, data ecosystem | RS-data, RS-data-fusion, RS-render |
| Software crates, deployment | RS-remote, RS-node (binary) |
| Software crates, user interface | RS-viewport3d, RS-ui, RS-app (binary) |
| Packaged artefacts | the `gungnir-node` container image (`deploy/node/Dockerfile`, SD-12); the `gungnir-app` desktop installer (release governance, planned) |
| External software the crates depend on | the pinned version set in `../../../../ARCHITECTURE.md` §9 (nalgebra, tokio, serde, arrow, eframe and egui, three-d, wgpu, and the rest) |
| Physical and hosting | the actual resources AR-01 to AR-07 (Ar-Sr) |

## Elements used

- RS-* (50 entries generated from the manifests), AR-01 to AR-07, SD-12.

## Notes

- Crate names are the identifiers' suffixes (RS-core is `gungnir-core`), so adding
  a crate adds an identifier without renumbering.
- The external dependency set is not in the registry as resources; it is pinned in
  one place (`Cargo.toml`) and listed in §9. Adding to it is a §2.9 sign-off.

## Traceability

- Derives from: `../../../../ARCHITECTURE.md` §1 to §8; the crate manifests.
- Feeds: Rs-Sr, Rs-Cn, Rs-If, Rs-Pr, Rs-St, Ar-Sr.

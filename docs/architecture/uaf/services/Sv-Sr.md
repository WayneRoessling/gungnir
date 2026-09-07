# Sv-Sr Service structure

**UAF definition.** The service structure view shows how services are composed and
which services depend on other services.

**Purpose here.** How the services stack: which services a facade composes, which
services every other service relies on, and which crates implement each. Read by
engineers placing new work and by plan 10.

Status: first draft, 2026-09-04.

## Composition

| Service | Composed of or relies on | Implemented by (resources) |
|---|---|---|
| SV-01 Tracking | the tracking-core traits (filters, association, lifecycle, RFS, track fusion) behind `gungnir-fusion-async`; SV-05 for time; produces IE-02 | RS-tracking-service, RS-fusion-async, RS-filters, RS-association, RS-track, RS-rfs, RS-track-fusion, RS-core, RS-coord, RS-model |
| SV-02 Intercept planning | SV-17 for rewards; the allocator; SV-01's tracks and IE-03 resources | RS-intercept-service, RS-allocation, RS-core, RS-coord, RS-model |
| SV-08 Ingest gateway | SV-20 (source authentication), SV-05, SV-10 (codecs); feeds SV-01 | RS-ingest |
| SV-15 Policy | SV-13 (geofences); reads IE-03 readiness; chained engines | RS-policy |
| SV-16 Approval workflow | SV-15 verdicts; SV-21 (authorization); SV-22 (audit, pending wiring) | RS-command |
| SV-18 Decision support | SV-17, SV-15 | RS-decision |
| SV-23 API v1 | SV-01, SV-02, SV-03, SV-20, SV-21; serves IE-22 to IE-25 | RS-api, RS-node, RS-model |
| SV-30 Remote backends | SV-23 as a client; SV-25 for the outbox; implements SV-01 and SV-02 traits remotely | RS-remote |
| SV-25 Store-and-forward and reconciliation | SV-04 (journals to merge); SV-26 (arbitration rule) | RS-resilience |
| SV-26 Shared picture and arbitration | SV-03 envelopes; SV-16 decisions; SV-21 roles | RS-collab |
| SV-27 Operator workflow | SV-21 (role layouts); SV-24 (alerts) | RS-workflow |
| SV-28 Replay | SV-04, SV-03, SV-05 | RS-replay |
| SV-29 Reporting | SV-04, SV-03, SV-33 | RS-reporting |
| SV-07 Mission lifecycle | SV-06, SV-04 | RS-mission |
| SV-19 Model registry | SV-06 | RS-modelops |
| SV-09 Sensor registry | SV-06 (sensor definitions); RS-coord | RS-sensor-management |
| SV-14 Analytics | SV-13, SV-31 (terrain) | RS-analytics |
| SV-13 Geospatial | RS-coord, SV-31 | RS-geo |
| SV-32 Point-cloud fusion | SV-31; RS-render's compute device | RS-data-fusion, RS-render |
| SV-03, SV-04, SV-05, SV-06, SV-10, SV-11, SV-12, SV-17, SV-20, SV-21, SV-22, SV-24, SV-31, SV-33, SV-34 | the foundation model only | one crate each (service-to-resource matrix) |

## Elements used

- SV-01 to SV-34; the RS-* resources named.

## Notes

- "Relies on" follows the crate dependency edges (Rs-Cn); a service never relies on
  one whose crate is above it in the layer order.
- The desktop and node compose the same services; only SV-30 and SV-23 differ
  between profiles (Ar-Sr).

## Traceability

- Derives from: Sv-Tx; Rs-Cn (the edges); `../model/relationships.yaml` implements.
- Feeds: Sv-Cn, Sv-Pr, the service-to-resource matrix.

# Ar-Sr Actual resource structure

**UAF definition.** The actual resources structure view shows the actual (fielded
or planned) instances of resources and how they are composed for a deployment.

**Purpose here.** What runs where in each of the three deployment profiles: which
crates on which host, which journal is authoritative, which resources are absent.
Read by a customer's deployment staff and by plan 10 (technology architecture).

Status: first draft, 2026-09-04. The disconnected profile is exercisable end to
end today; the connected profiles are scaffolded but not connectable (GAP-041).

## Profiles

| Profile | Hosts | Runs | System of record | Users |
|---|---|---|---|---|
| Disconnected desktop | AR-01 | RS-app with every crate embedded: tracking core, facades, productization, data ecosystem, UI; local adapters for attached sensors | the desktop's local journal (RS-store) | one operator; other roles by logging in locally |
| On-prem connected | AR-02 (one or more) plus AR-01 per post | RS-node on AR-02 with the facades and productization crates, no UI or rendering; RS-app on AR-01 with RS-remote pointing at the node | the node | several operators, supervisors, analysts sharing one mission |
| Cloud connected | AR-03 plus AR-01 per post, AR-05 peers | the same RS-node image on AR-03; RS-app as above over the WAN | the node | as on-prem plus remote clients and peer systems |

## Crate placement (from `../../../../ARCHITECTURE.md` §8.3)

| Crate group | AR-01 desktop | AR-02 and AR-03 node |
|---|---|---|
| Tracking core, service facades | embedded | yes |
| RS-model, RS-eventing, RS-config, RS-mission | yes | yes |
| RS-store | local journal | authoritative journal |
| RS-time, RS-ingest, RS-sensor-management, RS-interop | for locally attached sensors | for sensors feeding the node |
| RS-identity, RS-identification, RS-assessment, RS-decision, RS-modelops | yes | yes |
| RS-policy, RS-command | local approval | arbiter for shared missions |
| RS-collab | projection side | authoritative side |
| RS-resilience | store-and-forward, reconcile on reconnect | accepts forwarded envelopes, reconciles |
| RS-security | login and local audit | authentication and authorization for every caller; central audit |
| RS-api | optional loopback | the node's only external surface |
| RS-observability | local health panel | health endpoint, watchdogs |
| RS-workflow, RS-replay, RS-reporting | yes | alert state shared; replay and reports against the node's journal |
| RS-data, RS-data-fusion, RS-geo, RS-analytics | yes | RS-geo only if geofences are evaluated server-side |
| RS-render, RS-viewport3d, RS-ui, RS-app | yes | never |

## Hosts

| Actual resource | Platform | Notes |
|---|---|---|
| AR-01 Operator workstation host | Windows 11, discrete NVIDIA GPU | OpenGL context for egui and three-d; separate wgpu compute device; CPU fallback for registration without a GPU |
| AR-02 On-prem service node host | Linux x86_64 container (`deploy/node/Dockerfile`) | journal on local storage; LAN to sensors and desktops |
| AR-03 Cloud service node host | the same image on a cloud host (provider not chosen) | WAN clients; at-rest encryption with keys off-host (GAP-060) |
| AR-04 Sensor feeds | per deployment | adapters per sensor class (GAP-001) |
| AR-05 Peer C2 nodes | per deployment | through the API and interop formats |
| AR-06 Effector systems | per deployment | handoff (GAP-040) |
| AR-07 Build and release infrastructure | GitHub (`WayneRoessling/gungnir`), GitHub Actions, a self-hosted `gpu` runner, GitHub Container Registry (D-10, amended 2026-09-07) | GAP-061 |

## Elements used

- AR-01 to AR-07; the RS-* groups named.

## Notes

- The toolchain both targets build with is pinned in `rust-toolchain.toml` (Rust
  1.98); the version set in `../../../../ARCHITECTURE.md` §9.
- Hosting the cloud node is a deployment decision; nothing in the crates assumes a
  provider.

## Traceability

- Derives from: `../../../../ARCHITECTURE.md` §8.1 to §8.3 and §8.7; `deploy/`;
  Rs-Sr.
- Feeds: Ar-Cn, Sc-Cn, plan 10 technology architecture, `deploy/README.md`.

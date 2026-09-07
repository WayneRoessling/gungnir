# Sc-Tx Security taxonomy

**UAF definition.** The security taxonomy view presents the security controls,
enclaves, and assets of the architecture as a hierarchy.

**Purpose here.** The security assets Gungnir protects, the controls it applies, and
the enclaves it runs in, so that the security views and the accreditation evidence
(OP-27) share one list. Read by the security engineer and plan 10 (security
architecture).

Status: first draft, 2026-09-04.

## Assets

| Asset | Held in | Why it matters |
|---|---|---|
| The mission record (IE-12 journal) | RS-store on AR-01 to AR-03 | System of record for decisions; integrity and, on the cloud host, confidentiality |
| Decision and audit records (IE-18, IE-20) | RS-command, RS-security | Accountability for every engagement |
| Configuration baselines (IE-17) | RS-config | Policy, delegations, and the asset list act through them |
| The live picture (IE-02) and products (IE-28) | every node | Releasability (CAP-6.6) |
| Credentials and keys | D-02 mechanism; key management (GAP-060) | Machine and operator identity |
| The software and model supply chain | AR-07 | A tampered build is a tampered weapon-system input |

## Controls

| Control | Capability | Implemented by | Status |
|---|---|---|---|
| Input validation and quarantine | CAP-6.7, CAP-1.2 | SV-08 | real |
| Source authentication | CAP-1.2 | SV-08 allow-list; SV-20 (planned) | partial |
| Operator and caller authentication | CAP-6.1 | SV-20 | trait only (D-02, GAP-057) |
| Role-based authorization | CAP-6.2 | SV-21 `role_permits` | real, coarse (GAP-058) |
| Recorded human decision before action | CAP-4.3 | SV-15, SV-16 | real by design; wiring GAP-028 |
| Append-only audit | CAP-6.3 | SV-22 | real; wiring GAP-059 |
| Encryption in transit | CAP-6.4 | transport (SD-08) | planned (GAP-060) |
| Encryption at rest, keys off-host | CAP-6.4 | RS-store on AR-03 | planned (GAP-060) |
| Releasability marking and enforcement | CAP-6.6 | views and RS-api | planned (D-06, GAP-062) |
| Supply-chain assurance | CAP-6.5 | `deny.toml`, release workflow, SD-13 | policy real, unexercised (GAP-061) |
| Assistant bounded by data-egress policy and no authority | CAP-4.7, CAP-6.7 | plan 08 | planned (D-14) |

## Enclaves

| Enclave | Contents | Boundary |
|---|---|---|
| Disconnected desktop | AR-01 with embedded services and local journal | The host; no network required |
| Site or sector LAN | AR-01 workstations, AR-02 node, AR-04 sensors | The LAN; sensors and desktops authenticate to the node |
| Cloud | AR-03 node, WAN clients, AR-05 peers | TLS everywhere; at-rest encryption |

## Elements used

- CAP-1.2, CAP-4.3, CAP-4.7, CAP-6.1 to CAP-6.7; SV-08, SV-15, SV-16, SV-20 to
  SV-22; RS-api, RS-config, RS-command, RS-security, RS-store; IE-02, IE-12, IE-17,
  IE-18, IE-20, IE-28; AR-01 to AR-05, AR-07; SD-08, SD-13.

## Notes

- The security posture per profile is `../../../../ARCHITECTURE.md` §8.5; this
  view enumerates it so that each control traces to a capability and a gap.

## Traceability

- Derives from: `../../../../ARCHITECTURE.md` §8.5; `gungnir-security`;
  `../../../release-governance.md`; decisions D-02, D-06, D-14.
- Feeds: Sc-Sr, Sc-Cn, Sc-Pr, plan 10 security architecture, the compliance
  assessment (MOP-40).

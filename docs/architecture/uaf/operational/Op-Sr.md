# Op-Sr Operational structure

**UAF definition.** The operational structure view shows the composition of
operational performers and the resources and information they hold.

**Purpose here.** How the sector is organized: which roles sit in which node, which
node runs which part of the system, and where the authoritative record lives. Read
with the vignettes' setting (`../../../mission/vignettes.md`).

Status: first draft, 2026-09-04.

Diagram: [`Op-Sr.puml`](Op-Sr.puml).

## Structure

| Node | Roles present | System parts | Holds |
|---|---|---|---|
| OP-10 Sector command post | OP-02 Supervisor, OP-01 Operators (air and land), OP-04 Sensor manager, OP-07 Planner, OP-06 Intelligence analyst, OP-08 Commander (when present), OP-03 Analyst, OP-05 Administrator | OP-32 Sector service node; OP-31 workstations connected to it | The authoritative journal for the mission (connected profiles); long-range radar and the acoustic network feed here; links to OP-20 and OP-21 |
| OP-11 Site defense cell (one per defended site) | OP-01 Operator (site), site authority | OP-31 Operator workstation connected to the node, embedded fallback | Local sensors and effectors; a local journal while disconnected |
| OP-12 Port defense cell | OP-01 Operator (maritime), port defense authority | OP-31 Operator workstation as above | Coastal radar, cameras, patrol craft, the boom |
| External | OP-20 to OP-27 | reached through OP-32's interface (OP-34 for effectors) | Their own records |

The C2 system (OP-30) is one logical performer distributed over the nodes: every
workstation embeds the full services layer and switches to the node's services when
connected (`../../../../ARCHITECTURE.md` §8.2); the node is the system of record.

## Elements used

- OP-01 to OP-08, OP-10 to OP-12, OP-20 to OP-27, OP-30 to OP-34.

## Notes

- The number of sites and cells is per deployment; the vignettes use one command
  post, two site cells, and one port cell.
- "Site authority" and "port defense authority" are the OP-01 or OP-02 holder of
  engagement authority at that node, not additional roles.

## Traceability

- Derives from: Op-Tx; `../../../mission/vignettes.md` (the setting);
  `../../../../ARCHITECTURE.md` §8.
- Feeds: Op-Cn, Ar-Sr (the physical hosts behind each node), Sc-Cn (trust
  boundaries follow these nodes).

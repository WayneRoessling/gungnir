# Mission glossary and vocabulary mapping

Status: first draft, 2026-09-04. The engineering glossary is in `../README.md`; this
one covers mission terms and maps this document set's function vocabulary to the
joint and NATO terms a reviewer may expect.

## 1. Function vocabulary and its mappings

| This document set | US joint usage (public doctrine) | NATO usage (public) | System function |
|---|---|---|---|
| Detect | Find (in find, fix, track, target, engage, assess) | Detection | Ingest and track initiation |
| Track | Fix and track | Tracking; the recognized air picture (RAP), recognized maritime picture (RMP) | Tracking service |
| Identify and classify | Identification; combat identification | Identification | Identification engine, evidence |
| Assess | Threat evaluation | Threat evaluation | Assessment (risk scoring) |
| Recommend | Weapon assignment; weapon-target pairing | Weapon assignment | Intercept planner, decision support |
| Decide | Engagement authority exercising rules of engagement under a weapons control status | Engagement authority; rules of engagement | Policy verdict and recorded decision |
| Engage (handoff) | Engage | Engagement | API handoff (to be defined) |
| Assess effects | Assess; battle damage assessment | Assessment | Post-engagement events |

## 2. Terms

| Term | Meaning here |
|---|---|
| Air picture, surface picture, ground picture | The set of tracks with identities and quality for a domain; one picture is shared by every role and panel |
| Area defense, point defense, self-defense | Layers by range and authority: long and medium-range missiles; short-range guns, missiles, interceptor drones, jammers; the fire unit's own immediate defense |
| Battle rhythm | The recurring cycle of handovers, plan changes, reports, and reviews |
| Cooperative identification | Identity supplied by the platform itself: IFF, ADS-B, AIS, blue-force tracking |
| Counter-battery | Fires against artillery that has just fired, cued by acoustic or radar detection |
| Counter-UAS (C-UAS) | Detecting, identifying, and defeating small uncrewed aircraft |
| Decoy | A cheap air vehicle or missile built to be mistaken for a threat and draw interceptors |
| Defended-asset list | The prioritized list of places and things the sector protects |
| Deconfliction | Ensuring an engagement or fires task does not endanger friendly aircraft, craft, positions, or civil traffic; represented as geofences and policy rules |
| Engagement authority | The role permitted to authorize an engagement of a given layer against a given class |
| Fibre-optic FPV | A first-person-view strike drone controlled over an unspooling optical fibre, immune to radio jamming |
| Glide bomb | An unpowered guided bomb released at standoff range from an aircraft |
| GNSS denial | Jamming or spoofing of satellite navigation signals |
| Identification criteria | The evidence and confidence required by policy before a track may be treated as hostile, per class |
| Laydown | The placement of sensors and effectors and the coverage that results |
| Leaker | A threat that passes the area layer and must be engaged by the point layer |
| Loitering munition | A one-way attack drone that waits over an area for a target |
| Mobile fire group | A vehicle-mounted team with machine guns, searchlights, and man-portable missiles that engages low-flying drones cheaply |
| One-way attack UAS | A drone flown to a target and detonated; the propeller class is the raid weapon of the war |
| Pattern of life | Recurring adversary behaviour over time: launch areas, routes, timings |
| Quasi-ballistic | A ballistic trajectory with manoeuvres in flight, complicating interception |
| Recommendation-only | The design rule that the system proposes and records but never executes without a human decision |
| Releasability | Whether a product may be shared with a given partner |
| Rules of engagement | The policy under which force may be used; represented as identification criteria, weapons control status, authorities, and restrictions |
| Sector | The area a command post is responsible for |
| Shoot and move | Artillery practice of firing and relocating within minutes |
| Store-and-forward | Queuing detections and decisions on a disconnected desktop for delivery when the node returns |
| Uncrewed surface vessel (USV) | A remotely or autonomously operated boat, here mainly explosive-laden attack craft |
| Weapons control status | A per-layer state saying whether fire units may engage unidentified tracks, only hostile tracks, or nothing except in self-defense; the joint terms are weapons free, weapons tight, weapons hold |
| Warning obligation | Who must be told, by what means, when a threat is inbound to an asset |

## 3. Identifiers used across the mission set

| Prefix | Meaning | Defined in |
|---|---|---|
| MT-xx | Mission thread | `mission-threads.md` |
| VG-xx | Vignette | `vignettes.md` |
| MOE-xx, MOP-xx | Measures | `measures.md` |
| TT-xx | Test-track scenario (proposed names; plan 07 confirms) | `vignettes.md` |
| CAP-xx.yy | Capability (plan 04) | `capabilities/` |
| GAP-xxx | Gap (plan 05) | `gap-analysis/` |

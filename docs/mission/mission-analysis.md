# Gungnir mission analysis

Status: first draft, 2026-09-04. Subject-matter validation pending; see §11.

## 1. Purpose and method

This document set characterizes the missions Gungnir supports so that every
capability (plan 04), gap (plan 05), architecture view (plan 03), screen (plan 06),
test track (plan 07), and learned model (plans 08 and 09) traces back to something a
person in a command post has to do. It follows a mission-engineering method:

1. **Characterize** the missions and the operational environment.
2. **Define measures** of effectiveness and performance for each mission.
3. **Analyze mission threads** end to end: who does what, with what information, and
   which decisions are held by whom.
4. **Identify** what the system does today, what a human does, and what nothing does
   yet, so plans 04 and 05 can turn that into capabilities and gaps.

The analysis is written at the tactical and low operational level, where a C2
desktop and a service node sit: a protected site, a sector, a port, or a task group,
with a small team of operators and a chain of authority above them. Vocabulary is
function-based (detect, track, identify, assess, decide, engage, assess effects) with
a mapping to joint and NATO terms in `glossary.md`, so that the analysis does not
assume one nation's doctrine.

Everything is unclassified and from open sources. Threat figures are given as ranges
with a confidence mark and a source type; the vehicle catalogue in plan 07 will carry
per-figure sources and is the place to correct them.

## 2. Mission set and priorities

| Priority | Mission | Why this priority |
|---|---|---|
| Lead | **Integrated air defense and counter-UAS** for a defended-asset list: detect, track, identify, prioritize, recommend engagement, record the human decision, hand off, and assess, against one-way attack drones, loitering munitions, small UAS, cruise and ballistic missiles, glide bombs, and crewed aircraft | It is where the intercept planner earns its keep, where the timelines are tightest, and where the Russia-Ukraine war has shown the cost asymmetry between cheap attackers and expensive interceptors that a good C2 can partly redress |
| Supporting | **Maritime domain awareness and port defense** against uncrewed surface vessels and fast craft, with surface picture compilation | Shares sensors, geography, and command posts with coastal air defense; USVs and air raids are coordinated by the adversary |
| Supporting | **Land picture and fires cueing**: convoy and battery tracking from ISR feeds, deconflicted handoff to fires, force protection against small UAS | Uses the same track picture, identity, and policy machinery; the FPV threat makes land force protection a counter-UAS problem |
| Cross-cutting | **Intelligence**: collection management, identification evidence, pattern of life, order of battle, dissemination | Feeds identification and prioritization in every domain |
| Cross-cutting | **Planning and battle management**: defended-asset lists, sensor and effector laydown, rehearsal, after-action review | Sets the conditions the live missions run under and closes the loop afterwards |

## 3. Operational environment (summary)

`operational-environment.md` describes the environment in full. The points that shape
the system most:

- **Low, slow, many, and cheap.** One-way attack drones fly low along terrain and
  rivers, in raids of tens to over a hundred, often at night and mixed with cruise
  missiles and decoys. Detection is a horizon and clutter problem; prioritization is
  a saturation problem; cost per engagement is a sustainment problem.
- **Fast and few.** Cruise missiles arrive at high subsonic speed at low altitude
  with minutes of warning; ballistic and aeroballistic missiles arrive in minutes
  with seconds of terminal warning. These set the latency budgets.
- **Contested spectrum.** GNSS jamming and spoofing, link jamming, and radar
  interference are normal. Sensors degrade, tracks coast, and the picture must say so.
- **A sensor mosaic, not a sensor.** Long- and medium-range radars, short-range
  counter-UAS radars, electro-optical and infrared cameras, acoustic arrays,
  radio-frequency detectors, cooperative sources (AIS, ADS-B, IFF), and human spotter
  networks reporting through mobile applications all contribute, at different rates,
  latencies, and reliabilities.
- **A layered effector mix.** Long- and medium-range surface-to-air missiles,
  short-range air defense guns and missiles, man-portable systems, mobile fire groups
  with machine guns, interceptor drones, electronic attack, and emerging directed
  energy; each with its own envelope, cost, and authority level.
- **Peers and partners.** The command post exchanges tracks and reports with higher
  and adjacent command posts, civil aviation and maritime authorities, and coalition
  systems, over links that may be intermittent.

## 4. Domains

### 4.1 Integrated air defense and counter-UAS (lead)

`air-defense-and-counter-uas.md`. Threat classes from small multirotors to ballistic
missiles; the engagement sequence with representative timelines (roughly ten minutes
for a propeller drone detected at 40 km, under two minutes for a low-altitude cruise
missile detected at 30 km, seconds for terminal ballistic and for FPV); the sensor
and effector mix per layer; the structure of rules of engagement (identification
criteria, weapons control status, engagement authority, self-defense); the human
decision points the system must present and record; and the failure modes the design
must guard against: misidentification and fratricide, saturation, decoys, and
coasting tracks presented as fresh.

### 4.2 Maritime (supporting)

`maritime.md`. Port, anchorage, and coastal defense against uncrewed surface vessels
and fast craft; surface picture compilation from coastal radar, AIS, cameras, patrol
craft, and aircraft; the specific difficulty of small, low, fast, night-time targets
in sea clutter; harbour protection measures and the roles of patrol boats and
helicopters; and coordination with air defense when USVs and drones arrive together.

### 4.3 Land (supporting)

`land.md`. The ground picture from UAS video, ground surveillance radar, acoustic
sensing, and spotter reports; tracking convoys and artillery batteries; cueing fires
with airspace deconfliction and a recorded decision; force protection of the command
post and its sensors against FPV and loitering munitions; and the identity problem of
vehicles that stop, hide, and reappear.

## 5. Cross-cutting functions

### 5.1 Intelligence

`intelligence.md`. Collection management (turning information requirements into
sensor tasking), identification evidence and its fusion (cooperative identification,
flight plans, emitter identification, kinematic profile, visual identification,
operator designation), pattern of life (launch areas, routes, timings), order of
battle maintenance, and dissemination to peers.

### 5.2 Planning and battle management

`planning-and-battle-management.md`. The defended-asset list and its priorities,
sensor and effector laydown and coverage planning, rules-of-engagement configuration,
mission plans and rehearsal by replay, the battle rhythm, and after-action review.

## 6. Mission threads

`mission-threads.md` documents ten end-to-end threads. Each has a trigger, actors and
roles, steps, the information exchanged (typed against `gungnir-model` where the
system carries it), decision points and who holds them, timing and tempo, success and
failure conditions, and the scenario and test-track set that exercises it.

| Id | Thread | Domain | Tempo |
|---|---|---|---|
| MT-01 | One-way attack drone raid against a defended-asset list | Air | Tens of minutes, tens of tracks |
| MT-02 | Mixed salvo of cruise missiles, drones, and decoys | Air | Minutes, mixed speeds |
| MT-03 | Small UAS over a protected site | Air | Seconds to minutes, one to a few tracks |
| MT-04 | Uncrewed surface vessel attack on a port or anchored ship | Maritime | Minutes, a few tracks in clutter |
| MT-05 | Surface picture compilation | Maritime | Continuous, hundreds of tracks |
| MT-06 | Convoy and battery tracking with cueing of fires | Land | Minutes to hours, intermittent tracks |
| MT-07 | Sensor management under electronic attack | Cross-cutting | Minutes, degraded picture |
| MT-08 | Collection management and identification evidence fusion | Intelligence | Continuous |
| MT-09 | Defended-asset planning, laydown, and rehearsal | Planning | Hours to days, offline |
| MT-10 | Disconnected operation and reconnection | Cross-cutting | Minutes to hours |

## 7. Vignettes

`vignettes.md` places one concrete scenario per thread on a fictional coastline (the
Vell estuary, the port of Kalsund, the Ostmark power station, and the Halden airfield)
with forces, a timeline, success criteria, and the mapping to the five engineering
scenarios in `../scenario-crate-narrative.md` and to the test-track scenario names
plan 07 will use.

## 8. Roles

`roles-and-stakeholders.md`. The five roles in `gungnir-security` (operator,
supervisor, analyst, sensor manager, administrator) are kept. Three roles are
proposed and marked as such: intelligence analyst, planner, and commander. Each role
has responsibilities, the decisions it may take, the information it needs, and the
panels it uses in `gungnir-workflow`.

## 9. Measures

`measures.md` defines measures of effectiveness (mission outcomes) and measures of
performance (system behaviour) per thread. Where `../performance-budgets.md` already
drafts a value it is reused; where doctrine or public analysis gives a value it is
cited; otherwise a proposed value is marked as such for the owner to confirm.

## 10. Traceability

| From | To | Where |
|---|---|---|
| Mission threads MT-xx | Capabilities CAP-xx.yy | Plan 04 fills `capabilities/capability-to-thread-matrix.md`; threads here list the capability areas they need |
| Vignettes VG-xx | Engineering scenarios 1 to 5 and test-track scenarios TT-xx | `vignettes.md`, each entry |
| Roles | UX personas and layouts | Plan 06 (`../ux/`), starting from `roles-and-stakeholders.md` and `gungnir-workflow::WorkspaceLayout` |
| Threads and vignettes | UAF operational views Op-Pr and Op-Is | Plan 03, one view per thread and per vignette |
| Threads | Verification rows | `../verification-capability-table.md` §2, the end-to-end replay rows, once test tracks exist |

## 11. Assumptions, open questions, and validation record

Assumptions made to produce this draft, each overturnable by the owner or a reviewer:

- **Vocabulary.** Function-based, with joint and NATO mappings in `glossary.md`. No
  single nation's doctrine is assumed.
- **Roles.** The three proposed roles were adopted on 2026-09-04 (D-05); they enter
  `gungnir-security::Role` under GAP-068 and plan 06 designs for all eight.
- **Space and cyber.** Treated as environment factors (GNSS denial, link loss,
  compromised feeds), not as missions.
- **Effector handoff.** The system recommends and records; handoff to an effector
  system is through the API and is the receiving system's responsibility. This
  matches the recommendation-only boundary in `../gungnir-capabilities.md` §5.4.
- **Geography.** Vignettes use a fictional coastline so that no real site's defence
  is described.

Questions carried forward from the plan, resolved with the owner on 2026-09-04
(`gap-analysis/decisions-needed.md`):

- Default display vocabulary: NATO and joint terms from `glossary.md`, with a
  per-deployment override table (D-12; GAP-070).
- The three proposed roles: adopted (D-05; GAP-068).
- Releasability marking: modelled in increment 3, enforced at the API in increment 4
  (D-06; GAP-062).
- Fires (MT-06): in the first release (D-07; GAP-036 in increment 3).
- External parties (warning channels, civil authorities, peers, effectors): generic
  configurable endpoints now, agreements per deployment (D-08).
- Cooperative identity: AIS and ADS-B now, IFF deferred; ASTERIX and STANAG 4676
  from public specifications with synthetic corpora (D-09).
- Delegation: point-layer engagements of confirmed-hostile small UAS may be
  pre-delegated per configuration; a disconnected desktop keeps the delegations in
  force at disconnection, with expiry (D-15).
- Assistant data egress: live picture in the cloud profile, allow-listed derived text
  on-prem, local model only when disconnected (D-14).

Validation record:

| Date | Reviewer | Scope | Outcome |
|---|---|---|---|
| 2026-09-04 | Drafting agent | All files | First draft; internal consistency and link check only |
| pending | Subject-matter reviewer (air defense) | `air-defense-and-counter-uas.md`, MT-01 to MT-03, VG-01 to VG-03 | |
| pending | Subject-matter reviewer (maritime) | `maritime.md`, MT-04, MT-05, VG-04, VG-05 | |
| pending | Subject-matter reviewer (land and fires) | `land.md`, MT-06, VG-06 | |
| 2026-09-04 | Owner | Roles and scoping decisions | D-01 to D-15 resolved (`gap-analysis/decisions-needed.md`); measure targets set (D-16) |

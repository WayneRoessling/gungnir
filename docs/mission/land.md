# Land (supporting domain)

Status: first draft, 2026-09-04. Open sources only; ranges with confidence marks.

## 1. Mission statement

Maintain a ground picture of adversary vehicles, convoys, artillery batteries, and
launch sites from ISR feeds; keep identity across intermittent observations; cue
fires with a recorded decision and airspace deconfliction; and protect the command
post, its sensors, and friendly vehicles against small UAS and loitering munitions.

## 2. Picture sources

| Source | Contribution | Character |
|---|---|---|
| ISR UAS video (tactical and medium-altitude) | Vehicles, batteries, launch sites, movement | Narrow field of view; hours of coverage; identity is per sortie unless correlated |
| Ground surveillance radar | Moving vehicles at kilometres to tens of km | Good for convoys on roads; loses stopped vehicles |
| Acoustic sensing | Artillery firing, vehicle movement | Coarse position; useful for counter-battery cueing |
| Spotter and unit reports | Sightings, contact reports | Minutes of latency; coarse; high reach |
| Signals and emitter reports | Radars, jammers, command posts | Position quality varies; identity is strong |
| Peer and higher command feeds | Order of battle, tracks | Format and latency vary |

## 3. Threats and targets

| Class | Behaviour relevant to tracking | Fires relevance |
|---|---|---|
| Main battle tanks and infantry fighting vehicles | Move in groups, stop, hide under cover, reappear | Priority targets when massing |
| Artillery and rocket artillery | Shoot and move within minutes; camouflage between missions | Counter-battery: the timeline is minutes |
| Mobile air-defense and electronic-warfare systems | Move, emit, stop; high value | Removing them opens the air picture; suppression of enemy air defense |
| Logistics trucks and convoys | Road-bound, predictable, many | Interdiction; convoys are the tracking scenario in `../scenario-crate-narrative.md` (Scenario 3) |
| Launch sites for missiles and drones | Pattern of life; brief activity | Pre-emptive cueing; the intelligence thread |
| Small UAS and loitering munitions against friendly forces | Approach at low level; FPV at very short range | Force protection; the counter-UAS thread applied to the land force |

## 4. Threads in this domain

### 4.1 Convoy and battery tracking with cueing of fires (MT-06)

| Step | Function | System role | Human role |
|---|---|---|---|
| 1 | Detect | Ingest ISR observations, radar plots, acoustic and spotter reports through the gateway | Task ISR |
| 2 | Track | Maintain vehicle and group tracks across sensors with different rates and latencies (the Scenario 3 out-of-sequence problem); coast through stops and cover; register sensors against each other to remove bias | Watch for identity swaps |
| 3 | Identify | Vehicle class from video and emitter evidence; global identity across sorties (`gungnir-identity`) | Confirm class where required |
| 4 | Assess | Priority against the fires plan; time sensitivity (a battery that has just fired will move) | Set priorities |
| 5 | Recommend | Propose a fires task with target location, time sensitivity, and deconfliction against friendly positions, airspace, and no-fire areas | Review |
| 6 | Decide | Present and record; the fires authority is distinct from the air-defense authority | Fires authority decides |
| 7 | Hand off | Pass the target to the fires system through the API with provenance | Fires unit executes |
| 8 | Assess | Battle damage assessment from the next observation | Confirm |

### 4.2 Force protection against small UAS

The counter-UAS sequence from `air-defense-and-counter-uas.md` applied at the command
post and around friendly vehicles: RF detection and jamming where the drone is
link-controlled, acoustic and visual warning, interceptor drones and small arms at
very short range, and the command post's own displacement plan. Fibre-optic FPVs
defeat jamming, so detection and warning matter more than electronic attack.

## 5. Identity across observations

Land targets are seen intermittently: a convoy is a track on radar, a set of vehicles
in video, a contact report, and then nothing for an hour. The design's answer is the
separation between session-local tracks (`gungnir-track`) and global identity
(`gungnir-identity`): the tracker owns the kinematics while the observations last,
and the identity resolver keeps the entity alive across gaps, sorties, and sessions,
with a lineage the analyst can inspect. Cross-session correlation by kinematics and
class is not yet implemented (`ARCHITECTURE.md` §10).

## 6. Deconfliction

Cueing fires and defending the air share the same airspace. Every fires task passes
the same geofence and policy checks as an intercept: no-fire areas, friendly
positions, airspace coordination measures, and the trajectories of interceptors and
friendly aircraft. The policy chain in `gungnir-policy` is the single place these
rules live, for both domains.

## 7. Human decision points

| Decision | Held by | System obligation |
|---|---|---|
| Confirm vehicle class or identity | Operator or intelligence analyst | Evidence and lineage shown |
| Prioritize targets | Fires authority or commander | Priorities visible; re-assessment on change |
| Authorize a fires task | Fires authority | Verdict, deconfliction, rationale, record |
| Displace the command post under UAS threat | Commander | Warning time and the disconnected plan (MT-10) |

## 8. What the system does today, and what it does not

| Function | Today | Not yet |
|---|---|---|
| Multi-sensor, multi-rate tracking with bias registration | Scenario 3 and its verification rows | The pipeline and track fusion implementation |
| Global identity | In-memory resolver with merge lineage | Cross-session correlation |
| Fires task as a plan | The plan and approval machinery are domain-neutral | A fires-specific plan type and handoff endpoint |
| Battle damage assessment | Event schema | Observation-to-effect linkage |

## 9. Sources and confidence

| Area | Source type | Confidence | Verify by |
|---|---|---|---|
| Land ISR sources and counter-battery timelines | Public fires and ISR doctrine; published analyses of the war | M | Subject-matter review |
| Vehicle behaviours | Open-source reporting and imagery analyses | M | Plan 07 catalogue |
| FPV force-protection practice | Public reporting on front-line counter-UAS measures | M | Subject-matter review |

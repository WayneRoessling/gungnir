# Maritime (supporting domain)

Status: first draft, 2026-09-04. Open sources only; ranges with confidence marks.

## 1. Mission statement

Maintain a surface picture of a port, anchorage, and coastal approach; detect and
identify uncrewed surface vessels and fast craft in time to defend ships at anchor,
port infrastructure, and bridges; recommend a defensive response within policy;
record the decision; and coordinate with the air-defense cell when surface and air
threats arrive together.

## 2. Threats

| Class | Indicative characteristics | Tactics observed | Warning time |
|---|---|---|---|
| Uncrewed surface vessel | About 40 knots (M); hundreds of km range (M); very low profile; explosive payload; some variants carry air-defense missiles or launch UAS (M) | Night approach from open sea in groups of several; final run at speed; targets ships at anchor, port facilities, bridges | Minutes from a coastal-radar detection at 10 to 15 km (about 8 minutes at 40 knots from 10 km); longer with maritime patrol aircraft or peer reporting |
| Fast craft | 30 to 50 knots (M); crewed | Raids, insertion, reconnaissance | Similar to USVs; identification is the harder problem |
| Uncrewed underwater vehicles | Not tracked by surface sensors | Reported in development (L) | Out of scope for the surface picture; noted for completeness |
| Mines and drifting objects | Stationary or drifting | Laid by USV or aircraft | Detection by patrol; picture must carry static hazards |

## 3. Surface picture compilation

| Source | Contribution | Quality and rate |
|---|---|---|
| Coastal surveillance radar | Primary detection of craft beyond visual range | Seconds per update; sea-clutter limited against low craft; sea-state dependent |
| AIS | Cooperative identity and position of compliant shipping | Reliable for compliant traffic; absent or spoofed for threats; identity source, not detection |
| Electro-optical and infrared cameras | Identification and tracking under cue at kilometres | Weather-limited; the main night identification tool against USVs by wake and thermal signature |
| Patrol boats and helicopters | Detection and identification at the outer approaches; the effector of first resort | Voice and data-link reports at minutes of latency |
| Port and harbour cameras and sensors | Inner harbour picture, booms and barriers | High reliability, short range |
| Peer maritime and naval command posts | Tracks and warnings from further out | Format and latency vary; `gungnir-interop` |
| Air picture | Aircraft and UAS over the sea; USV-launched drones | From the air-defense cell through the shared node |

The compilation problem is the maritime clutter scenario in
`../scenario-crate-narrative.md` (Scenario 2): high false-alarm rates, dropouts behind
land masks, and the need for gating and association that tolerate clutter without
dropping the one real track.

## 4. Engagement sequence

| Step | Function | System role | Human role |
|---|---|---|---|
| 1 | Detect | Fuse radar, camera, patrol, and peer reports; suppress clutter; maintain tracks through dropouts | Watch the approaches; task cameras |
| 2 | Track | Estimate course and speed; predict closest point of approach to protected ships and infrastructure | |
| 3 | Identify | Fuse AIS, visual identification, behaviour (speed, heading toward a protected asset, night, no AIS) into an identity with confidence | Declare where policy requires |
| 4 | Assess | Time to closest approach against the protected list; group behaviour | Adjust priorities |
| 5 | Recommend | Assign patrol craft, helicopter, shore effector, or barrier closure; check geofences (shipping lanes, own craft) | Review |
| 6 | Decide | Present and record | Port defense authority decides |
| 7 | Engage (handoff) | Hand to the patrol craft or shore effector; warn ships | Craft executes |
| 8 | Assess | Confirm neutralized, lost, or continuing | Confirm |

## 5. Coordination with air defense

- USVs and drone raids arrive together; the same command post and node serve both,
  so the picture is one picture. The supervisor sees both queues.
- Some USV variants carry air-defense missiles against helicopters, and some launch
  UAS. A USV track therefore raises the risk to the helicopter the planner would
  otherwise recommend; the assessment must see both domains.
- Shore-based guns engaging surface craft need the same geofence and deconfliction
  checks as air-defense guns (own craft, shipping lanes, the shore).

## 6. Human decision points

| Decision | Held by | System obligation |
|---|---|---|
| Declare a craft hostile | Port defense authority or supervisor | Evidence with confidence; behaviour criteria shown |
| Dispatch a patrol craft or helicopter | Port defense authority | Plan with time to intercept and the craft's readiness |
| Close a barrier or boom; warn and move ships | Port authority | Predicted closest approach and time |
| Authorize a shore effector | Engagement authority | Verdict, geofences, rationale, record |

## 7. Failure modes

| Failure | Design response |
|---|---|
| Real USV lost in clutter | Scenario 2 verification rows; clutter-tolerant association; camera cue on any low-confidence radar track heading in |
| Fishing vessel or own craft engaged | AIS and visual identification weighted; human declaration; geofences for lanes and own craft |
| Picture stale after link loss to the node | Local fallback and journaling (MT-10) |
| Helicopter sent against a USV carrying air-defense missiles | Cross-domain assessment; class-specific risk to the effector |

## 8. What the system does today, and what it does not

| Function | Today | Not yet |
|---|---|---|
| Clutter-tolerant tracking | Scenario 2 defined; verification rows specified | Implementation of association and lifecycle |
| AIS ingestion | Adapter boundary and codec catalogue | AIS decoder |
| Closest point of approach | Not in the assessment baseline | Extend `gungnir-assessment` with course-based approach prediction |
| Barriers and static hazards | Geofences | Hazard layer in `gungnir-geo` |

## 9. Sources and confidence

| Area | Source type | Confidence | Verify by |
|---|---|---|---|
| USV characteristics and tactics | Public reporting on Black Sea operations 2022 to 2026; public manufacturer claims | M | Plan 07 catalogue |
| Surface picture sources | Public maritime domain awareness doctrine and port security practice | M | Subject-matter review |
| Timelines | Computed from public speed and detection-range figures | M | Sensor-model specification |

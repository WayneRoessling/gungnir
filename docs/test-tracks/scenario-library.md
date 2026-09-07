# Scenario library

Status: rendered by `tools/build_catalogue.py` from `scenarios.yaml` (version 2026-09-04) on 2026-09-04. One or more scenarios per mission vignette, composed from the class profiles and sensor models on the fictional Vell estuary; each names its classes, counts, timing, sensors, events, and the expected outcome the validation and the measures check. `sample` gives the reduced composition committed under `../../testdata/tracks/samples/`; full-size sets are generated on demand.

| Scenario | Vignette | Thread | Entities (full) | Duration (full) | Sensor sets | Sample |
|---|---|---|---|---|---|---|
| [TT-01](#tt-01) Night raid of propeller drones with mixed routes and decoys | VG-01 | MT-01 | 45 | 4200 s | sector, port | 480 s, scale 0.25, seed 1701 |
| [TT-02](#tt-02) Cruise-missile salvo with decoys during the raid | VG-02 | MT-02 | 20 | 1500 s | sector, airfield | 420 s, scale 0.5, seed 1702 |
| [TT-03](#tt-03) Multirotor and fibre-optic FPV at a site | VG-03 | MT-03 | 2 | 1200 s | airfield | 1200 s, scale 1.0, seed 1703 |
| [TT-04](#tt-04) USV group at night against the anchorage | VG-04 | MT-04 | 9 | 1200 s | port, sector | 420 s, scale 1.0, seed 1704 |
| [TT-05](#tt-05) Dense compliant surface traffic with anomalies | VG-05 | MT-05 | 139 | 3600 s | port | 420 s, scale 0.1, seed 1705 |
| [TT-06](#tt-06) Battery and convoy with ISR gaps and a biased sensor | VG-06 | MT-06 | 15 | 5400 s | land, sector | 900 s, scale 0.5, seed 1706 |
| [TT-07](#tt-07) Raid under GNSS denial with a radar loss | VG-07 | MT-07 | 31 | 3000 s | sector, port | 1500 s, scale 0.3, seed 1707 |
| [TT-08](#tt-08) Mixed friendly, civil, and hostile air traffic with cooperative sources | VG-08 | MT-08 | 11 | 5400 s | sector, airfield | 900 s, scale 1.0, seed 1708 |
| [TT-09](#tt-09) TT-01 replayed under two laydowns | VG-09 | MT-09 | 45 | 4200 s | sector, port | 480 s, scale 0.25, seed 1701 |
| [TT-10](#tt-10) TT-01 with a mid-raid link loss at the port cell | VG-10 | MT-10 | 45 | 4200 s | sector, port | 480 s, scale 0.25, seed 1701 |

## TT-01 Night raid of propeller drones with mixed routes and decoys

Vignette VG-01, thread MT-01, start 01:38:00, duration 4200 s.

Forty-five propeller one-way attack drones in three streams, two along the Vell below the ridge and one from the sea over Kalsund, six decoys with reflectors among them, arriving at two to four per minute over an hour; the medium-range system at HAF is held for missiles.

| Group | Class | Platform | Side | Count | Spawn window (s) | Route | Notes |
|---|---|---|---|---|---|---|---|
| stream-south | One-way attack UAS, propeller | shahed-136 | red | 18 | 0 to 3000 | vell_valley |  |
| stream-north | One-way attack UAS, propeller | shahed-136 | red | 15 | 200 to 3200 | vell_valley_north |  |
| stream-sea | One-way attack UAS, propeller | shahed-136 | red | 6 | 900 to 3300 | sea_stream |  |
| decoys | One-way attack UAS, propeller | shahed-136 | red | 6 | 0 to 3300 | vell_valley | decoy=True |

Events: t=0 launch_warning (neighbouring sector reports a raid crossing at 01:10).

Expected: entities: 45; decoys: 6; arrival_rate_per_min: [2, 4]; assets_threatened: ['OPS', 'KAL']; success: no drone reaches OPS; at most two reach KAL infrastructure; every engagement recorded (MOE-01, MOE-05).

## TT-02 Cruise-missile salvo with decoys during the raid

Vignette VG-02, thread MT-02, start 01:50:00, duration 1500 s.

The raid of TT-01 plus four subsonic cruise missiles routed low along the Vell to arrive at OPS and HAF at 02:00, two decoy missiles among them, and a peer launch warning eight minutes before detection; the area layer must find the fast tracks in the drone picture.

| Group | Class | Platform | Side | Count | Spawn window (s) | Route | Notes |
|---|---|---|---|---|---|---|---|
| raid | One-way attack UAS, propeller | shahed-136 | red | 12 | 0 to 900 | vell_valley |  |
| salvo-ops | Cruise missile, subsonic | kalibr | red | 2 | 480 to 520 | vell_valley |  |
| salvo-haf | Cruise missile, subsonic | kh-101 | red | 2 | 500 to 540 | sea_to_haf |  |
| decoy-missiles | Cruise missile, subsonic | kalibr | red | 2 | 490 to 530 | vell_valley_north | decoy=True, no_terminal=True |
| interceptors | Air-defense interceptor missile | pac-3 | blue | 2 | 1080 to 1140 | none | launch=HAF, target_group=salvo-haf |

Events: t=0 launch_warning (peer launch warning 01:52).

Expected: entities: 20; fast_tracks: 6; decoys: 2; success: every missile engaged by the area layer or its target warned with sixty seconds; decoys draw at most one interceptor each; decision path in seconds (MOP-07).

## TT-03 Multirotor and fibre-optic FPV at a site

Vignette VG-03, thread MT-03, start 14:18:00, duration 1200 s.

A consumer multirotor over the HAF apron in daylight, driven off by jamming; twelve minutes later an FPV on a fibre-optic tether approaches the site cell's mast from a tree line at 120 km/h.

| Group | Class | Platform | Side | Count | Spawn window (s) | Route | Notes |
|---|---|---|---|---|---|---|---|
| recce | Small multirotor | mavic-3 | red | 1 | 100 | haf_apron |  |
| fpv | FPV strike quadcopter | fpv-strike-quad | red | 1 | 840 | fpv_run | emitting=False |

Expected: entities: 2; success: the multirotor is driven off without jamming inside the navigation geofence; the FPV is warned in time; both recorded; sensors re-tasked.

## TT-04 USV group at night against the anchorage

Vignette VG-04, thread MT-04, start 03:02:00, duration 1200 s.

Four uncrewed surface vessels from the west at 38 knots, no AIS, in a loose line; two ships at anchor; the boom open for a departing tanker; sea state 4; one USV lost in clutter and reacquired by camera cue.

| Group | Class | Platform | Side | Count | Spawn window (s) | Route | Notes |
|---|---|---|---|---|---|---|---|
| usv-line | Uncrewed surface vessel | magura-v5 | red | 4 | 0 to 120 | usv_approach | spacing_m=600 |
| anchored | Amphibious, auxiliary, and civil traffic | tanker-coastal | civil | 2 | 0 | none | phases=['anchored'], at=KAL_ANCHORAGE |
| departing-tanker | Amphibious, auxiliary, and civil traffic | tanker-coastal | civil | 1 | 0 | coastal_lane_n | reverse=True |
| patrol | Fast craft and patrol boat | gyurza-m | blue | 2 | 360 to 400 | usv_approach | reverse=True |

Events: t=0 sea_state.

Expected: entities: 9; success: no USV reaches a ship or the port; the lost USV reacquired within five minutes; the helicopter not sent.

## TT-05 Dense compliant surface traffic with anomalies

Vignette VG-05, thread MT-05, start 12:00:00, duration 3600 s.

One hundred and forty vessels in the approaches on a weekday afternoon, coastal shipping, ferries, fishing boats, two patrol boats, a survey vessel; one fishing boat with AIS off loitering near the outfall; one spoofed AIS position for a coaster.

| Group | Class | Platform | Side | Count | Spawn window (s) | Route | Notes |
|---|---|---|---|---|---|---|---|
| shipping-n | Amphibious, auxiliary, and civil traffic | tanker-coastal | civil | 60 | 0 to 2400 | coastal_lane_n |  |
| shipping-s | Amphibious, auxiliary, and civil traffic | tanker-coastal | civil | 40 | 0 to 2400 | coastal_lane_s | reverse=True |
| fishing | Amphibious, auxiliary, and civil traffic | fishing-vessel | civil | 35 | 0 to 1800 | coastal_lane_s | phases=['transit', 'loiter'] |
| patrol | Fast craft and patrol boat | gyurza-m | blue | 2 | 0 | coastal_lane_n |  |
| ais-off-loiterer | Amphibious, auxiliary, and civil traffic | fishing-vessel | civil | 1 | 600 | none | phases=['loiter'], at=[-27000, -1500, 0], ais=False |
| spoofed-coaster | Amphibious, auxiliary, and civil traffic | tanker-coastal | civil | 1 | 1200 | coastal_lane_n | ais_spoof_offset_m=[6000, 4000] |

Expected: entities: 139; anomalies: 2; success: every vessel a track with identity or anomaly; both anomalies flagged within two minutes (MOP-26); false tracks under the display threshold (MOP-04).

## TT-06 Battery and convoy with ISR gaps and a biased sensor

Vignette VG-06, thread MT-06, start 22:00:00, duration 5400 s.

A self-propelled battery of four shoot-and-moving from wood lines east of the ridge; a resupply convoy of eight trucks on the valley road at night through a twenty-minute gap under tree cover; an ISR UAS orbiting over the ridge; ISR sortie 1 carries an injected position bias for the registration test.

| Group | Class | Platform | Side | Count | Spawn window (s) | Route | Notes |
|---|---|---|---|---|---|---|---|
| battery | Self-propelled artillery | 2s19-msta-s | red | 4 | 0 | none | at=FAR_BANK_WOODS, spacing_m=200 |
| convoy | Logistics truck | kamaz-truck | red | 8 | 1800 | valley_road | spacing_m=80, cover_gap_s=[2400, 3600] |
| isr-uas | Tactical fixed-wing ISR UAS | orlan-10 | red | 1 | 0 | none | at=EAST_ORBIT, phases=['orbit'] |
| own-ad | Mobile air-defense system | patriot-launcher | blue | 2 | 0 | none | at=[-21000, 39000, 0], phases=['operate'] |

Events: t=1800 fires (acoustic detection of a fire mission at 22:30).

Expected: entities: 15; identity_gaps: 1; bias_sensor: 17; success: the battery engaged inside its shoot-and-move window; the convoy's identity survives the gap (MOE-09); the bias between radar and ISR video estimated and removed.

## TT-07 Raid under GNSS denial with a radar loss

Vignette VG-07, thread MT-07, start 01:30:00, duration 3000 s.

The raid of TT-01 a week later; from the start the sector is under GNSS jamming from across the border, and twenty minutes in a loitering munition strikes the ridge radar, which goes off the air; coverage must be recomputed and the HAF radar re-tasked.

| Group | Class | Platform | Side | Count | Spawn window (s) | Route | Notes |
|---|---|---|---|---|---|---|---|
| stream-south | One-way attack UAS, propeller | shahed-136 | red | 20 | 0 to 2400 | vell_valley |  |
| stream-sea | One-way attack UAS, propeller | shahed-136 | red | 10 | 300 to 2400 | sea_stream |  |
| radar-strike | Loitering munition | lancet-3 | red | 1 | 600 | vell_valley | target=RIDGE_RADAR |

Events: t=0 ea_skew; t=0 ea_dropout; t=1200 sensor_lost (ridge radar struck 01:50); t=1380 sensor_retasked (HAF radar to search pattern upper Vell).

Expected: entities: 31; skewed_sensors: [3, 5, 9]; lost_sensor: 1; success: operators know within a minute what they cannot see (MOE-10, 30 s); no decision on a stale track without acknowledgement; the loss a formal accepted gap.

## TT-08 Mixed friendly, civil, and hostile air traffic with cooperative sources

Vignette VG-08, thread MT-08, start 15:00:00, duration 5400 s.

A busy afternoon at HAF, airliners on the corridor with ADS-B, two friendly fighters returning without a filed plan change but with IFF, a friendly helicopter, an adversary ISR UAS loitering at altitude sixty kilometres east, and a light aircraft with an intermittent transponder.

| Group | Class | Platform | Side | Count | Spawn window (s) | Route | Notes |
|---|---|---|---|---|---|---|---|
| airliners | Civil airliner | airliner-a320 | civil | 6 | 0 to 4800 | civil_corridor | adsb=True |
| fighters | Tactical fixed-wing aircraft | f-16 | blue | 2 | 1800 to 1830 | fighter_return | iff=True |
| helicopter | Rotary wing | mi-8 | blue | 1 | 600 | sea_to_haf | iff=True |
| isr-uas | Tactical fixed-wing ISR UAS | orlan-10 | red | 1 | 0 | none | at=EAST_ORBIT, phases=['orbit'] |
| light-aircraft | Light aircraft | cessna-172 | civil | 1 | 2400 | light_aircraft_route | adsb_intermittent=0.6 |

Expected: entities: 11; hostile: 1; success: no friendly or civil track declared hostile (MOE-02); the ISR UAS declared with recorded evidence (MOP-24); the unknown resolved by tasking.

## TT-09 TT-01 replayed under two laydowns

Vignette VG-09, thread MT-09, start 13:00:00, duration 4200 s, based on TT-01.

The raid of TT-01 replayed under the current laydown (A) and a sea-weighted laydown (B) with the HAF radar moved to the coast and a second gun at Kalsund, to show the rehearsal effect; the same seed so only the laydown differs.

| Group | Class | Platform | Side | Count | Spawn window (s) | Route | Notes |
|---|---|---|---|---|---|---|---|
| stream-south | One-way attack UAS, propeller | shahed-136 | red | 18 | 0 to 3000 | vell_valley |  |
| stream-north | One-way attack UAS, propeller | shahed-136 | red | 15 | 200 to 3200 | vell_valley_north |  |
| stream-sea | One-way attack UAS, propeller | shahed-136 | red | 6 | 900 to 3300 | sea_stream |  |
| decoys | One-way attack UAS, propeller | shahed-136 | red | 6 | 0 to 3300 | vell_valley | decoy=True |

Variants: `laydown-A` (sensors=['sector', 'port']); `laydown-B` (sensors=['sector', 'port'], moves=[{'sensor': 2, 'pos': [-28000, 8000, 30]}]).

Expected: success: laydown B engages the sea stream earlier than A (MOE-12); identical truth in both variants.

## TT-10 TT-01 with a mid-raid link loss at the port cell

Vignette VG-10, thread MT-10, start 01:38:00, duration 4200 s, based on TT-01.

The raid of TT-01 in progress; as the sea stream reaches Kalsund the link between the KAL desktop and the sector node fails for eleven minutes; the port cell continues on local sensors under delegated authority and reconciles on reconnection.

| Group | Class | Platform | Side | Count | Spawn window (s) | Route | Notes |
|---|---|---|---|---|---|---|---|
| stream-south | One-way attack UAS, propeller | shahed-136 | red | 18 | 0 to 3000 | vell_valley |  |
| stream-north | One-way attack UAS, propeller | shahed-136 | red | 15 | 200 to 3200 | vell_valley_north |  |
| stream-sea | One-way attack UAS, propeller | shahed-136 | red | 6 | 900 to 3300 | sea_stream |  |
| decoys | One-way attack UAS, propeller | shahed-136 | red | 6 | 0 to 3300 | vell_valley | decoy=True |

Events: t=1500 link_lost.

Expected: link_outage_s: 660; success: nothing recorded offline is lost (MOE-11); the one conflict visible and resolved; the operator saw the backend state throughout.

## Geography

Named points (ENU metres from the sector command post): SCP [0, 0, 0]; OPS [0, 0, 0]; KAL [-25000, 2000, 0]; KAL_ANCHORAGE [-29000, 1500, 0]; KAL_BOOM [-24000, 2500, 0]; HAF [-20000, 40000, 0]; RIDGE_RADAR [8000, 3000, 450]; RIDGE_GSR [9000, 2000, 420]; VELL_UPPER [40000, -25000, 0]; FAR_BANK_WOODS [35000, -8000, 0]; VALLEY_ROAD_START [60000, -30000, 0]; VALLEY_ROAD_END [30000, -6000, 0]; TREE_LINE_HAF [-18500, 41200, 0]; SEA_FAR [-90000, 20000, 0]; SEA_MID [-45000, 8000, 0]; EAST_FAR [120000, -80000, 0]; EAST_MID [60000, -40000, 0]; EAST_NEAR [20000, -12000, 0]; NORTH_CORRIDOR_S [-40000, 70000, 9000]; NORTH_CORRIDOR_N [-10000, 110000, 9000]; EAST_ORBIT [60000, 5000, 5000].

Routes: `vell_valley` (6 waypoints); `vell_valley_north` (5 waypoints); `sea_stream` (6 waypoints); `sea_to_haf` (4 waypoints); `usv_approach` (4 waypoints); `coastal_lane_n` (4 waypoints); `coastal_lane_s` (4 waypoints); `valley_road` (5 waypoints); `civil_corridor` (2 waypoints); `fighter_return` (3 waypoints); `haf_apron` (3 waypoints); `fpv_run` (3 waypoints); `light_aircraft_route` (4 waypoints).

Sensor sets: `sector` (1 R1 ridge radar, 2 R2 HAF radar, 3 A3 acoustic node upper Vell, 5 A5 acoustic node mid Vell, 9 A9 acoustic node coast, 4 F4 OPS counter-UAS radar, 6 C6 OPS camera); `port` (7 R7 KAL coastal radar, 8 C8 KAL camera, 10 AIS receiver KAL); `airfield` (11 F11 HAF counter-UAS radar, 12 F12 HAF RF detector, 13 C13 HAF camera, 14 ADS-B receiver HAF, 15 IFF interrogator HAF); `land` (16 G16 ridge ground radar, 17 V17 ISR sortie 1, 18 V18 ISR sortie 2, 19 A19 battery-location array).

Rendered from `scenarios.yaml` by `tools/build_catalogue.py`; the vignettes are `../mission/vignettes.md`.

# Vignettes

Status: first draft, 2026-09-04. One concrete scenario per mission thread on a
fictional coastline, so that no real site's defence is described. Each vignette
names its forces, timeline, success criteria, the engineering scenario in
`../scenario-crate-narrative.md` that exercises the tracking core for it, and the
test-track scenario name plan 07 will use (TT-xx, proposed).

## The setting: the Vell estuary

```
                        N
        Halden airfield  ^        Hoge ridge (east, 300 to 600 m)
             [HAF]       |          /\/\/\/\
                         |         /
   open sea      Kalsund port     /   Vell river (from the south-east)
   (west)        [KAL] ~~~~~~~~~~/~~~~~~~~~~~~~~~~~~~
                  |  anchorage  /
                  |            /   Ostmark power station [OPS]  (25 km inland)
                  |           /    Sector command post [SCP]    (at Ostmark)
                 ~~~ estuary ~~~
        adversary launch areas: beyond the eastern border, 250 to 400 km east
```

- **Kalsund (KAL):** port and anchorage on the estuary mouth; port defense cell with
  a desktop connected to the sector node; coastal radar, cameras, two patrol boats,
  one helicopter on call, a boom across the inner harbour.
- **Ostmark power station (OPS):** 25 km inland on the Vell; site defense cell with a
  desktop; short-range counter-UAS radar, cameras, RF detection, one gun system, a
  mobile fire group, jammers.
- **Halden airfield (HAF):** 40 km north; site defense cell; medium-range radar and
  missile system; civil and military traffic; ADS-B and IFF.
- **Sector command post (SCP):** at Ostmark; the service node; supervisor, two
  operators, sensor manager, planner, intelligence analyst; long-range radar on the
  Hoge ridge; an acoustic network of a few hundred nodes along the Vell and the
  coast; links to higher command and the neighbouring sector.
- **Defended-asset list:** OPS (priority 1), HAF (2), KAL port infrastructure (3),
  ships at anchor (4), SCP (5).

## VG-01 Night raid on the power station (MT-01)

**Forces:** 45 propeller one-way attack drones in three streams; two streams follow
the Vell from the south-east below the Hoge ridge, one comes in from the sea over
Kalsund; six are decoys with reflectors. Defenders as above; the medium-range
missile system at HAF is held for missiles.

**Timeline:** 01:10 the neighbouring sector reports a raid crossing; 01:25 acoustic
nodes on the upper Vell begin route tracks; 01:38 the ridge radar detects the first
stream at 48 km; 01:41 to 02:30 drones arrive at 2 to 4 per minute; the sea stream
reaches Kalsund at 02:05; 02:40 last engagement.

**Success:** no drone reaches OPS; at most two reach KAL infrastructure; no
medium-range missile spent on a drone; every engagement has a recorded decision;
the supervisor's queue never exceeds the authority's capacity; decoys identified as
such in at least half the cases.

**Exercised by:** engineering Scenario 4 (dense swarm); TT-01.

## VG-02 Salvo with the raid (MT-02)

**Forces:** VG-01 plus four subsonic cruise missiles launched to arrive at OPS and
HAF at 02:00, routed low along the Vell, with two decoy missiles; a peer launch
warning arrives at 01:52.

**Timeline:** 01:52 warning; 01:58 ridge radar detects two fast tracks at 31 km;
01:59 pre-delegated engagement by HAF's missile system; 02:00 leakers to the OPS
gun; 02:01 impacts or misses.

**Success:** all four missiles engaged by the area layer or their targets warned to
shelter with at least sixty seconds; decoys do not draw more than one area
interceptor each; the system's decision path completes in seconds even with the
drone queue active.

**Exercised by:** engineering Scenarios 1 and 4; TT-02.

## VG-03 Quadcopter over the airfield (MT-03)

**Forces:** a single consumer multirotor at 90 m over HAF's apron at 14:20 in
daylight; then, at 14:32, an FPV on a fibre-optic tether approaching the site cell's
sensor mast at 120 km/h from a tree line 1.5 km away.

**Timeline:** 14:20 RF detection and radar track; 14:21 camera identification;
14:22 site authority decides to jam; 14:24 the multirotor returns toward its
operator, whose position is estimated from the uplink; 14:32 acoustic and visual
warning of the FPV; 14:33 impact or interception by the mobile fire group.

**Success:** the multirotor is driven off without jamming inside the ADS-B and
airfield navigation geofence; the FPV is warned in time for personnel to take cover
and, if the interceptor drone is ready, intercepted; both engagements recorded; the
site's alert state raised and sensors re-tasked.

**Exercised by:** TT-03.

## VG-04 USV attack on the anchorage (MT-04)

**Forces:** four uncrewed surface vessels approaching Kalsund from the west at 03:00
at 38 knots, no AIS, in a loose line; one variant of a class reported to carry
air-defense missiles; two ships at anchor; the boom is open for a departing tanker.

**Timeline:** 03:04 coastal radar detects three low tracks at 12 km in sea state 4;
03:06 cameras confirm wakes; 03:07 port defense authority declares hostile; 03:08
patrol boats dispatched, the tanker held, the boom closed; the helicopter is not
recommended because of the missile-carrying variant; 03:14 first USV engaged by a
patrol boat at 4 km; 03:19 last USV neutralized or beached.

**Success:** no USV reaches a ship or the port; the fourth USV, lost in clutter at
03:05, is reacquired by camera cue before 03:10; the helicopter is not sent.

**Exercised by:** engineering Scenario 2 (maritime clutter); TT-04.

## VG-05 The afternoon surface picture (MT-05)

**Forces:** 140 vessels in the approaches on a weekday afternoon: coastal shipping,
ferries, fishing boats, two patrol boats, a survey vessel; one fishing boat with AIS
off loitering near the outfall; one spoofed AIS position for a coaster.

**Timeline:** continuous 12:00 to 18:00; anomalies flagged at 13:12 and 15:40; a
patrol boat checks the loiterer at 13:40.

**Success:** every vessel is a track with an identity or an anomaly; the two
anomalies are flagged within two minutes and resolved by tasking; the neighbouring
maritime command post receives the picture with under the agreed latency; false
tracks in clutter stay below the display threshold.

**Exercised by:** engineering Scenario 2; TT-05.

## VG-06 Battery on the far bank (MT-06)

**Forces:** an adversary self-propelled artillery battery of four vehicles
shoot-and-moving from wood lines 35 km east of the Hoge ridge; a resupply convoy of
eight trucks on the valley road at night; one tactical ISR UAS orbiting over the
ridge cueing them. Defenders: two ISR UAS sorties, a ground surveillance radar on
the ridge, an acoustic battery-location capability, a rocket-artillery unit, and the
HAF missile system for the ISR UAS.

**Timeline:** 22:00 the ISR UAS is tracked and engaged (MT-03 at sector scale);
22:30 acoustic detection of a fire mission; 22:31 the ridge radar tracks vehicles
moving; 22:34 ISR video confirms the battery; 22:36 fires task recommended with
deconfliction against the outgoing interceptor and a no-fire area; 22:37 fires
authority accepts; 22:39 fires delivered; 22:45 battle damage assessment from the
next ISR pass; 23:10 the convoy is tracked through a 20-minute gap under tree cover
and reacquired with the same identity.

**Success:** the battery is engaged inside its shoot-and-move window; the convoy's
identity survives the gap; sensor bias between radar and ISR video is estimated and
removed; every task has provenance.

**Exercised by:** engineering Scenario 3 (urban convoy with injected bias); TT-06.

## VG-07 Raid under GNSS denial with a radar loss (MT-07)

**Forces:** VG-01 repeated a week later; from 01:30 the sector is under GNSS
jamming from across the border; at 01:50 a loitering munition strikes the ridge
radar, which goes off the air.

**Timeline:** 01:30 clock-source skew rises on GNSS-dependent feeds; 01:32 the
system correlates the skew alerts into one incident; 01:50 radar loss; 01:51 coverage
recomputed and the gap on the upper Vell shown; 01:53 the sensor manager re-tasks the
HAF radar to a search pattern and raises acoustic node reporting rates; 01:55 the
supervisor acknowledges the degraded state; drones continue to arrive.

**Success:** the operators know within a minute what they cannot see; no decision
is taken on a stale track without acknowledgement; the raid outcome degrades
gracefully rather than collapsing; the loss is a formal accepted gap until repaired.

**Exercised by:** engineering Scenarios 3 and 5; TT-07.

## VG-08 Who is that aircraft (MT-08)

**Forces:** a busy afternoon at HAF: civil airliners on the corridor, two friendly
fighters returning without a filed plan change, a friendly helicopter, a medium-
altitude adversary ISR UAS at 5 km altitude loitering 60 km east, and a light
aircraft with an intermittent transponder.

**Timeline:** 15:00 to 16:30; the ISR UAS is declared hostile at 15:12 on emitter
and behaviour evidence; the fighters are identified friendly by IFF despite the
missing plan; the light aircraft stays unknown until visual identification at 15:48.

**Success:** no friendly or civil track is ever declared hostile; the ISR UAS is
declared with recorded evidence and engaged; the unknown light aircraft is never
engaged and is resolved by tasking; the neighbouring sector receives the identities.

**Exercised by:** TT-08; identification rows.

## VG-09 Planning the week's laydown (MT-09)

**Forces:** the planner, commander, sensor manager, and analyst on Monday morning;
a new threat estimate says raids will favour the sea approach; a second gun system
has arrived; the medium-range system at HAF needs a maintenance window.

**Timeline:** 09:00 defended-asset priorities reviewed (KAL infrastructure raised to
2 for the week); 10:00 laydown options computed with coverage over terrain; 11:00
policy updated (weapons control status per layer, delegation for the disconnected
case at KAL); 13:00 rehearsal: TT-01 replayed under the old and new laydowns; 15:00
plan applied and audited; Friday: after-action review of the week's journals.

**Success:** the applied plan is validated and rehearsed; the maintenance window's
coverage gap is accepted formally with a warning obligation; the rehearsal shows the
new laydown engages the sea stream earlier.

**Exercised by:** TT-09 (TT-01 under two laydowns); configuration and replay rows.

## VG-10 The port cell loses the node (MT-10)

**Forces:** VG-01 in progress; at 02:03, as the sea stream reaches Kalsund, the link
between the KAL desktop and the sector node fails for eleven minutes.

**Timeline:** 02:03 link loss detected; 02:04 the KAL desktop falls back to
embedded services with a banner; 02:04 to 02:14 the port cell continues on local
sensors and its pre-delegated authority, recording six decisions locally and queuing
its detections; 02:14 the link returns; 02:15 the queue is forwarded and the journals
reconciled; one decision conflicts with a supervisor hold issued at the node during
the outage; 02:16 the supervisor resolves it under the arbitration rule; 02:17 the
port cell is back on the remote backend.

**Success:** the port cell never stops operating; every offline decision reaches the
node's record; the one conflict is visible and resolved rather than overwritten; the
operator saw the backend state throughout.

**Exercised by:** TT-10 (TT-01 with a mid-raid link loss); resilience and
collaboration rows; the cross-layer reconciliation row.

## Mapping summary

| Vignette | Thread | Engineering scenario | Test-track scenario (proposed) |
|---|---|---|---|
| VG-01 | MT-01 | 4 | TT-01 raid, mixed routes, decoys |
| VG-02 | MT-02 | 1, 4 | TT-02 salvo with decoys during a raid |
| VG-03 | MT-03 | none | TT-03 multirotor and fibre-optic FPV at a site |
| VG-04 | MT-04 | 2 | TT-04 USV group at night |
| VG-05 | MT-05 | 2 | TT-05 dense compliant traffic with anomalies |
| VG-06 | MT-06 | 3 | TT-06 battery and convoy with ISR gaps |
| VG-07 | MT-07 | 3, 5 | TT-07 raid under GNSS denial with a sensor loss |
| VG-08 | MT-08 | 1 | TT-08 mixed friendly, civil, and hostile air traffic |
| VG-09 | MT-09 | none | TT-09 TT-01 replayed under two laydowns |
| VG-10 | MT-10 | 4 | TT-10 TT-01 with a mid-raid link loss |

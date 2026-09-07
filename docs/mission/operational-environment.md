# Operational environment

Status: first draft, 2026-09-04. Open sources only; figures are ranges with
confidence marks (H, M, L) and a source type; plan 07's catalogue is where per-figure
sources will be recorded and corrected.

## 1. The setting

Gungnir's command post protects a defended-asset list in a sector that mixes coast,
estuary, urban area, and open country: a port and anchorage, an airfield, a power
station and substations, a logistics hub, troop concentrations, and the command post
itself with its sensors. The sector is within reach of one-way attack drones, cruise
missiles, ballistic and aeroballistic missiles, glide bombs, small UAS, and uncrewed
surface vessels, and is observed by adversary ISR UAS. Friendly aircraft, helicopters,
civil aviation, coastal shipping, and fishing traffic share the space and the
picture.

## 2. Physical environment

| Factor | Effect on the mission |
|---|---|
| Terrain and rivers | Low-flying drones and cruise missiles route along valleys, rivers, and coastlines to stay below radar horizons and in ground clutter; detection ranges against them are set by the radar horizon and terrain masking, not by radar power. `gungnir-analytics` line-of-sight and coverage answer where the gaps are. |
| Coast and sea state | Sea clutter hides small, low craft; sea state changes detection ranges hour by hour. USVs approach at night from open sea. |
| Urban area | Multipath, screening, and civil traffic; small UAS at rooftop height; acoustic sensing is degraded by noise. |
| Weather | Cloud, fog, and precipitation degrade electro-optical and infrared sensors and some radar bands; wind changes small-UAS behaviour and endurance. |
| Night | Most raids are at night; infrared and acoustic sensing and radar carry the picture; visual identification is limited. |
| Distances | Warning time is distance over speed. A propeller drone detected at 40 km gives about thirteen minutes; a low-altitude cruise missile detected at 30 km gives under two; a short-range ballistic missile gives minutes from launch detection and seconds in the terminal phase. |

## 3. Electromagnetic and information environment

| Factor | Effect on the mission |
|---|---|
| GNSS jamming and spoofing | Own sensors and platforms lose or corrupt position and time; drones with inertial and terrain-following navigation are unaffected. `gungnir-time` must reason about clock sources; positions from GNSS-dependent sources carry lower quality. |
| Link jamming | Controlled UAS lose links and either return, loiter, or continue on a pre-planned route; own sensor feeds and peer links drop and recover. Fibre-optic FPVs are immune to link jamming. |
| Radar interference and deception | Decoys with radar reflectors, chaff-like effects, and interference reduce track quality; the picture must show quality, not just position. |
| Adversary ISR | Fixed-wing ISR UAS observe the sector for hours, cueing missiles and artillery; their detection and the decision to engage them is a mission in itself (MT-03, MT-06). |
| Information | Peer command posts, civil aviation and maritime authorities, and coalition systems exchange tracks and reports with different formats, latencies, and trust; `gungnir-interop` and `gungnir-ingest` are the boundary. |
| Cyber | Feeds and links may be compromised; the ingest gateway quarantines what fails validation, and the design treats every external message as untrusted data. |

## 4. Sensor inventory (typical of a sector)

| Sensor class | Detects | Typical characteristics | Notes for the system |
|---|---|---|---|
| Long-range surveillance radar | Aircraft, missiles at altitude, large UAS | Hundreds of km against high targets; horizon-limited against low fliers | Rotating, seconds per update; feeds via ASTERIX-class formats |
| Medium-range tactical air-defense radar | Aircraft, cruise missiles, medium UAS | Tens of km; higher update rate; often tied to a missile system | May be the only sensor that sees a low cruise missile in time |
| Short-range counter-UAS radar | Small and slow UAS, propeller drones | Single-digit to about twenty km against small targets (M); high update rate; classification by micro-Doppler in some systems | Many, cheap, distributed; the backbone of the drone picture |
| Electro-optical and infrared | Anything in line of sight | Kilometres, weather-limited; identification quality; slew-to-cue | Confirms identity; tracks under cue |
| Acoustic arrays and networks | Propeller drones, helicopters, some missiles | Kilometres per node; networks of hundreds of nodes give route tracks along approach corridors (M) | High-latency, low-precision, high-value early warning; strongly out-of-sequence |
| Radio-frequency detection | Controlled UAS by uplink and video downlink | Kilometres to tens of km depending on emitter | Ineffective against autonomous drones with no link |
| Passive coherent location | Aircraft, larger UAS | Uses broadcast transmitters; no emissions | Emerging; useful under emission control |
| Cooperative sources | Own and civil aircraft (IFF, ADS-B), ships (AIS), own vehicles (blue-force tracking) | Reliable when present; absent for the threat | Identification and deconfliction, not detection |
| Human spotter networks | Drones and missiles by sight and sound, reported through mobile applications | Minutes of latency; coarse position; wide coverage (M) | A real input in the war; treated as a low-quality, high-reach sensor |
| ISR UAS video | Ground vehicles, batteries, launch sites | Hours of coverage; narrow field of view | Land picture; identity across sorties |
| Ground surveillance radar | Moving ground vehicles | Kilometres to tens of km | Convoy tracking |
| Coastal surveillance radar | Surface craft | Tens of km; clutter-limited against small craft | Maritime picture |
| Patrol craft and helicopters | Surface craft, USVs | Visual and radar, reported by voice or data link | Low rate, high value |

## 5. Effector inventory (typical of a sector)

| Layer | Effectors | Envelope and cost, indicative | Authority (typical) |
|---|---|---|---|
| Long range | Surface-to-air missile systems for aircraft, cruise, and ballistic missiles | Tens to over a hundred km; very high cost per shot | Sector or higher engagement authority; strict identification criteria |
| Medium range | Medium-range surface-to-air missiles | Tens of km; high cost | Sector engagement authority |
| Short range | Gun and missile short-range air defense, radar-directed guns | Single-digit km; moderate cost | Delegated to site or fire unit under rules of engagement |
| Very short range | Man-portable missiles, mobile fire groups with machine guns and searchlights, small-arms | Kilometres or less; low cost | Fire-unit self-defense and delegated authority |
| Counter-UAS | Interceptor drones, radio-frequency jammers, spoofers, directed energy (emerging), nets and shotguns at very short range | Hundreds of metres to kilometres; low cost per engagement; jamming has collateral effects on own systems | Site authority; jamming often needs spectrum coordination |
| Maritime | Patrol boats, helicopters, shore-based guns and missiles, booms and barriers | Kilometres; moderate cost | Port authority and naval command |
| Land fires | Artillery, rocket artillery, armed UAS | Tens of km; moderate cost; airspace deconfliction required | Fires authority; separate from air defense |

The intercept planner's job is to recommend the cheapest adequate layer for each
track given readiness, geometry, and policy, and to keep expensive interceptors for
the threats that warrant them.

## 6. Adversary systems and tactics (from open reporting)

Figures are public claims or estimates; the catalogue in plan 07 records sources.

| Class | Examples | Indicative kinematics | Tactics observed |
|---|---|---|---|
| One-way attack UAS, propeller | Shahed-136 and Geran-2 family | About 150 to 190 km/h cruise (M); low altitude, often under 1 km, sometimes very low (M); ranges of over a thousand km claimed (M) | Night raids of tens to over a hundred; routing along rivers; timed with missiles; decoy variants; increasing use of jam-resistant navigation |
| One-way attack UAS, jet | Jet-powered Geran variants | Several hundred km/h (L) | Fewer, faster, shorter warning |
| Loitering munition | Lancet family | About 80 to 110 km/h (M); tens of km range (M) | Loiter over an area, dive on cued targets such as air-defense radars and artillery |
| Small multirotor and FPV | Consumer quadcopters, FPV strike drones, fibre-optic FPVs | 60 to 150 km/h (M); kilometres to about 20 km range (M) | Saturation at the front line; ambush of vehicles and command posts; fibre-optic variants immune to jamming |
| Tactical ISR UAS | Orlan-10, Zala, Supercam class | 100 to 150 km/h (M); up to about 5 km altitude (M); hours of endurance | Loiter to cue missiles and artillery; hard to see, cheap to lose |
| Medium-altitude UAS | Bayraktar TB2, Orion class | 130 to 220 km/h (M) | Standoff surveillance and strike where air defense is thin |
| Cruise missile, subsonic | Kalibr, Kh-101 and Kh-555, Storm Shadow and SCALP, Neptune | About Mach 0.7 to 0.9 (H); low altitude; ranges from hundreds to over two thousand km (M) | Terrain-following routes; mixed with drones; salvos timed to arrive together |
| Cruise missile, supersonic and aeroballistic | Kh-22 and Kh-32, Kinzhal, Zircon class | Mach 3 and above (M); aeroballistic profiles for Kinzhal (M) | Few, fast; used against high-value fixed targets |
| Short-range ballistic missile | Iskander-M, KN-23 class | Quasi-ballistic; hundreds of km (H); minutes of flight; high terminal speed (M) | Salvos with decoys; targets air defense sites, airfields, energy |
| Glide bomb | KAB with UMPK kits (500 to 3,000 kg classes) | Released at altitude tens of km out (M); glide at high subsonic speed (M) | Massed against front-line and near-rear targets; hard to intercept |
| Crewed aircraft and helicopters | Su-25, Su-34, Su-35, Ka-52, Mi-8 | Aircraft class kinematics | Standoff release of glide bombs and missiles; helicopters at low level |
| Uncrewed surface vessel | Magura V5, Sea Baby class | About 40 knots (M); hundreds of km range (M); low profile | Night approach in groups; used against ships at anchor, bridges, and ports; some carry air-defense missiles or launch drones |
| Fast craft | Small patrol and assault boats | 30 to 50 knots (M) | Raiding and insertion |
| Electronic warfare | Krasukha, Leer, Pole-21, trench-level jammers | Not applicable | GNSS denial and spoofing over wide areas; link jamming of UAS |

## 7. Blue-force structure (assumed for the analysis)

| Element | Location in the vignettes | System footprint |
|---|---|---|
| Sector command post | Ostmark, inland from the coast | Service node (on-prem profile), supervisor and operator desktops, planner and intelligence analyst desktops, links to higher command and neighbours |
| Port defense cell | Kalsund harbour | Desktop connected to the node; falls back to embedded when the link drops |
| Site defense cells | Ostmark power station, Halden airfield | Desktop connected to the node, each with local short-range sensors and effectors |
| Mobile fire groups and patrol craft | Sector | Report by voice and data link; no desktop |
| Higher command | Out of sector | Peer C2 over `gungnir-api` |
| Civil authorities | Airport, port, police | Cooperative data (ADS-B, AIS) and voice coordination |

## 8. Sources and confidence

| Area | Source type | Confidence | Verify by |
|---|---|---|---|
| Sensor classes and characteristics | Manufacturer public specifications; national doctrine publications on air defense and counter-UAS techniques (public releases); trade press | M | Plan 07 sensor-model specification |
| Effector layers and authorities | Public joint and NATO doctrine on countering air and missile threats; open reporting on Ukrainian mobile fire groups and interceptor drones | M | Subject-matter review |
| Adversary kinematics | Public manufacturer claims; open-source intelligence trackers of equipment in use; published analyses of the war (2022 to 2026) | M to L per figure | Plan 07 catalogue, per-figure sources |
| Tactics | Public reporting and published analyses | M | Subject-matter review; dated review cadence |
| Electromagnetic environment | Public reporting on GNSS interference and UAS link jamming | M | Subject-matter review |

# Amphibious, auxiliary, and civil traffic (`sea.amphibious-auxiliary`)

Landing ships, tankers, merchants, ferries, and fishing boats, slow and predictable, AIS on for the larger ones; the bulk of the surface picture.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 0 to 13 | 0 | 0 | 0.02 to 0.4 | 0 to 0.5 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| transit | `sea-transit` | 3600 to 36000 | 4 to 9 | 0 |  |
| anchored | `anchored` | 3600 to 86400 | 0 to 0.3 | 0 |  |
| loiter | `orbit` | 600 to 7200 | 1 to 4 | 0 | radius_m=200 to 800 |

## Randomization

- lanes: per-entity from the scenario lanes
- AIS off: per-scenario anomaly list
- spoofed AIS: per-scenario anomaly list

## Signatures and sensors

Radar cross-section large, infrared high, acoustic loud. Seen by: `radar.coastal`, `ais`, `eo-ir` (`../sensor-models.md`).

## Platforms in this class

- [Ropucha class landing ship](../platforms/landing-ship-ropucha.md) (red, confidence high)
- [Coastal tanker and merchant class (civil)](../platforms/tanker-coastal.md) (civil, confidence high)
- [Fishing vessel and small craft (civil)](../platforms/fishing-vessel.md) (civil, confidence high)

## Threads and scenarios

Threads MT-04, MT-05; scenarios TT-04, TT-05 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

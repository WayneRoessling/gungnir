# Surface combatant (`sea.surface-combatant`)

Corvette or frigate on a slow transit or holding station; large return; emitters on; AIS off in operations.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 1 to 16 | 0 | 0 | 0.03 to 0.3 | 0 to 0.5 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| transit | `sea-transit` | 3600 to 36000 | 6 to 13 | 0 |  |
| station | `sea-transit` | 3600 to 36000 | 2 to 6 | 0 |  |

## Randomization

- station box: per-entity

## Signatures and sensors

Radar cross-section large, infrared high, acoustic loud. Seen by: `radar.coastal`, `radar.long`, `eo-ir`, `rf` (`../sensor-models.md`).

## Platforms in this class

- [Karakurt and Buyan-M class corvettes](../platforms/corvette-karakurt.md) (red, confidence high)
- [Admiral Grigorovich class frigate](../platforms/frigate-grigorovich.md) (red, confidence high)

## Threads and scenarios

Threads MT-05; scenarios TT-05 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

# Fast craft and patrol boat (`sea.fast-craft`)

Patrol boats and armoured boats at planing or displacement speed; radar and radio emitters; AIS on when in civil traffic.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 2 to 26 | 0 | 0 | 0.05 to 0.6 | 0 to 1.5 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| patrol | `sea-transit` | 1800 to 14400 | 5 to 15 | 0 |  |
| intercept | `sea-transit` | 300 to 1800 | 15 to 25 | 0 |  |

## Randomization

- patrol line: per-entity
- intercept target: from the scenario

## Signatures and sensors

Radar cross-section medium, infrared medium, acoustic loud. Seen by: `radar.coastal`, `eo-ir`, `ais` (`../sensor-models.md`).

## Platforms in this class

- [Raptor class patrol boat (Project 03160)](../platforms/raptor-patrol-boat.md) (red, confidence high)
- [Gyurza-M class armoured boat](../platforms/gyurza-m.md) (blue, confidence high)

## Threads and scenarios

Threads MT-04, MT-05; scenarios TT-04, TT-05 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

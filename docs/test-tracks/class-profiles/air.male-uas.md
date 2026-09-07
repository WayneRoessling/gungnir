# Medium-altitude long-endurance UAS (`air.male-uas`)

Large drone at medium altitude, long orbits or transits, continuous datalink; behaves like a slow light aircraft to a radar.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 30 to 65 | 0 to 8000 | -20 to 10 | 0.2 to 1.6 | 0 to 2 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| transit | `waypoint-cruise` | 1200 to 7200 | 36 to 55 | 4000 to 6500 |  |
| orbit | `orbit` | 3600 to 36000 | 36 to 50 | 4500 to 7000 | radius_m=3000 to 8000 |

## Randomization

- orbit centre and radius: per-entity

## Signatures and sensors

Radar cross-section medium, infrared medium, acoustic moderate. Seen by: `radar.long`, `radar.medium`, `rf` (`../sensor-models.md`).

## Platforms in this class

- [Bayraktar TB2](../platforms/tb2.md) (blue, confidence high)
- [Orion (Inokhodets)](../platforms/orion-uas.md) (red, confidence medium)

## Threads and scenarios

Threads MT-08; scenarios TT-08 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

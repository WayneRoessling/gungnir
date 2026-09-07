# Tactical fixed-wing ISR UAS (`air.tactical-isr`)

Small fixed-wing drone that transits to an area and orbits for hours, cueing strikes; steady speed, gentle turns, continuous datalink.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 15 to 45 | 0 to 5000 | -20 to 6 | 0.2 to 1.2 | 0 to 2 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| transit | `waypoint-cruise` | 600 to 3600 | 25 to 40 | 1000 to 3000 |  |
| orbit | `orbit` | 1800 to 14400 | 22 to 35 | 1500 to 3500 | radius_m=1500 to 4000 |
| egress | `waypoint-cruise` | 600 to 3600 | 25 to 40 | 1000 to 3000 |  |

## Randomization

- orbit centre: per-entity in the area of interest
- orbit direction: per-entity

## Signatures and sensors

Radar cross-section small, infrared low, acoustic moderate. Seen by: `radar.long`, `radar.medium`, `rf`, `eo-ir` (`../sensor-models.md`).

## Platforms in this class

- [Orlan-10](../platforms/orlan-10.md) (red, confidence high)
- [Leleka-100 / Furia class](../platforms/leleka-100.md) (blue, confidence medium)

## Threads and scenarios

Threads MT-03, MT-06, MT-08; scenarios TT-06, TT-08 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

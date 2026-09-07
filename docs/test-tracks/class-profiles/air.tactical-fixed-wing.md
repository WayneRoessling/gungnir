# Tactical fixed-wing aircraft (`air.tactical-fixed-wing`)

Fast jet on a transit, a stand-off release run, or a low-level attack; large return; friendly examples carry IFF and sometimes ADS-B.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 100 to 650 | 0 to 16000 | -250 to 250 | 1.0 to 9.5 | 0 to 20 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| transit | `waypoint-cruise` | 300 to 3600 | 200 to 300 | 5000 to 11000 |  |
| run-in | `waypoint-cruise` | 60 to 300 | 250 to 320 | 200 to 9000 |  |
| evade | `evade` | 20 to 60 | 250 to 350 | 200 to 9000 |  |
| egress | `waypoint-cruise` | 300 to 1800 | 220 to 320 | 3000 to 11000 |  |

## Randomization

- altitudes and speeds: per-phase
- evade turns: per-entity

## Signatures and sensors

Radar cross-section large, infrared high, acoustic loud. Seen by: `radar.long`, `radar.medium`, `rf`, `adsb`, `iff` (`../sensor-models.md`).

## Platforms in this class

- [Su-25](../platforms/su-25.md) (both, confidence high)
- [Su-34](../platforms/su-34.md) (red, confidence high)
- [F-16](../platforms/f-16.md) (blue, confidence high)

## Threads and scenarios

Threads MT-02, MT-08; scenarios TT-02, TT-08 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

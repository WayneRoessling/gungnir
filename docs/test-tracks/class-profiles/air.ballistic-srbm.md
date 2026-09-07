# Ballistic missile, short range (`air.ballistic-srbm`)

Boost, a high arc to tens of kilometres apogee, and a fast descent, with quasi-ballistic manoeuvre in the terminal phase for some types; the shortest warning of any class.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 500 to 2200 | 0 to 55000 | -2200 to 2200 | 0.5 to 12.0 | 0 to 100 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| arc | `ballistic` | 120 to 420 | 700 to 2100 | 0 to 50000 | apogee_m=30000 to 50000 |

## Randomization

- apogee: per-entity
- terminal jink: per-entity 0 to 2 lateral pulls
- decoys: 0 to 2 released at apogee

## Signatures and sensors

Radar cross-section medium, infrared high, acoustic loud. Seen by: `radar.long`, `radar.medium` (`../sensor-models.md`).

## Platforms in this class

- [9K720 Iskander-M (9M723)](../platforms/iskander-m.md) (red, confidence medium)
- [MGM-140 ATACMS](../platforms/atacms.md) (blue, confidence medium)
- [KN-23 class](../platforms/kn-23.md) (red, confidence low)

## Threads and scenarios

Threads MT-02; scenarios TT-02 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

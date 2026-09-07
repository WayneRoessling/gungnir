# Loitering munition (`air.loitering`)

Launched toward an area, loiters under video control looking for a target, then dashes and dives; datalink emits until the dive.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 20 to 90 | 0 to 5000 | -80 to 10 | 0.3 to 2.5 | 0 to 5 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| transit | `waypoint-cruise` | 300 to 1500 | 25 to 45 | 500 to 3000 |  |
| loiter | `orbit` | 120 to 1200 | 22 to 35 | 800 to 3000 | radius_m=800 to 2500 |
| terminal | `dash-terminal` | 15 to 60 | 60 to 85 | 0 to 3000 |  |

## Randomization

- loiter centre: per-entity within the target area
- loiter duration: per-entity
- dash target: per-entity from the scenario target list

## Signatures and sensors

Radar cross-section very-small, infrared low, acoustic moderate. Seen by: `radar.short`, `radar.medium`, `rf`, `eo-ir`, `acoustic` (`../sensor-models.md`).

## Platforms in this class

- [Lancet-3](../platforms/lancet-3.md) (red, confidence medium)
- [Switchblade 600](../platforms/switchblade-600.md) (blue, confidence high)

## Threads and scenarios

Threads MT-03, MT-06; scenarios TT-03, TT-06 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

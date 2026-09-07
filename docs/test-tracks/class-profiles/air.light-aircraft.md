# Light aircraft (`air.light-aircraft`)

General-aviation aircraft at low altitude and speed with an intermittent transponder; the identification problem that must stay unknown rather than becoming hostile.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 20 to 70 | 0 to 4200 | -6 to 5 | 1.0 to 2.0 | 0 to 2 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| transit | `waypoint-cruise` | 600 to 7200 | 50 to 63 | 800 to 2500 |  |

## Randomization

- route: per-entity
- transponder gaps: per-opportunity from the scenario's intermittency

## Signatures and sensors

Radar cross-section medium, infrared medium, acoustic moderate. Seen by: `radar.long`, `radar.medium`, `adsb` (`../sensor-models.md`).

## Platforms in this class

- [Light aircraft (Cessna 172 class)](../platforms/cessna-172.md) (civil, confidence high)

## Threads and scenarios

Threads MT-08; scenarios TT-08 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

# Submarine, surfaced or snorkelling (`sea.submarine-surfaced`)

Slow surfaced submarine or snorkel mast; small intermittent return; for surface-picture purposes only.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 0 to 7 | 0 | 0 | 0.02 to 0.2 | 0 to 0.3 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| surfaced-transit | `sea-transit` | 1800 to 14400 | 2 to 6 | 0 |  |

## Randomization

- intermittent visibility: per-tick 40 percent

## Signatures and sensors

Radar cross-section small, infrared low, acoustic quiet. Seen by: `radar.coastal`, `eo-ir` (`../sensor-models.md`).

## Platforms in this class

- [Kilo class submarine (surfaced or snorkelling)](../platforms/kilo-surfaced.md) (red, confidence high)

## Threads and scenarios

Threads MT-05; scenarios TT-05 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

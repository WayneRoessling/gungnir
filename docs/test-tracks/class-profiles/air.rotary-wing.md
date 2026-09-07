# Rotary wing (`air.rotary-wing`)

Helicopter at nap-of-the-earth or low transit, hovers and pop-ups; large return with rotor modulation.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 0 to 95 | 0 to 6000 | -15 to 15 | 0.3 to 3.0 | 0 to 5 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| transit | `nap-of-earth` | 300 to 3600 | 50 to 75 | 15 to 100 | weave=0.03 |
| hover | `hover` | 30 to 300 | 0 to 3 | 10 to 60 |  |
| egress | `nap-of-earth` | 300 to 3600 | 50 to 80 | 15 to 100 |  |

## Randomization

- hover point: per-entity
- altitudes: per-phase

## Signatures and sensors

Radar cross-section large, infrared high, acoustic loud. Seen by: `radar.long`, `radar.medium`, `radar.short`, `acoustic`, `eo-ir`, `iff` (`../sensor-models.md`).

## Platforms in this class

- [Ka-52](../platforms/ka-52.md) (red, confidence high)
- [Mi-8 / Mi-17](../platforms/mi-8.md) (both, confidence high)

## Threads and scenarios

Threads MT-04, MT-08; scenarios TT-04, TT-08 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

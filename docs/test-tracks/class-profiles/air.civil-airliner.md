# Civil airliner (`air.civil-airliner`)

Passenger aircraft on a published corridor at cruise altitude, steady speed and heading, transponder and ADS-B on; the traffic identification must never engage.

## Envelope

| Speed (m/s) | Altitude (m) | Climb (m/s) | Lateral acceleration (g) | Acceleration (m/s²) |
|---|---|---|---|---|
| 50 to 270 | 0 to 13000 | -20 to 20 | 1.0 to 2.0 | 0 to 3 |

## Phases of a representative mission

| Phase | Model | Duration (s) | Speed (m/s) | Altitude (m) | Parameters |
|---|---|---|---|---|---|
| transit | `waypoint-cruise` | 600 to 7200 | 220 to 250 | 8000 to 12000 |  |

## Randomization

- entry time: per-entity in the spawn window
- cruise altitude: per-entity in the band

## Signatures and sensors

Radar cross-section large, infrared high, acoustic loud. Seen by: `radar.long`, `radar.medium`, `adsb` (`../sensor-models.md`).

## Platforms in this class

- [Narrow-body airliner (A320 / 737 class)](../platforms/airliner-a320.md) (civil, confidence high)

## Threads and scenarios

Threads MT-08; scenarios TT-08 (`../scenario-library.md`).

Rendered from `../classes.yaml` by `tools/build_catalogue.py`.

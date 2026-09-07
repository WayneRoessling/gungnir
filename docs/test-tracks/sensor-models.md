# Sensor models

Status: rendered by `tools/build_catalogue.py` from `sensors.yaml` (version 2026-09-04) on 2026-09-04. The observation models that turn truth into `DetectionView`s in `tools/gen_tracks.py`. Every value is an engineering assumption for tracker testing, not any real sensor's performance (`sourcing-and-legal.md` §4); scenarios override per instance.

## How a detection is produced

1. Every `update_period_s` (with a per-sensor phase offset) the sensor has an opportunity against every alive, unoccluded entity.
2. The entity's signature class for the sensor's `signature_key` selects the range band; beyond it there is no detection. Cooperative sensors (AIS, ADS-B, IFF, blue-force tracking) see only entities that emit the matching signal; `moving_only` sensors skip stationary entities; `cued` sensors need another sensor to have detected the entity within the last 10 s; a radar `horizon` limits range by the radar-horizon rule for the two heights.
3. Inside range and the field of regard and altitude limits, the detection happens with probability `pd_in_range × (1 − dropout × ea.dropout_multiplier)`.
4. The measurement is the true position plus Gaussian noise along the line of sight (`range_m`), across it (`cross_m`), and vertically (`height_m`), plus any instance `bias_m` (the registration test) and, for a spoofed AIS, the spoof offset.
5. `source_time` is the opportunity time minus any electronic-attack clock skew (a lagging clock; a leading one would be quarantined by the gateway rule that source time may not lead receipt time by more than one second). `receipt_time` is `source_time` plus `latency_s.mean` plus a half-normal jitter, plus an extra 1 to 4 s with probability `out_of_order`, so late data arrives across sensors as the tracking core must handle (`gungnir-time`).
6. False alarms: Poisson(`false_alarms_per_scan × ea.fa_multiplier`) per opportunity, uniform within the sensor's largest range band and altitude limits.
7. Events change the parameters in time: `sensor_lost` stops a sensor, `ea_skew` and `ea_dropout` apply the electronic-attack multipliers to the named sensors, `sea_state` raises coastal-radar dropout.

## Types

| Type | Name | Range by class (m) | Pd | Period (s) | Noise range / cross / height (m) | Field of regard (deg) | Altitude (m) | Latency mean ± jitter (s) | Dropout | Out of order | False alarms per scan | Traits |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `radar.long` | Long-range surveillance radar (ridge site) | large 200000, medium 140000, small 70000, very-small 35000 | 0.9 | 2.0 | 40 / 120 / 250 | 0 to 360 | 30 to 40000 | 0.3 ± 0.15 | 0.05 | 0.02 | 0.1 | horizon |
| `radar.medium` | Medium-range air-defense radar (airfield site) | large 120000, medium 80000, small 45000, very-small 20000 | 0.92 | 1.0 | 25 / 60 / 150 | 0 to 360 | 20 to 25000 | 0.25 ± 0.1 | 0.04 | 0.02 | 0.08 | horizon |
| `radar.short` | Short-range counter-UAS radar (site) | large 15000, medium 10000, small 6000, very-small 3500 | 0.85 | 0.5 | 8 / 20 / 40 | 0 to 360 | 0 to 5000 | 0.15 ± 0.05 | 0.08 | 0.01 | 0.05 |  |
| `radar.coastal` | Coastal surveillance radar (port) | large 40000, medium 25000, small 14000, very-small 9000 | 0.8 | 2.5 | 15 / 60 / 0 | 180 to 360 | 0 to 300 | 0.4 ± 0.2 | 0.15 | 0.03 | 2.0 | horizon |
| `radar.ground` | Ground surveillance radar (ridge) | large 40000, medium 25000, small 12000, very-small 6000 | 0.75 | 4.0 | 20 / 60 / 0 | 30 to 150 | 0 to 50 | 0.5 ± 0.3 | 0.2 | 0.03 | 0.5 | horizon, moving_only |
| `eo-ir` | Electro-optical and infrared camera (cued) | high 15000, medium 8000, low 4000 | 0.7 | 0.5 | 150 / 15 / 15 | 0 to 360 | 0 to 10000 | 0.6 ± 0.3 | 0.25 | 0.05 | 0.02 | horizon, cued |
| `acoustic` | Acoustic sensor node (network) | loud 4000, moderate 1500, quiet 400 | 0.6 | 5.0 | 800 / 400 / 300 | 0 to 360 | 0 to 3000 | 2.5 ± 1.0 | 0.3 | 0.25 | 0.05 | gnss_dependent |
| `rf` | Radio-frequency detector (site) | datalink 8000, control 5000, radar 30000, none 0 | 0.8 | 1.0 | 1500 / 300 / 500 | 0 to 360 | 0 to 6000 | 0.8 ± 0.4 | 0.2 | 0.05 | 0.1 |  |
| `isr-video` | ISR video observation (from a UAS sortie) | high 12000, medium 8000, low 5000 | 0.8 | 3.0 | 25 / 25 / 10 | 0 to 360 | 0 to 100 | 2.5 ± 1.0 | 0.15 | 0.35 | 0.02 | mobile, gnss_dependent |
| `ais` | AIS receiver (cooperative maritime) | ais 60000, none 0 | 0.98 | 10.0 | 10 / 10 / 0 | 0 to 360 | 0 | 1.0 ± 0.5 | 0.02 | 0.05 | 0.0 | horizon, cooperative, spoofable |
| `adsb` | ADS-B receiver (cooperative air) | adsb 250000, none 0 | 0.98 | 1.0 | 30 / 30 / 30 | 0 to 360 | 0 to 15000 | 0.5 ± 0.2 | 0.02 | 0.02 | 0.0 | horizon, cooperative |
| `iff` | IFF interrogator (cooperative, friendly only) | iff 150000, none 0 | 0.95 | 4.0 | 100 / 300 / 300 | 0 to 360 | 0 to 20000 | 0.3 ± 0.1 | 0.05 | 0.01 | 0.0 | horizon, cooperative |
| `bft` | Blue-force tracking (own vehicles) | bft 100000, none 0 | 0.99 | 30.0 | 15 / 15 / 5 | 0 to 360 | 0 to 100 | 2.0 ± 1.0 | 0.05 | 0.2 | 0.0 | cooperative, gnss_dependent |

## Electronic-attack sensitivities

| Type | Clock skew (s) when attacked | Dropout multiplier | False-alarm multiplier |
|---|---|---|---|
| `radar.long` | 0 | 2.0 | 3.0 |
| `radar.medium` | 0 | 2.0 | 2.5 |
| `radar.short` | 0 | 3.0 | 4.0 |
| `radar.coastal` | 0 | 1.5 | 2.0 |
| `radar.ground` | 0 | 2.0 | 2.0 |
| `eo-ir` | 0 | 1.0 | 1.0 |
| `acoustic` | 2.0 | 1.2 | 1.0 |
| `rf` | 0 | 1.0 | 2.0 |
| `isr-video` | 1.5 | 3.0 | 1.0 |
| `ais` | 0 | 1.0 | 1.0 |
| `adsb` | 0 | 1.0 | 1.0 |
| `iff` | 0 | 1.0 | 1.0 |
| `bft` | 1.0 | 2.0 | 1.0 |

## Alignment with the tracking core

- Latency, jitter, and the out-of-order fraction produce the multi-rate, out-of-sequence arrival the `gungnir-fusion-async` pipeline is built for (Scenario 3 of `../scenario-crate-narrative.md`).
- Coastal-radar false alarms at sea state 4 produce the clutter of Scenario 2; the acoustic network's coarse, late reports produce the low-quality source Scenario 3 mixes in.
- The ISR-video bias in TT-06 is the registration test of the `gungnir-track-fusion` rows; the ground truth of the bias is in the set's `sensors.json`.
- Cooperative sensors carry identity evidence for `gungnir-identification` (GAP-010) without any classification appearing in the observation stream.

Rendered from `sensors.yaml` by `tools/build_catalogue.py`.

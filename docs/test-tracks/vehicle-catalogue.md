# Vehicle catalogue

Status: rendered by `tools/build_catalogue.py` from `catalogue-*.yaml` (version 2026-09-04) on 2026-09-04; do not edit by hand. One table per domain; per-platform detail pages with sources under `platforms/`. Every figure follows `sourcing-and-legal.md`; figures are published approximations organised by kinematic class for tracker testing, given as ranges. Speeds in m/s, altitudes in m, endurance in hours, range in km.

## Air (reviewer: subject-matter reviewer (air defense), pending)

| Class | Platform | Side | Speed | Altitude (typical) | Endurance | Range | RCS | IR | Acoustic | Confidence |
|---|---|---|---|---|---|---|---|---|---|---|
| One-way attack UAS, propeller | [Shahed-136 / Geran-2 family](platforms/shahed-136.md) | red | 40 to 52 | 50 to 4000 (100 to 1500) | 8 to 12 | 1500 to 2500 | small | low | loud | medium |
| One-way attack UAS, jet | [Jet-powered Geran variant (Geran-3 class)](platforms/geran-3-jet.md) | red | 80 to 170 | 50 to 5000 (100 to 2000) | 1.5 to 3 | 600 to 1000 | small | medium | loud | low |
| One-way attack UAS, propeller | [Ukrainian long-range strike UAS (Liutyi, UJ-22 class)](platforms/ua-long-range-strike-uas.md) | blue | 28 to 50 | 50 to 3000 (100 to 1000) | 6 to 10 | 700 to 1000 | small | low | loud | low |
| Loitering munition | [Lancet-3](platforms/lancet-3.md) | red | 22 to 31 | 50 to 5000 (500 to 3000) | 0.5 to 0.7 | 40 to 70 | very-small | low | moderate | medium |
| Loitering munition | [Switchblade 600](platforms/switchblade-600.md) | blue | 30 to 52 | 50 to 4500 (300 to 2000) | 0.6 to 0.7 | 40 to 90 | very-small | low | moderate | high |
| Small multirotor | [DJI Mavic 3 class multirotor](platforms/mavic-3.md) | both | 0 to 21 | 0 to 500 (30 to 150) | 0.5 to 0.75 | 5 to 15 | very-small | low | quiet | high |
| FPV strike quadcopter | [FPV strike quadcopter (7 to 10 inch)](platforms/fpv-strike-quad.md) | both | 15 to 45 | 1 to 300 (2 to 60) | 0.15 to 0.35 | 5 to 20 | very-small | low | quiet | medium |
| Tactical fixed-wing ISR UAS | [Orlan-10](platforms/orlan-10.md) | red | 21 to 42 | 100 to 5000 (1000 to 3000) | 14 to 18 | 120 to 600 | small | low | moderate | high |
| Tactical fixed-wing ISR UAS | [Leleka-100 / Furia class](platforms/leleka-100.md) | blue | 18 to 33 | 100 to 3000 (500 to 2000) | 2.5 to 4 | 45 to 100 | small | low | quiet | medium |
| Medium-altitude long-endurance UAS | [Bayraktar TB2](platforms/tb2.md) | blue | 36 to 61 | 500 to 7600 (4000 to 6000) | 24 to 27 | 150 to 300 | medium | medium | moderate | high |
| Medium-altitude long-endurance UAS | [Orion (Inokhodets)](platforms/orion-uas.md) | red | 33 to 56 | 500 to 7500 (3000 to 6000) | 20 to 24 | 150 to 300 | medium | medium | moderate | medium |
| Cruise missile, subsonic | [Kalibr (3M-14) land-attack cruise missile](platforms/kalibr.md) | red | 230 to 275 | 20 to 1000 (50 to 150) | 1.5 to 2.5 | 1500 to 2500 | small | medium | loud | medium |
| Cruise missile, subsonic | [Kh-101](platforms/kh-101.md) | red | 190 to 270 | 30 to 6000 (30 to 100) | 3 to 5 | 2500 to 4500 | very-small | medium | loud | medium |
| Cruise missile, subsonic | [Storm Shadow / SCALP-EG](platforms/storm-shadow.md) | blue | 270 to 310 | 30 to 1000 (30 to 100) | 0.3 to 0.6 | 250 to 560 | small | medium | loud | high |
| Cruise missile, subsonic | [R-360 Neptune](platforms/neptune.md) | blue | 250 to 310 | 3 to 300 (10 to 50) | 0.3 to 0.5 | 280 to 400 | small | medium | loud | medium |
| Cruise missile, supersonic and aeroballistic | [Kh-22 / Kh-32](platforms/kh-22.md) | red | 900 to 1400 | 500 to 40000 (12000 to 25000) | 0.15 to 0.3 | 600 to 1000 | large | high | loud | low |
| Cruise missile, supersonic and aeroballistic | [Kh-47M2 Kinzhal](platforms/kinzhal.md) | red | 1000 to 2000 | 1000 to 40000 (15000 to 30000) | 0.1 to 0.25 | 1500 to 2000 | medium | high | loud | low |
| Ballistic missile, short range | [9K720 Iskander-M (9M723)](platforms/iskander-m.md) | red | 700 to 2100 | 0 to 50000 (20000 to 50000) | 0.05 to 0.12 | 50 to 500 | medium | high | loud | medium |
| Ballistic missile, short range | [MGM-140 ATACMS](platforms/atacms.md) | blue | 700 to 1200 | 0 to 50000 (20000 to 50000) | 0.05 to 0.1 | 70 to 300 | medium | high | loud | medium |
| Ballistic missile, short range | [KN-23 class](platforms/kn-23.md) | red | 700 to 2000 | 0 to 50000 (20000 to 50000) | 0.08 to 0.15 | 400 to 900 | medium | high | loud | low |
| Glide bomb | [KAB with UMPK glide kit (FAB-500 to FAB-1500 class)](platforms/kab-umpk.md) | red | 180 to 300 | 0 to 15000 (3000 to 12000) | 0.03 to 0.08 | 40 to 70 | small | low | quiet | medium |
| Glide bomb | [JDAM-ER class glide bomb](platforms/jdam-er.md) | blue | 180 to 300 | 0 to 12000 (3000 to 10000) | 0.03 to 0.08 | 40 to 75 | small | low | quiet | medium |
| Tactical fixed-wing aircraft | [Su-25](platforms/su-25.md) | both | 120 to 265 | 20 to 7000 (50 to 3000) | 1.5 to 2.5 | 400 to 750 | large | high | loud | high |
| Tactical fixed-wing aircraft | [Su-34](platforms/su-34.md) | red | 150 to 550 | 30 to 15000 (100 to 10000) | 2 to 4 | 1000 to 1500 | large | high | loud | high |
| Tactical fixed-wing aircraft | [F-16](platforms/f-16.md) | blue | 150 to 600 | 30 to 15000 (500 to 12000) | 1.5 to 3 | 500 to 1200 | medium | high | loud | high |
| Rotary wing | [Ka-52](platforms/ka-52.md) | red | 0 to 85 | 5 to 5500 (10 to 100) | 1.5 to 2.5 | 400 to 500 | large | high | loud | high |
| Rotary wing | [Mi-8 / Mi-17](platforms/mi-8.md) | both | 0 to 70 | 5 to 5000 (20 to 300) | 2 to 4 | 450 to 800 | large | high | loud | high |
| Civil airliner | [Narrow-body airliner (A320 / 737 class)](platforms/airliner-a320.md) | civil | 60 to 260 | 0 to 12500 (8000 to 12000) | 3 to 6 | 3000 to 6000 | large | high | loud | high |
| Light aircraft | [Light aircraft (Cessna 172 class)](platforms/cessna-172.md) | civil | 25 to 65 | 0 to 4100 (300 to 2500) | 3 to 5 | 800 to 1200 | medium | medium | moderate | high |
| Air-defense interceptor missile | [Patriot PAC-2 and PAC-3 interceptors](platforms/pac-3.md) | blue | 1000 to 1700 | 0 to 25000 (1000 to 20000) | 0.01 to 0.03 | 20 to 160 | small | high | loud | medium |
| Air-defense interceptor missile | [NASAMS (AIM-120 class) interceptor](platforms/nasams-amraam.md) | blue | 800 to 1400 | 0 to 15000 (200 to 10000) | 0.01 to 0.02 | 15 to 40 | small | high | loud | medium |
| Air-defense interceptor missile | [S-300 and S-400 family interceptors (48N6 class)](platforms/s-400-interceptor.md) | red | 1200 to 2000 | 0 to 30000 (1000 to 25000) | 0.01 to 0.04 | 40 to 250 | small | high | loud | low |

## Sea (reviewer: subject-matter reviewer (maritime), pending)

| Class | Platform | Side | Speed | Altitude (typical) | Endurance | Range | RCS | IR | Acoustic | Confidence |
|---|---|---|---|---|---|---|---|---|---|---|
| Uncrewed surface vessel | [Magura V5](platforms/magura-v5.md) | blue | 8 to 22 | 0 | 20 to 60 | 450 to 800 | very-small | low | moderate | medium |
| Uncrewed surface vessel | [Sea Baby class](platforms/sea-baby.md) | blue | 8 to 25 | 0 | 20 to 60 | 800 to 1000 | very-small | low | moderate | low |
| Fast craft and patrol boat | [Raptor class patrol boat (Project 03160)](platforms/raptor-patrol-boat.md) | red | 5 to 25 | 0 | 12 to 36 | 500 to 600 | medium | medium | loud | high |
| Fast craft and patrol boat | [Gyurza-M class armoured boat](platforms/gyurza-m.md) | blue | 3 to 13 | 0 | 48 to 120 | 1500 to 1700 | medium | medium | loud | high |
| Surface combatant | [Karakurt and Buyan-M class corvettes](platforms/corvette-karakurt.md) | red | 2 to 15 | 0 | 240 to 360 | 4000 to 5000 | large | high | loud | high |
| Surface combatant | [Admiral Grigorovich class frigate](platforms/frigate-grigorovich.md) | red | 2 to 15 | 0 | 600 to 720 | 8000 to 9000 | large | high | loud | high |
| Amphibious, auxiliary, and civil traffic | [Ropucha class landing ship](platforms/landing-ship-ropucha.md) | red | 2 to 9 | 0 | 480 to 720 | 5000 to 11000 | large | high | loud | high |
| Amphibious, auxiliary, and civil traffic | [Coastal tanker and merchant class (civil)](platforms/tanker-coastal.md) | civil | 2 to 8 | 0 | 240 to 720 | 3000 to 10000 | large | high | loud | high |
| Amphibious, auxiliary, and civil traffic | [Fishing vessel and small craft (civil)](platforms/fishing-vessel.md) | civil | 0 to 12 | 0 | 6 to 240 | 20 to 500 | small | low | moderate | high |
| Submarine, surfaced or snorkelling | [Kilo class submarine (surfaced or snorkelling)](platforms/kilo-surfaced.md) | red | 2 to 6 | 0 | 720 to 1080 | 6000 to 12000 | small | low | quiet | high |

## Land (reviewer: subject-matter reviewer (land and fires), pending)

| Class | Platform | Side | Speed | Altitude (typical) | Endurance | Range | RCS | IR | Acoustic | Confidence |
|---|---|---|---|---|---|---|---|---|---|---|
| Main battle tank | [T-72 / T-80 / T-90 family](platforms/t-72.md) | both | 0 to 19 | 0 | 6 to 10 | 400 to 550 | large | high | loud | high |
| Main battle tank | [Leopard 2 / Challenger 2 / M1 Abrams](platforms/leopard-2.md) | blue | 0 to 20 | 0 | 6 to 10 | 400 to 550 | large | high | loud | high |
| Infantry fighting vehicle | [BMP-2 / BMP-3](platforms/bmp-2.md) | both | 0 to 19 | 0 | 8 to 12 | 550 to 600 | large | high | loud | high |
| Infantry fighting vehicle | [M2 Bradley / CV90 / Marder](platforms/bradley.md) | blue | 0 to 19 | 0 | 8 to 12 | 400 to 500 | large | high | loud | high |
| Armoured personnel carrier and MRAP | [BTR-82 / BTR-80](platforms/btr-82.md) | both | 0 to 22 | 0 | 8 to 12 | 600 to 700 | large | high | loud | high |
| Armoured personnel carrier and MRAP | [Stryker / M113 / MaxxPro](platforms/stryker.md) | blue | 0 to 29 | 0 | 8 to 12 | 480 to 700 | large | high | loud | high |
| Self-propelled artillery | [2S19 Msta-S / 2S3 Akatsiya](platforms/2s19-msta-s.md) | both | 0 to 17 | 0 | 8 to 12 | 450 to 500 | large | high | loud | high |
| Self-propelled artillery | [PzH 2000 / Caesar / M109 / Krab](platforms/pzh-2000.md) | blue | 0 to 28 | 0 | 8 to 12 | 350 to 600 | large | high | loud | high |
| Rocket artillery | [BM-21 Grad / Tornado-S](platforms/bm-21-grad.md) | both | 0 to 24 | 0 | 8 to 12 | 450 to 750 | large | high | loud | high |
| Rocket artillery | [M142 HIMARS / M270](platforms/himars.md) | blue | 0 to 26 | 0 | 8 to 12 | 480 to 640 | large | high | loud | high |
| Mobile air-defense system | [Buk / Tor / Pantsir vehicles](platforms/buk.md) | red | 0 to 25 | 0 | 8 to 12 | 500 to 700 | large | high | loud | high |
| Mobile air-defense system | [Patriot and NASAMS launcher vehicles; Gepard](platforms/patriot-launcher.md) | blue | 0 to 25 | 0 | 8 to 12 | 400 to 700 | large | high | loud | high |
| Electronic-warfare vehicle | [Krasukha / Leer-3 electronic-warfare vehicles](platforms/krasukha.md) | red | 0 to 25 | 0 | 8 to 12 | 500 to 900 | large | high | loud | medium |
| Electronic-warfare vehicle | [Bukovel-AD class counter-UAS EW](platforms/bukovel-ad.md) | blue | 0 to 25 | 0 | 8 to 24 | 300 to 600 | medium | medium | moderate | low |
| Logistics truck | [KamAZ / Ural / HEMTT logistics trucks](platforms/kamaz-truck.md) | both | 0 to 28 | 0 | 8 to 14 | 600 to 1000 | large | high | loud | high |
| Uncrewed ground vehicle | [Small logistics and engineering UGVs](platforms/small-ugv.md) | both | 0 to 6 | 0 | 2 to 10 | 10 to 60 | small | low | quiet | low |

## Validation record

| Date | Reviewer | Domain | Fields checked | Outcome |
|---|---|---|---|---|
| 2026-09-04 | Drafting agent | all | sources present, confidence present, envelope containment (`tools/build_catalogue.py --check`) | first draft; no subject-matter review yet |

## Change log

- 2026-09-04: first draft, 58 platform entries in 30 classes.

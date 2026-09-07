#!/usr/bin/env python3
# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

"""Validate generated test-track sets (plan 07, docs/test-tracks/validation.md).

Checks every set under testdata/tracks/samples (or the directories given): files
present, metadata versions current, truth within the class envelopes and physically
plausible, detections well formed and inside the gateway's rules, arrival order and
out-of-order fractions as the sensor models specify, counts matching metadata.
Writes validation-report.json into each set and records the result in metadata.json.

Usage (from the workspace root):

    python docs/test-tracks/tools/validate_tracks.py            # every sample set
    python docs/test-tracks/tools/validate_tracks.py testdata/tracks/samples/TT-01-sample

Exit status is non-zero when any set fails.
"""
from __future__ import annotations

import json
import math
import sys
from collections import defaultdict
from pathlib import Path

import yaml

HERE = Path(__file__).resolve().parent
TT = HERE.parent
ROOT = TT.parent.parent
MAX_MAGNITUDE = 1.0e7
MAX_SOURCE_AHEAD = 1.0
MAX_LATENCY = 5.0


def load(name):
    return yaml.safe_load((TT / name).read_text(encoding="utf-8"))


def norm(v):
    return math.sqrt(sum(x * x for x in v))


def validate(set_dir: Path, classes, sensors_yaml, versions) -> dict:
    checks = []

    def check(name, ok, detail=""):
        checks.append({"check": name, "passed": bool(ok), "detail": detail})

    files = ["metadata.json", "truth.jsonl", "detections.jsonl", "sensors.json", "events.jsonl"]
    missing = [f for f in files if not (set_dir / f).exists()]
    check("files present", not missing, f"missing {missing}" if missing else "")
    if missing:
        return {"passed": False, "checks": checks}
    meta = json.loads((set_dir / "metadata.json").read_text(encoding="utf-8"))
    for k, want in versions.items():
        check(f"{k} current", meta.get(k) == want, f"set {meta.get(k)} vs {want}")
    sensors = {s["id"]: s for s in json.loads((set_dir / "sensors.json").read_text(encoding="utf-8"))["sensors"]}
    cls_by = {c["id"]: c for c in classes["classes"]}

    # truth
    truth = [json.loads(l) for l in (set_dir / "truth.jsonl").read_text(encoding="utf-8").splitlines() if l.strip()]
    per = defaultdict(list)
    for r in truth:
        per[r["entity"]].append(r)
    env_viol, teleport, time_viol, alive_viol = [], [], [], []
    for eid, recs in per.items():
        env = cls_by[recs[0]["class"]]["envelope"]
        vmax = env["speed_mps"][1]
        seen_dead = False
        for i, r in enumerate(recs):
            spd = norm(r["vel"])
            if spd > vmax * 1.15 + 0.5:
                env_viol.append((eid, r["t"], "speed", round(spd, 1)))
            if not (env["altitude_m"][0] - 1.0 <= r["pos"][2] <= env["altitude_m"][1] + 1.0):
                env_viol.append((eid, r["t"], "altitude", round(r["pos"][2], 1)))
            if i > 0:
                prev = recs[i - 1]
                dt = r["t"] - prev["t"]
                if dt <= 0:
                    time_viol.append((eid, r["t"]))
                else:
                    d = norm([r["pos"][j] - prev["pos"][j] for j in range(3)])
                    if d / dt > max(vmax * 1.3, 5.0) + 1.0:
                        teleport.append((eid, r["t"], round(d / dt, 1)))
                if seen_dead and r["alive"]:
                    alive_viol.append((eid, r["t"]))
            if not r["alive"]:
                seen_dead = True
    check("truth speed within class envelope", not [v for v in env_viol if v[2] == "speed"], f"{len([v for v in env_viol if v[2] == 'speed'])} violations, first {env_viol[:3]}")
    check("truth altitude within class envelope", not [v for v in env_viol if v[2] == "altitude"], f"{[v for v in env_viol if v[2] == 'altitude'][:3]}")
    check("truth has no teleports", not teleport, f"{teleport[:3]}")
    check("truth time monotonic per entity", not time_viol, f"{time_viol[:3]}")
    check("truth alive flag never revives", not alive_viol, f"{alive_viol[:3]}")
    check("truth record count matches metadata", len(truth) == meta["counts"]["truth_records"], f"{len(truth)} vs {meta['counts']['truth_records']}")
    check("entity count matches metadata", len(per) == meta["counts"]["entities"], f"{len(per)} vs {meta['counts']['entities']}")

    # detections
    lines = [l for l in (set_dir / "detections.jsonl").read_text(encoding="utf-8").splitlines() if l.strip()]
    bad_parse, bad_rule, bad_sensor = [], [], []
    last_receipt = -1e9
    order_ok = True
    per_sensor = defaultdict(list)
    for n, line in enumerate(lines, 1):
        try:
            d = json.loads(line)
        except json.JSONDecodeError as e:
            bad_parse.append((n, str(e)))
            continue
        try:
            # `measurement` is the tagged gungnir_model::Measurement since DN-27
            # (docs/design/DN-27-bearing-only-detections.md section 4). A generated set
            # is a position feed: any other variant here is a shape error and is
            # reported as one rather than skipped.
            measurement = d["measurement"]
            assert list(measurement) == ["Position"], "not a Position measurement"
            position = measurement["Position"]
            m = position["enu"]
            variance = position["variance_m2"]
            st, rt = float(d["source_time"]), float(d["receipt_time"])
            prov = d["provenance"]
            assert isinstance(d["sensor"], int) and len(m) == 3 and len(variance) == 3
            assert all(float(v) > 0.0 for v in variance)
            assert all(math.isfinite(float(x)) for x in m) and math.isfinite(st) and math.isfinite(rt)
            assert isinstance(prov["source_sensor_ids"], list) and isinstance(prov["algorithm_version"], str)
        except (KeyError, AssertionError, TypeError, ValueError) as e:
            bad_parse.append((n, f"shape {e}"))
            continue
        if norm(m) > MAX_MAGNITUDE:
            bad_rule.append((n, "magnitude"))
        if st > rt + MAX_SOURCE_AHEAD:
            bad_rule.append((n, "source ahead of receipt"))
        if rt - st > MAX_LATENCY:
            bad_rule.append((n, "latency exceeds the gateway window"))
        if d["sensor"] not in sensors:
            bad_sensor.append((n, d["sensor"]))
        if rt < last_receipt - 1e-9:
            order_ok = False
        last_receipt = rt
        per_sensor[d["sensor"]].append((st, rt))
    check("detections parse as DetectionView", not bad_parse, f"{bad_parse[:3]}")
    check("detections satisfy the gateway rules", not bad_rule, f"{len(bad_rule)} violations, first {bad_rule[:3]}")
    check("detections name known sensors", not bad_sensor, f"{bad_sensor[:3]}")
    check("detections in receipt order", order_ok)
    check("detection count matches metadata", len(lines) == meta["counts"]["detections"], f"{len(lines)} vs {meta['counts']['detections']}")
    ooo_report = {}
    ooo_bad = []
    for sid, pairs in per_sensor.items():
        srcs = [p[0] for p in pairs]
        inversions = sum(1 for i in range(1, len(srcs)) if srcs[i] < srcs[i - 1])
        frac = inversions / max(1, len(srcs) - 1)
        spec = sensors[sid]["params"]["out_of_order"]
        ooo_report[str(sid)] = round(frac, 3)
        if frac > spec * 4 + 0.08:
            ooo_bad.append((sid, round(frac, 3), spec))
    check("out-of-order fraction per sensor within the model", not ooo_bad, f"{ooo_bad[:3]}")
    exp = meta.get("expected", {})
    if "entities" in exp and meta["variant"] == "full":
        check("expected entity count (full set)", exp["entities"] == meta["counts"]["entities"], f"{meta['counts']['entities']} vs {exp['entities']}")
    passed = all(c["passed"] for c in checks)
    return {"passed": passed, "checks": checks, "out_of_order": ooo_report}


def main(argv):
    classes = load("classes.yaml")
    sensors_yaml = load("sensors.yaml")
    versions = {"catalogue_version": load("catalogue.yaml")["version"], "classes_version": classes["version"],
                "sensors_version": sensors_yaml["version"], "scenarios_version": load("scenarios.yaml")["version"]}
    dirs = [Path(a) for a in argv] if argv else sorted(p for p in (ROOT / "testdata" / "tracks" / "samples").iterdir() if p.is_dir())
    all_ok = True
    for d in dirs:
        report = validate(d, classes, sensors_yaml, versions)
        (d / "validation-report.json").write_text(json.dumps(report, indent=1) + "\n", encoding="utf-8", newline="\n")
        meta_path = d / "metadata.json"
        if meta_path.exists():
            meta = json.loads(meta_path.read_text(encoding="utf-8"))
            meta["validation"] = {"passed": report["passed"], "checks": len(report["checks"]), "report": "validation-report.json"}
            meta_path.write_text(json.dumps(meta, indent=1) + "\n", encoding="utf-8", newline="\n")
        failed = [c for c in report["checks"] if not c["passed"]]
        print(f"{d.name}: {'PASS' if report['passed'] else 'FAIL'} ({len(report['checks'])} checks)" + ("" if report["passed"] else f" failed: {[(c['check'], c['detail']) for c in failed]}"))
        all_ok &= report["passed"]
    return 0 if all_ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

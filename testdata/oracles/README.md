# Oracle fixtures

Recorded outputs of the external oracles named in `../../docs/verification-capability-table.md`,
checked in so that the differential tests in the tracking-core crates run on any machine
without Python installed, and so that a disagreement is attributable to a specific oracle
version rather than to whatever happened to be on the developer's path.

A fixture is **evidence, not a criterion**. The criterion stays in the capability table;
these files only record what the oracle answered.

## What is here

| Directory | Oracle | Capability-table row | Generator |
|---|---|---|---|
| `coord/` | `pymap3d` 3.2.0 | Coordinate frame transforms (ECEF/ENU/NED/geodetic) | `tools/gen_coord_fixtures.py` |
| `allocation/` | Custom textbook DP (the generator itself) | Bellman/DP resource-to-track assignment | `tools/gen_allocation_fixtures.py` |
| `filters/ekf.json`, `ukf.json`, `rts.json` | `filterpy` 1.4.5 | EKF, UKF, RTS smoother | `tools/gen_nonlinear_fixtures.py` |
| `filters/imm.json` | `filterpy` 1.4.5 `IMMEstimator` -- **not** the Stone Soup IMM §2 named, which does not exist in 1.9.1 | Interacting Multiple Model | `tools/gen_imm_sqrt_fixtures.py` |
| `filters/sqrt.json` | `filterpy` 1.4.5 standard-form `KalmanFilter`, which is the row's own stated method | Square-root / UDU form | `tools/gen_imm_sqrt_fixtures.py` |
| `filters/particle.json` | Reference SIR over `filterpy` 1.4.5 `systematic_resample`, with the exact Kalman posterior carried alongside | Particle filter (statistical) | `tools/gen_imm_sqrt_fixtures.py` |
| `association/jpda.json` | Stone Soup 1.9.1 `JPDA` over `PDAHypothesiser`, driven directly | Joint probabilistic data association | `tools/gen_jpda_fixtures.py` |
| `track/fusion.json` | Hand-derived covariance intersection (scipy 1.18.1 bounded Brent), cross-checked against Stone Soup `ChernoffUpdater` at a fixed ω | Track-to-track fusion | `tools/gen_track_fusion_fixtures.py` |
| `track/registration.json` | Hand-derived weighted least squares; the bias is injected by construction | Sensor registration | `tools/gen_track_fusion_fixtures.py` |
| `track/phd.json` | The textbook Vo--Ma GM-PHD recursion. **Stone Soup 1.9.1 was driven and disagrees**; the fixture carries the confirmed cause and the part that is not explained | Gaussian-mixture PHD | `tools/gen_phd_fixtures.py` |
| `track/cphd.json` | The closed-form Gaussian-mixture CPHD update (Vo/Vo/Cantoni 2007), re-derived. **Stone Soup 1.9.1 has no CPHD updater at all**; the fixture carries the brute-force cross-check that stands in for a library comparison | Gaussian-mixture CPHD | `tools/gen_cphd_fixtures.py` |
| `track/lmb.json` | The LMB filter (Reuter/Vo/Vo/Dietmayer 2014), re-derived, with the association marginals from literal enumeration. **Stone Soup 1.9.1 has no GLMB and no LMB at all** -- not partial, absent, established three ways; the fixture carries five independent checks including an untruncated delta-GLMB run alongside, which measures what the LMB approximates | Labelled multi-Bernoulli | `tools/gen_lmb_fixtures.py` |

## MATLAB

The capability table names MATLAB oracles for several §1 rows
(`trackingKF`, `trackingEKF`, `initcvkf`, `initcakf`, `initctekf`, `assignmunkres`,
`trackerPHD`, and others). **MATLAB is not installed on the development machine and no
MATLAB fixture is generated here.** Rows whose only oracle is MATLAB stay Draft; rows with
both a Python and a MATLAB oracle are gated on the Python column alone, and the capability
table says so per row rather than implying a MATLAB run that did not happen. Nothing in
this directory should be read as a MATLAB result.

## Regenerating

The generators need the oracle packages, which are deliberately *not* a workspace
dependency: they are development tooling, run by hand when a fixture is added or an oracle
version is bumped.

**Installed on this machine 2026-09-06**: `.venv-oracles/` holds `filterpy` 1.4.5 and
`stonesoup` 1.9.1 alongside `numpy` and `scipy`, which is what makes the remaining Area A
§1 rows gateable against the oracles they name (the nonlinear estimators against
`filterpy`, JPDA/MHT/PHD against Stone Soup). The directory is git-ignored; the fixtures
it produces are not.

```bash
python -m venv .venv-oracles
./.venv-oracles/Scripts/python -m pip install numpy scipy pymap3d filterpy stonesoup
```

Then, from this directory:

```bash
./.venv-oracles/Scripts/python tools/gen_coord_fixtures.py
```

Each generator stamps the oracle's version into the fixture it writes. When a regenerated
fixture differs from the committed one by more than the row's tolerance, that is a finding
to investigate and record, not a file to overwrite quietly: either the oracle changed its
algorithm or ours did.

## Conventions

- Angles are radians, distances metres, matching the Rust types.
- Every fixture carries `row`, `oracle`, `oracle_version`, `python`, and `generated_by`.
- Cases are named; the Rust test prints the worst-case error and the case that produced it,
  so a tightening tolerance shows where the margin actually is.

# DN-01 Defended assets

Closes GAP-026. Status: first draft, 2026-09-05. **Design only; no code exists.** What the
owner has signed of this note is in [`../signatures.md`](../signatures.md).

## 1. The gap and the thread step it blocks

MT-01 step 4 asks the system to prioritize a raid against the assets being defended.
Today `ClosingSpeedAssessor` scores every track against **one** point, `protected_point_enu`,
with a single `max_range_m`. There is no list, no priority, and no warning obligation, so
saturation triage in MT-01, missiles-first ordering in MT-02, warning in MT-04, and
laydown planning in MT-09 all have nothing to rank against.

This is the keystone of the design set: DN-02, DN-03, DN-04, DN-05, and DN-19 all read
what is defined here.

## 2. The owning component

Split three ways, because three different crates need three different things:

| Concern | Crate | Why |
|---|---|---|
| The asset type itself | `gungnir-model` | Assessment, configuration, reporting, the interface, and the viewport all need it. AP-06 puts a shared type in the lowest crate that needs it |
| The configured list and its validation | `gungnir-config` | It is a baseline section, versioned and promotable like every other |
| Scoring against the list | `gungnir-assessment` | It already owns `ThreatAssessor` |

`gungnir-assessment` depends on `gungnir-model` only, and `gungnir-config` depends on
`gungnir-model` only. Putting the type in the model means **no new dependency edge**.

## 3. Types

In `gungnir-model`:

```rust
/// Identifier of a defended asset, stable across baseline versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord,
         serde::Serialize, serde::Deserialize)]
pub struct AssetId(pub u32);

/// How the asset is defended, ordered. `Critical` outranks `High` and so on; the
/// ordinal is what the score multiplies by, through `AssetPriority::weight`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default,
         serde::Serialize, serde::Deserialize)]
pub enum AssetPriority {
    Low,
    #[default]
    Medium,
    High,
    Critical,
}

impl AssetPriority {
    /// 0.25, 0.5, 0.75, 1.0. Named rather than free so that two deployments'
    /// scores are comparable and MOP-28 monotonicity is checkable.
    pub fn weight(self) -> f64 { /* ... */ }
}

/// The shape an asset occupies. A point is a mast or a substation; an area is a
/// port, an airfield, or a built-up area that cannot be reduced to one coordinate
/// without changing which track threatens it.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AssetExtent {
    Point { position: Geodetic },
    Circle { center: Geodetic, radius_m: f64 },
}

/// What the deployment has undertaken to do when a threat approaches this asset.
/// Absent means no obligation, which is different from an obligation with zero
/// lead time and must not be conflated with it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WarningObligation {
    /// Seconds of warning owed before predicted impact.
    pub lead_time_s: f64,
    /// Endpoint name in the baseline, resolved per D-08's generic-endpoint rule.
    pub channel: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DefendedAsset {
    pub id: AssetId,
    pub name: String,
    pub extent: AssetExtent,
    pub priority: AssetPriority,
    pub warning: Option<WarningObligation>,
    /// Free text for the operator; never parsed. Treated as untrusted by the
    /// assistant (`../ai/safety-boundaries.md`).
    pub note: Option<String>,
}

/// The list as the picture sees it, with the baseline version it came from so a
/// score can be traced to the list that produced it.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AssetListView {
    pub baseline_version: u32,
    pub assets: Vec<DefendedAsset>,
}
```

`AssetExtent` deliberately offers two shapes and not a polygon. A polygon needs a
containment routine, which is `gungnir-geo`'s concern, and the model may not depend on
geo. If polygons prove necessary, the extent gains a variant carrying a layer reference
and `gungnir-geo` resolves it; that is a later change with a version consequence, not a
reason to put geometry in the model now.

## 3a. Refinement made during implementation, 2026-09-05

The design showed `AssetListAssessor` holding an `AssetListView` directly. It cannot:
assets are geodetic, tracks are local ENU metres, and the conversion lives in
`gungnir-coord`, which `gungnir-assessment` does not depend on. Adding that edge was not
among the five the engineering reviewer accepted, and `ARCHITECTURE.md` does not draw it.

**The caller converts**, supplying each asset's ENU centre through an `AssetAnchor`,
exactly as `ClosingSpeedAssessor` already takes a `protected_point_enu`. A helper,
`anchor_list`, takes the list and a conversion closure so the binaries can do it in one
line. Nothing else in this note changes, and no edge is added.

This is recorded rather than quietly absorbed because it is the kind of frame mismatch a
design pass is prone to missing and an implementation always finds.

## 4. Edges

**None.** The alternative considered and rejected was `gungnir-assessment` to
`gungnir-config`, which the plan lists as allowed-but-not-recommended. Putting
`DefendedAsset` in the model removes the need for it, keeps configuration out of the
scoring path, and lets a replayed session carry its own asset list without reading a
baseline file.

## 5. Behaviour

`gungnir-assessment` gains an assessor that supersedes the single-point one:

```rust
pub struct AssetListAssessor {
    pub assets: AssetListView,
    /// Range beyond which a track scores zero against any asset.
    pub max_range_m: f64,
}

/// Which asset a track threatens, and how soon.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AssetExposure {
    pub asset: AssetId,
    pub range_m: f64,
    pub time_to_impact_s: Option<f32>,
}
```

`RiskScore` gains one field, `exposure: Option<AssetExposure>`, naming the asset that
produced the score. Adding a field with a default is compatible under the interface
contract's own rule.

Scoring rule: for each track, compute exposure against every asset, keep the exposure with
the highest product of proximity and priority weight, and report that asset. Stale tracks
still score zero and are never allocated against, unchanged.

**When the list is empty, which is the honest-status case that matters.** The assessor
does not silently fall back to a hidden point and does not score everything zero. It
returns scores with `exposure: None` and the health summary reports that no asset list is
configured, so PN-01 can say so. A zero score and an unconfigured system look identical to
an operator otherwise, and that is exactly the confusion AP-02 exists to prevent.

`ClosingSpeedAssessor` is retained, unchanged, as the degenerate case used by tests and by
a deployment that genuinely defends one point.

## 6. Configuration and interface delta

`gungnir-config` gains:

```rust
pub struct AssetConfig {
    pub id: u32,
    pub name: String,
    pub position: [f64; 3],
    pub radius_m: Option<f64>,
    pub priority: String,
    pub warning_lead_time_s: Option<f64>,
    pub warning_channel: Option<String>,
    pub note: Option<String>,
}
```

and `ConfigBaseline.assets: Vec<AssetConfig>`, defaulting to empty.

Validation rules added to `validate`:

1. Asset identifiers are unique.
2. `priority` parses to a known variant; an unknown string is an error, not a default,
   because a silently downgraded priority is a safety problem.
3. `radius_m`, when present, is finite and positive.
4. A warning obligation has both a lead time and a channel, or neither.
5. The channel names an endpoint the baseline declares (D-08).

`SUPPORTED_CONFIG_VERSION` stays 1: the section is additive with a default.

Interface: `GET /v2/snapshot` gains `assets: AssetListView` in `SnapshotResponse`. Additive
and therefore compatible.

## 7. User-interface delta

| Panel | Change |
|---|---|
| PN-02 Viewport | Assets drawn as a layer, priority by symbol weight rather than by colour alone (`../ux/accessibility.md`) |
| PN-04 Track detail | The evidence card names the threatened asset and the priority contribution to the score |
| PN-16 Planning panel | The asset list is the panel's primary content; it already lists GAP-026 as a blocker |
| PN-14 Configuration editor | The asset section, with validation results inline |
| PN-01 Status strip | States "no asset list configured" when the list is empty |

## 8. Verification

New row for `verification-capability-table.md` §2:

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-3.1 Defended-asset list | Property test over generated lists, plus a baseline promotion test | A priority change in a promoted baseline is reflected in scoring within two ticks (MOP-27); an unknown priority string fails validation rather than defaulting; an empty list yields `exposure: None` and an unconfigured health state, never a zero score presented as a real one | Generated asset lists; TT-01 sample tracks |

Monotonicity of the score in asset priority and in time to impact is MOP-28 and is verified
with DN-02, which supplies time to impact.

## Traceability

GAP-026; CAP-3.1, and CAP-5.6 for the baseline section; MOP-27, MOP-28; MT-01 step 4,
MT-02, MT-04, MT-09; `../ux/wireframes/WF-16-planning.puml`; principles AP-02, AP-06.
Read by DN-02, DN-03, DN-04, DN-05, DN-19.

## 9. Amendment 1

Raised 2026-09-06.

Raised by GAP-017's implementation, which found the same fault here. §3 has
`AssetListView::baseline_version` "so a score can be traced to the list that produced
it", and the implementation stamped `ConfigBaseline::version`, the schema version --
the same number for every promotion, so no score could be traced to any particular list.
The baseline now carries `revision`, advanced on every promotion and refused when not
(`ConfigError::RevisionNotAdvanced`), and `asset_list()` stamps that. The field keeps its
name; its meaning is the revision.

## 10. Amendment 2: the score reads time to impact

Raised 2026-09-25 by GAP-124, under D-83.

§5's rule kept the exposure with the highest product of proximity and priority, and the
implementation multiplied a closing factor of 1.0 or 0.5 into it. §8 left MOP-28's
"monotonic in time to impact" to be verified with DN-02, and the GAP-067 walk found the
score never read time to impact at all: a far, fast track arriving in 40 s scored below a
near, slow one arriving in 200 s, and the row's only fixture moved range at one speed, so
it could not tell.

**The kinematic factor** (`gungnir_assessment::kinematics`) replaces the proximity and the
closing factor; priority, affiliation lethality and class lethality multiply it as before.
Against one asset, with `r` the range to its boundary and `v_c` the closing speed along the
line of sight:

1. **Time to impact is unchanged**: `T = r / v_c`, only while `v_c > 0`. §5 and `RiskScore`
   already defined it, and DN-03's warnings read it. It is **not** the time to the closest
   point of approach, for the reasons D-83 records: a track passing 20 km off would have
   a small time to "impact" and trigger impact warnings, and the score would drop by half
   the instant it passed. The closest approach stays on the exposure, computed by the
   predictor's own routine (`prediction::closest_on_course`, which the exposure used to
   restate).
2. **Urgency** `u = τ / (τ + T)`, computed as `1 / (1 + r / (τ v_c))` so nothing divides by
   the closing speed. `τ` is the baseline's `assessment.urgency_half_time_s` (default 60 s,
   finite and positive or the baseline is refused).
3. **Closing confidence** `c`: zero when not closing; for a closing track,
   `(Φ(z - 2) - Φ(-2)) / (1 - Φ(-2))` with `z` the closing speed in its own one-sigma from
   the velocity block of the covariance -- zero at zero, a half at two sigma, one well
   beyond. A covariance that cannot give a sigma credits the estimate fully.
4. **The factor** `K = c (1/2 + u/2) + (1 - c) p/2`, with `p = 1 - r / max_range`.

A confidently closing track scores on its time to impact alone, in `(1/2, 1]`; a track that
is not closing scores `p/2` as before, and has no time to impact. So among confidently
closing tracks of the same priority and class **the score is non-increasing in time to
impact whatever their ranges**, which is MOP-28's clause exactly, and every confidently
closing track outranks every track that is not closing at the same weights, which is what
monotonicity in time to impact means for a track that has none. Between the two the
covariance decides, so a contact whose closing is noise is not promoted as inbound and the
score is continuous as a track turns from closing to passing. A non-finite state gives no
factor, and the assessor reports no exposure rather than a NaN that would sort to the top
of a triage.

PN-04's evidence card shows the time to impact, the urgency, the closing confidence, the
proximity and the factor, read from `RiskScore::kinematics` rather than recomputed. The
verification is `gungnir-assessment/tests/time_to_impact.rs`; the row itself is unchanged,
and walking it is the owner's.

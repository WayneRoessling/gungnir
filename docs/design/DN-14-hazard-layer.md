# DN-14 Static hazard and barrier layer

Closes GAP-017. Status: first draft, 2026-09-05. **Design only; no code exists.**
The smallest note in the set, and it is small because `gungnir-geo` already has the shape
of the answer.

## 1. The gap and the thread step it blocks

MT-04 defends a port. Booms, nets, barriers, and static hazards are what the defence
actually rests on, and they are not in the picture. Geofences stand in, which conflates two
different things: a geofence is a rule about where we may act, and a boom is a physical
object that changes where a threat can go.

## 2. The owning component

`gungnir-geo`, which owns `MapLayer`, `LayerKind`, and `Geofence`.

## 3. Types

In `gungnir-geo`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LayerKind {
    RasterImagery,
    VectorFeatures,
    Geofence,
    /// Physical obstacles and hazards: booms, nets, barriers, wrecks, shoals.
    Hazard,
    /// Areas artillery may not strike (DN-05). Distinct from a no-go geofence,
    /// which constrains our own interceptors.
    NoFireArea,
}

/// A physical obstacle or hazard. Unlike a geofence it says nothing about what
/// we may do; it says what is there.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Hazard {
    pub name: String,
    pub kind: HazardKind,
    pub extent: HazardExtent,
    /// True when the obstacle stops a surface craft. A boom does; a shoal does
    /// for a deep-draught vessel and not for a jet ski, which is why this is a
    /// stated property and not inferred from the kind.
    pub blocks_surface: bool,
    /// Height above the surface, metres, where it constrains air movement.
    pub height_m: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum HazardKind {
    Boom,
    Net,
    Barrier,
    Wreck,
    Shoal,
    Other,
}

/// A hazard is a line more often than an area: a boom across a harbour mouth is
/// a segment, not a circle.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum HazardExtent {
    Polyline { points: Vec<Geodetic> },
    Circle { center: Geodetic, radius_m: f64 },
}

/// True if the segment from `a` to `b` crosses any hazard that blocks surface
/// movement. Companion to the existing `route_crosses_no_go`.
pub fn route_crosses_hazard(route: &[Geodetic], hazards: &[Hazard]) -> bool;
```

## 4. Edges

**None.** `gungnir-geo` depends on `gungnir-coord` and `gungnir-data`, which is all the
geometry needs.

## 5. Behaviour

The hazard layer is descriptive. It changes three things and no more:

1. **The viewport draws it**, so the operator sees the harbour as it is.
2. **Prediction consults it** (DN-02): a surface track's predicted path that crosses a
   surface-blocking hazard is flagged, because either the prediction is wrong or something
   unusual is happening, and both are worth showing. The prediction is **not** silently
   bent around the obstacle; a bent line asserts knowledge the system does not have.
3. **Planning consults it** (PN-16): a laydown that assumes an approach a boom closes is a
   laydown built on a wrong premise.

**What it does not do.** It does not deny an engagement. That is what a geofence is for,
and keeping the two apart is the reason for the separate type. A design that let a boom
deny an intercept would put a survey artefact into the authority chain.

**Currency is stated.** A hazard carries no automatic expiry, and a boom that was removed
last week is a hazard layer that lies. The layer therefore carries the baseline version it
came from, like the asset list, and the panel shows when the layer was last updated. That
is the honest limit of a static layer, and stating it is cheaper than pretending the layer
is live.

## 6. Configuration and interface delta

`ConfigBaseline.hazards: Vec<HazardConfig>`, defaulting to empty, with validation that
polylines have at least two points, radii are positive, and heights are finite.

Interface: `SnapshotResponse` gains `hazards: Vec<Hazard>`. Additive.

## 7. User-interface delta

| Panel | Change |
|---|---|
| PN-02 Viewport | Hazards as a toggleable layer, drawn distinctly from geofences because they mean different things |
| PN-16 Planning panel | Hazards as a planning input, with the layer's age visible |
| PN-04 Track detail | A note when a track's predicted path crosses a surface-blocking hazard |
| PN-14 Configuration editor | The hazard section |

## 8. Verification

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-2.5 Clutter-tolerant surface picture, hazard part | Unit tests on crossing detection plus an MT-04 replay | A route crossing a surface-blocking hazard is detected and one crossing a non-blocking hazard is not; no hazard ever contributes to a policy verdict; the layer's baseline version appears on every published hazard set | TT-04 sample set; generated harbour geometries |

The middle criterion is a negative test and is the one that matters: it fails if anybody
later wires hazards into the policy chain.

## Traceability

GAP-017; CAP-2.5; MT-04; feeds DN-02 and DN-05's no-fire variant;
`../ux/wireframes/WF-02-viewport.puml`, `WF-16-planning.puml`; principles AP-01, AP-02.

## 9. Amendment 1 -- **signed by the owner 2026-09-06**

Raised 2026-09-06; signed the same day. The same sign-off covers the code that conforms to it.

Raised by GAP-017 on 2026-09-06, by implementing §5 and §8 rather than by reading them.
Two corrections; both change what the note says, and each needs a signature.

**(a) The negative test is a source scan, not the absence of an edge.** §8 says no hazard
ever contributes to a policy verdict, and §4 says the note adds no edge, which reads as if
the edge were the guard. It cannot be: `gungnir-policy` already depends on `gungnir-geo`
for geofences, and always will. The guard is
`gungnir-geo/tests/no_hazard_in_the_policy_chain.rs`, which reads the sources of
`gungnir-policy` and `gungnir-command` and fails the day either names a hazard. The
criterion in §8 stands unchanged; this records how it is met.

**(b) "The baseline version" is the baseline's revision.** §5 has the layer carry "the
baseline version it came from", and the first implementation stamped
`ConfigBaseline::version` -- the **schema** version, `SUPPORTED_CONFIG_VERSION`, which is
the same number for every survey ever loaded and so dated nothing. The baseline now
carries `revision`, the deployment's per-promotion counter, which `ConfigStore::apply`
refuses to leave unadvanced (`ConfigError::RevisionNotAdvanced`), and
`HazardLayer::baseline_version` is that. The field keeps its name so the verification row
reads as written; what it holds is the revision. The same correction applies to DN-01's
asset list (DN-01 amendment 1).

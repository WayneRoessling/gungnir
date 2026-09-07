# DN-26 Laydown options

Unblocks GAP-087, and through it GAP-020's approach corridors and GAP-045's rehearsal
record. Status: **signed by the owner 2026-09-06, confirmed 2026-09-07** (the confirmation
was needed because the signed status appeared on disk with nobody present to vouch for
it; see GAP-087's register entry). `ConfigBaseline.laydowns` with its five refusals and
§6's options table are **both built (GAP-087, 2026-09-07)**; the rehearsal section, the
gap-acceptance control, first-engagement range and the viewport push this note's own
§7 and GAP-087's remaining item name are not.

## 1. The gap and what it blocks

PN-16 is the planning panel. `docs/ux/ux-to-code-map.md` gives it an options table, a
comparison between options, and a rehearsal a planner can run before committing. GAP-087
has stayed open through three batches for a reason that is not the panel's own:

**`ConfigBaseline` carries one set of sensor and resource positions.** There is one
laydown, it is the deployment's current one, and there is no way to describe a second.
An options table built on that has exactly one row, a comparison has nothing to compare,
and a planner is shown a screen whose purpose is choosing between alternatives that
cannot exist.

This is the same shape GAP-086 had to fix before GAP-053 could be built, and DN-24 §1
records that case in almost the same words: a registry built on a baseline with one
configuration would "hold exactly one candidate, validate it, promote it, and report a
promoted baseline in force. Every part of that would be true and the whole of it would be
theatre." A laydown table with one row is the same theatre with a map behind it.

So this note does for laydowns what DN-24 did for algorithm baselines: gives the thing
being chosen between a schema and an identity, so that the panel choosing between them
has something to choose.

**What is deliberately not in scope.** The panel itself, and the rehearsal. The rehearsal
record a submit carries is GAP-045, and GAP-045 was blocked on GAP-011, which is now
built. A panel could be started once this note is signed; it should not be started before,
because its central table would have to be designed around a type that does not exist yet.

## 2. The owning components

| Concern | Crate |
|---|---|
| The laydown identifier and the laydown itself | `gungnir-model` |
| Declaring laydowns in a deployment's configuration | `gungnir-config` |
| Evaluating a laydown's coverage | `gungnir-analytics` (exists; `combined_coverage`) |
| Comparing laydowns and ranking them | `gungnir-decision` |
| The panel | `gungnir-ui`, driven from `gungnir-app` |

Every one of these edges already exists. **This note adds no dependency edge**, which is
worth stating explicitly because DN-24's §5 had to add one and the omission was caught
only after the fact.

## 3. What a laydown is

**A named, complete placement of the sensors and effectors a deployment could adopt.**
Complete, not a delta: a laydown that described only what differs from the current one
would be unreadable the moment two of them differed from each other, and a planner
comparing three options needs to see three whole answers rather than three diffs against
a fourth thing.

A laydown is not a plan. A plan assigns resources to tracks and is decided in minutes; a
laydown decides where the resources physically are and is decided in days. They are
different panels, different authorities, and different lifetimes, and DN-06's engagement
authority does not reach a laydown.

One laydown in a deployment is marked **current**. That is what the system is actually
running, and it is the baseline every option is compared against. A configuration that
declares laydowns and marks none current is refused rather than defaulted: guessing which
placement is the real one is exactly the kind of quiet assumption that makes a comparison
meaningless.

## 4. Types

In `gungnir-model`:

```rust
/// A named candidate placement of a deployment's sensors and effectors.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct LaydownId(pub String);

/// Where one sensor sits in a laydown, and what it is doing there.
///
/// The mode is carried because a laydown that moved a radar without saying whether it
/// is searching or tracking has not described a coverage answer, and coverage is the
/// thing laydowns are compared on.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SensorPlacement {
    pub sensor: SensorId,
    /// Local ENU metres, in the deployment's own frame.
    pub position_enu: [f64; 3],
    pub mode: SensorMode,
}

/// Where one effector sits.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ResourcePlacement {
    pub resource: ResourceId,
    pub position_enu: [f64; 3],
}

/// A complete placement a deployment could adopt.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Laydown {
    pub id: LaydownId,
    /// What this option is for, in the planner's own words. Shown in the options table;
    /// a laydown whose reason for existing is not written down is one nobody can choose
    /// between.
    pub intent: String,
    pub sensors: Vec<SensorPlacement>,
    pub resources: Vec<ResourcePlacement>,
    /// True for exactly one laydown in a deployment: the placement actually in force.
    pub current: bool,
}
```

In `gungnir-config`, `ConfigBaseline` gains:

```rust
/// The laydowns this deployment can choose between. Empty is valid and means the
/// deployment has no alternatives declared, which is a different statement from having
/// one: an empty table says "none offered", a one-row table would say "here are your
/// options" and be lying.
pub laydowns: Vec<Laydown>,
```

**Validated at load**, and every one of these is a refusal rather than a default:

1. More than one laydown marked `current`, or none while `laydowns` is non-empty.
2. A `SensorId` or `ResourceId` no sensor or resource registry declares.
3. A duplicate `LaydownId`.
4. A sensor or resource placed in one laydown and absent from another. A laydown is
   complete by definition, and a partial one would produce a coverage answer with a
   silent hole in it.
5. A non-finite coordinate.

## 5. What a comparison is, and what it must never claim

A comparison of two laydowns is a comparison of **coverage answers**, computed by
`gungnir_analytics::combined_coverage`, which already exists and already takes a
line-of-sight model and a set of approaches. The panel does not compute coverage; it asks
for it, the way PN-09 asks for health.

Three things this note fixes now, because each is a place where a plausible number would
be worse than none:

**A comparison must carry the terrain model it was computed under.** Coverage over flat
terrain and coverage over a loaded digital elevation model are different answers, and
`gungnir-analytics` already distinguishes `FlatTerrainLineOfSight` from
`TerrainMaskLineOfSight`. A comparison that did not say which one it used would let a
planner compare a flat-earth answer for one option against a terrain-masked answer for
another and read the difference as a property of the laydowns.

**A laydown that could not be evaluated is not a laydown that scored zero.** The existing
`CoverageResponse::{Computed, NotComputed { reason }}` split is exactly this distinction
and the options table must carry it through: a row that says "not computed: no terrain
loaded for this sector" is useful, and a row showing 0% coverage for the same reason is a
lie about the ground.

**Ranking is advisory and must be labelled.** `gungnir-decision` may order the options by
uncovered approach length. It must not present that order as a recommendation, because
coverage is one of several things a laydown is chosen on -- survivability, logistics,
and the reason written in `intent` are others the system knows nothing about.

## 6. Behaviour

1. On load, `ConfigBaseline` validates the laydowns per §4 and refuses the baseline if
   any check fails, with the reason naming the laydown and the field.
2. The desktop evaluates coverage for the current laydown and for each option, using one
   line-of-sight model for all of them, chosen once and reported with the results.
3. The panel shows one row per laydown: its identifier, its intent, its coverage answer
   or the reason there is none, and its difference from the current one.
4. Adopting a laydown is **out of scope for this note**. Moving a sensor is a physical
   act with an authority chain this system does not model, and a button that appeared to
   do it would be the most dangerous control on the display. The panel compares; a person
   acts.

## 7. What this note deliberately does not do

* **No optimiser.** Nothing here searches for a good laydown. The options are the ones a
  planner declared, and generating placements would be a different capability with a
  different verification row.
* **No adoption path.** See §6 rule 4.
* **No per-laydown algorithm baseline.** A laydown is a physical placement; which filter
  is in force is DN-24's mission profile and the two are independent. Coupling them would
  make moving a radar silently change the tracker.
* **No rehearsal.** GAP-045, and it wants this note signed first so that it has something
  to rehearse against.

## 8. Configuration and interface delta

`ConfigBaseline` gains `laydowns: Vec<Laydown>`, defaulting to empty, so every existing
configuration file remains valid and describes a deployment with no declared alternatives.
That is the honest reading of a file that does not mention laydowns.

No API route is added. A laydown is a planning artefact of one deployment and no exchange
item in `gungnir_model::ExchangeItem` covers it; a peer that received one would learn
where this deployment is thinking of putting its sensors, which is not something any
agreement in DN-18 contemplates.

## 9. User-interface delta

`docs/ux/ux-to-code-map.md` PN-16 gains the options table described in §6, and its entry
should record that the panel is unblocked by this note and still unbuilt.

## 10. Verification

One new row in `docs/verification-capability-table.md` §1:

| Component | Test | Method | Criterion |
|---|---|---|---|
| `gungnir-config` laydown validation | `gungnir-config/tests/laydowns.rs` | each of §4's five refusals, and a valid multi-laydown baseline | every invalid baseline is refused naming the laydown and field; the valid one loads |

The comparison itself needs no new row: it is `gungnir-analytics`'s existing coverage
computation applied to more than one input, and that row is already gated.

## Traceability

CAP-1.4 (coverage and gaps), MT-05. `../ux/panels.md` PN-16. Related notes: DN-12
(coverage and gaps), DN-13 (sensor retasking -- which changes a sensor's *mode* within a
fixed laydown, and is the short-lived counterpart to this note's long-lived decision),
DN-24 (mission profiles and algorithm baselines -- the same shape of fix, for a different
thing being chosen between).

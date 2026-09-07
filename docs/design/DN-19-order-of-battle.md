# DN-19 Pattern of life and order of battle

Closes GAP-025. Status: first draft, 2026-09-05. **Design only; no code exists.**

## 1. The gap and the thread step it blocks

MT-08 step 5 asks the intelligence analyst to assess what was seen and update the order of
battle. `gungnir-reporting` produces per-session reports over one journal. Launch areas,
routes, timings, and unit locations across sessions have no product at all, so priors for
the next laydown come from somebody's memory.

Phase B of the architecture description found this cluster to be the least served part of
the product, and this note is the largest piece of it.

## 2. The owning component

`gungnir-reporting`, which already owns `ReportGenerator` and reads journals through
`gungnir-store`.

## 3. Types

In `gungnir-reporting`:

```rust
/// A versioned assessment of what is out there, built from many sessions.
/// Versioned rather than mutable: an order of battle that changes without a
/// history cannot be argued with.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OrderOfBattle {
    pub version: u32,
    pub produced: MissionTime,
    /// Sessions this version was built from, so any entry can be traced back.
    pub sources: Vec<SessionId>,
    pub entries: Vec<OrderOfBattleEntry>,
    pub releasability: Releasability,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OrderOfBattleEntry {
    pub entity: GlobalEntityId,
    pub label: String,
    pub class: Option<String>,
    /// Where it has been seen, with how often and how recently.
    pub locations: Vec<ObservedLocation>,
    pub first_seen: MissionTime,
    pub last_seen: MissionTime,
    /// Sightings behind this entry. A single-sighting entry and a
    /// hundred-sighting entry must never look the same.
    pub sighting_count: u32,
    /// Set by the analyst, never by the query.
    pub assessment: Option<String>,
}

/// Recurring behaviour: launch areas, transit routes, and times of day.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PatternOfLife {
    pub area: AssetExtent,
    /// Histogram of activity by hour of day, over the sessions queried.
    pub by_hour: [u32; 24],
    pub routes: Vec<ObservedRoute>,
    pub sessions_covered: u32,
    pub sessions_with_activity: u32,
}

pub trait CrossSessionQuery: Send + Sync {
    fn order_of_battle(&self, sessions: &[SessionId]) -> Result<OrderOfBattle, ReportError>;
    fn pattern_of_life(&self, sessions: &[SessionId], area: AssetExtent)
        -> Result<PatternOfLife, ReportError>;
}
```

## 4. Edges

**One: `gungnir-reporting` to `gungnir-identity`.**

An order of battle is a list of **entities**, not of tracks. The same craft seen in six
sessions is one entry with six sightings, and only `gungnir-identity` knows that:
`GlobalEntityId`, `EntityLineage`, and `MergeEvent` live there.

The alternative was to read entity identifiers out of the journal as opaque values.
Rejected: the lineage is what makes an entry defensible, and an analyst asked "why is this
one entity" needs the merge events, not an identifier.

Acyclic: `gungnir-identity` depends on `gungnir-model` only.

Recorded in [`dependency-edges.md`](dependency-edges.md).

## 5. Behaviour

**Every figure traces to a journal.** `MissionReport` already carries figures with journal
references, and this extends the property across sessions: an entry names the sessions and
the envelope sequences that produced it. An order of battle that cannot be traced back is
an opinion with a version number.

**Cross-session correlation is the hard part and is honest about its limits.** GAP-019 is
open: the identity resolver correlates by session track identifier only. Until it closes:

1. An entry is built from entities the resolver actually merged, and no more.
2. The product reports how many tracks it could **not** attribute to an entity, so the
   analyst sees the coverage of the assessment rather than assuming it is complete.
3. It does not guess. Two similar tracks in different sessions stay two entries, and the
   analyst may merge them with a recorded assessment.

Point 3 is the whole design stance: **the product assembles evidence; the analyst
concludes.** An automatic merge on kinematic similarity would produce a confident order of
battle nobody can audit, which is the intelligence-product version of the over-trust
problem the assistant design also guards against.

**Pattern of life reports its denominator.** `sessions_covered` and
`sessions_with_activity` are both published, because "seen on six occasions" means
different things out of six sessions and out of six hundred. A histogram without its
denominator is the most common way an activity chart misleads.

**Versioning.** A new query produces a new version; versions are kept. An analyst's
assessment attaches to an entry and carries forward, with the analyst named.

## 6. Configuration and interface delta

`ConfigBaseline.reporting.retention_sessions: usize`, bounding how far back a query may
reach, defaulted and validated positive. Journal retention is a deployment question and
this makes the query respect it.

Interface, additive:

| Method and path | Response | Authorization action |
|---|---|---|
| `GET /v2/order-of-battle?sessions=...` | `OrderOfBattle` | `picture.view` |
| `GET /v2/pattern-of-life?sessions=...&area=...` | `PatternOfLife` | `picture.view` |

Both are filtered by DN-17.

## 7. User-interface delta

| Panel | Change |
|---|---|
| PN-13 Reports | Order of battle and pattern of life as report types, with the session set as an input and the unattributed count shown |
| PN-04 Track detail | Prior sightings of this entity, which is what makes a live track legible in MT-08 |
| PN-16 Planning panel | Pattern of life as a laydown input, with the denominator visible |
| PN-02 Viewport | Observed launch areas and routes as a layer, drawn as historical rather than live |

## 8. Verification

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-2.12 Pattern of life and order of battle | Multi-session replay with known truth across sessions | Every entry traces to the sessions and sequences that produced it; the unattributed track count is reported and correct; no two entities are merged without a recorded merge event or an analyst assessment; the pattern-of-life denominator matches the session set queried; sighting counts are correct against truth | Several TT-08 sample sets replayed as separate sessions |

## Traceability

GAP-025; CAP-2.12; MT-08 step 5; depends on GAP-019 for correlation quality, DN-01 for
`AssetExtent`, DN-17 for the marking; feeds DN-18 and DN-20;
`../ux/wireframes/WF-13-reports.puml`; principles AP-02, AP-07, AP-08.

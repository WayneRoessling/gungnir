# DN-17 Releasability marking and enforcement

Closes GAP-062. Status: **signed off by the owner 2026-09-05.** Design only; no code exists yet.
**Human-owned and signed**: this is `gungnir-security`'s enforcement and the interface's
write path. The owner signed it on 2026-09-05.

D-06 settled the shape: **modelled in increment 3, enforced per caller in increment 4.**
This note designs both halves at once so the marking is not designed twice.

## 1. The gap and the thread step it blocks

MT-05 and MT-08 produce pictures and products that a coalition partner should see some of
and not all of. Nothing carries a marking and nothing enforces one, so the only safe answer
today is to share nothing, which makes the sector an island.

## 2. The owning component

`gungnir-model` owns the marking type, because tracks, reports, plans, handoffs, and the
interface all carry it. `gungnir-security` owns the decision about whether a given caller
may receive a given marking. `gungnir-api` applies that decision at the boundary.

`gungnir-api` already depends on `gungnir-security`. **No new edge.**

## 3. Types

In `gungnir-model`:

```rust
/// Who may receive this. Deliberately a small, ordered set plus a named-parties
/// case, because an expressive marking language nobody configures correctly is
/// worse than a coarse one everybody understands.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Releasability {
    /// Never leaves this deployment.
    #[default]
    Internal,
    /// May go to the named parties and nobody else.
    Parties(BTreeSet<String>),
    /// May go to any authenticated peer.
    AllPeers,
}

impl Releasability {
    /// True if a caller belonging to `party` may receive this.
    pub fn permits(&self, party: &str) -> bool { /* ... */ }

    /// The marking of a product derived from several inputs: the most
    /// restrictive of them. Never the least.
    pub fn combine(items: impl IntoIterator<Item = Releasability>) -> Releasability;
}
```

`Internal` is the default, so anything unmarked stays in. `combine` taking the most
restrictive input is the rule that stops a report from laundering a restricted track by
aggregation.

Marked types: `TrackView`, `PlanView`, `MissionReport`, `Handoff`, `Anomaly`, and the
snapshot itself.

## 4. Edges

**None.**

## 5. Behaviour

**Marking, increment 3.** Every markable type gains a `releasability: Releasability`
field. Sources supply it: a configured sensor carries a default marking that its
detections inherit; a peer-sourced track inherits the peer's; an operator may raise a
marking on a product but never lower one, and a lowering is a separate authorized action
with its own record.

**Enforcement, increment 4.** At the interface boundary, per caller:

1. The caller's identity establishes its party, from the machine identity D-02 gives it.
2. Every item in a response is filtered by `permits`.
3. **Filtering is by removal, with a count.** The response says how many items were
   withheld. A silently shortened list makes a peer believe they have the whole picture,
   which is worse than telling them they do not.
4. A request for a single item the caller may not receive returns a not-found rather than a
   forbidden, so the existence of a restricted item is not disclosed by the error code.
5. Aggregates and reports are marked by `combine` before filtering, so a report containing
   one restricted track is restricted as a whole.

**Rule 3 and rule 4 pull in opposite directions and both are kept**, deliberately: a
collection response admits that items were withheld, because the peer needs to know their
picture is partial; a single-item request does not, because it would be an oracle for
guessing identifiers.

**Degradation.** When a caller's party cannot be established, it gets `Internal` only,
which is nothing. There is no anonymous peer.

**What is not designed here.** A marking is not a classification system, and this note does
not introduce one. Everything in this repository stays unclassified (AP-04); the marking
governs distribution between deployments, which is a different question from national
classification and must not be conflated with it in the implementation or in the
documentation.

## 6. Configuration and interface delta

`ConfigBaseline` gains `default_releasability: Releasability` for the deployment, plus a
per-sensor and per-peer override. Validation: named parties must appear in the peer table
or the endpoint table.

`gungnir_security::actions` gains `product.release` for raising or lowering a marking.

Interface:

- Every response type gains `releasability` on its items.
- Collection responses gain `withheld: usize`.
- `SnapshotResponse` is filtered per caller.

The field additions are additive; the filtering changes behaviour for an existing endpoint
and is therefore gated on the caller having a party, so an unfiltered deployment sees no
change until it configures parties.

## 7. User-interface delta

| Panel | Change |
|---|---|
| PN-03 Track table | A marking column, filterable |
| PN-04 Track detail | The marking and where it came from |
| PN-13 Reports | The report's combined marking, computed before export, with the inputs that produced it |
| PN-17 Commander summary | What was released to whom in the period |
| PN-20 Audit and accounts | Marking changes with the operator who made them |

## 8. Verification

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-6.6 Releasability | Property tests on `combine` and `permits`, plus a two-node test with parties configured | `combine` always yields the most restrictive input; an unmarked item is `Internal`; a caller with no party receives nothing; a collection response reports the withheld count; a single restricted item returns not-found rather than forbidden; lowering a marking is an authorized action with a record | Generated marking sets; two-node test harness |

The `combine` property is exhaustively testable over the small variant set, and it should
be, because it is the rule an aggregation bug would silently break.

## Traceability

GAP-062; CAP-6.6, CAP-7.4; D-06; depends on DN-16 for peer parties and GAP-041 for the
transport; feeds DN-07 and DN-18; `../gungnir-api-v1.md`;
`../ux/wireframes/WF-13-reports.puml`; principles AP-04, AP-09; contract C-10.

# DN-33 Accepting a coverage gap

Closes GAP-106. Written and built 2026-09-26 on the owner's decision of that day (D-118),
with the details the owner delegated. What the owner has signed of this note is in
[`../signatures.md`](../signatures.md).

## 1. The gap and the thread step it blocks

DN-12 finds the holes along the declared approaches, and PN-11 and the viewport draw them.
Nothing lets a person say that a hole is **known and tolerated** -- argued about, weighed
against what closing it would cost, and taken on for this mission with a reason and a name
against it. So a gap a commander accepted yesterday reads exactly like one nobody has
looked at, and the picture cannot tell a risk somebody took on purpose from one nobody has
noticed. `docs/mission/roles-and-stakeholders.md` §4 gives the decision to the commander
("Accept coverage gap") and nothing in the code could take it; usability task US-16 was
held for round 2 because the control did not exist (GAP-074).

## 2. What the owner decided (D-118)

Taken by the owner, 2026-09-26, directly:

1. **An acceptance names the gap, a reason, and the commander who accepted it.** The reason
   is required.
2. **It holds for the baseline revision and the laydown it was made under, and re-opens by
   itself** when either changes, or when the gap's own shape changes. It has no clock of its
   own.
3. **An accepted gap stays drawn**, on PN-11 and on PN-16, marked accepted. It is never
   hidden.
4. **An accepted gap still counts in the coverage measure.** The measure reports the
   accepted and the unaccepted parts separately; it never subtracts one from the other.
5. **Only the commander may accept** (§4). The row is mapped to a coarse action first, in
   §4 and in `gungnir-security/tests/role_matrix.rs`, and `gungnir-security` follows.
6. **Accepting and re-opening are events on the record**: journaled and audited.

What was left to this note, under the owner's delegation, is §5's definition of a shape
change, §6's reading of "the commander who accepted it", and §8's timing.

## 3. The owning components

| Concern | Crate |
|---|---|
| The acceptance, the gap it names, why it re-opened, and the events | `gungnir-model` (`laydown.rs`, `events.rs`) |
| Whether an acceptance still holds against a coverage report, and the measure | `gungnir-analytics::coverage` (DN-12's crate) |
| The action and who holds it | `gungnir-security` (human-owned) |
| The ledger: accepting, recovering from the journal, re-opening each frame | `gungnir-app::gap_acceptance` |
| The control and the marking | `gungnir-ui` (PN-11, PN-16), `gungnir-viewport3d` (the gap layer) |

`GapSeverity` moves from `gungnir-analytics` to `gungnir-model` and is re-exported where it
was, because an acceptance on the journal names one and the model may not depend on
analytics (AP-06, the move `AzimuthSector` made for the same reason). **This note adds no
dependency edge**: every crate above already reaches the ones it uses.

## 4. Types

In `gungnir-model`:

```rust
/// A coverage gap as an acceptance names it: enough to find the same gap in a later report.
pub struct AcceptedGap {
    /// The approach, by the name the baseline declares -- not by its index, which is only
    /// a position in a list.
    pub approach: String,
    pub severity: GapSeverity,
    /// Along the approach, metres: where the gap starts and ends.
    pub from_m: f64,
    pub to_m: f64,
    /// What the report that found it was computed with (DN-12 §5 rule 4): a gap found
    /// every 500 m and one found every 100 m, or flat and terrain-masked, are different
    /// answers about the same ground.
    pub sample_spacing_m: f64,
    pub terrain_masking_applied: bool,
}

pub struct GapAcceptance {
    pub id: GapAcceptanceId,
    pub gap: AcceptedGap,
    /// Why, in the commander's words. Never empty.
    pub reason: String,
    /// The signed-in operator who accepted it, and the role their session carried.
    pub operator: String,
    pub role: String,
    /// What it holds for: the baseline revision in force, and the laydown marked current
    /// (`None` for a deployment that declares no laydown).
    pub revision: u32,
    pub laydown: Option<LaydownId>,
    pub at: MissionTime,
}

pub enum ReopenedBecause {
    RevisionChanged { from: u32, to: u32 },
    LaydownChanged { from: Option<LaydownId>, to: Option<LaydownId> },
    /// No gap of the accepted shape is on the approach now; `now` is what is there
    /// instead, overlapping the accepted extent -- empty when the stretch is covered.
    ShapeChanged { now: Vec<AcceptedGap> },
    /// Coverage cannot be computed at all any more (no local frame, no approach).
    NotMeasured { reason: String },
}
```

In `gungnir_model::events`, a new `PlanningEvent` on `Event::Planning`:
`GapAccepted(GapAcceptance)` and `GapAcceptanceReopened { acceptance, because, at }`, with
DN-26 amendment 1's `LaydownRehearsed` beside them.

In `gungnir-analytics::coverage`: `accepted_gap(&CoverageGap, approach, &CoverageParameters)`
builds the name, `same_gap` compares two, `standing(acceptance, report, approaches,
revision, laydown)` answers `Holds { gap }` or `Reopens(ReopenedBecause)`, and
`CoverageMeasure` is §7's measure.

## 5. When an acceptance holds, and when it re-opens

An acceptance holds while, **in this order**:

1. the running baseline's `revision` is the one it was made under -- else
   `RevisionChanged`;
2. the laydown marked current is the one it was made under -- else `LaydownChanged`. A
   laydown's content cannot change without the revision changing, so this rule is the
   owner's second condition made explicit rather than a second test of the same thing;
3. **the live coverage report holds a gap of the same shape** -- else `ShapeChanged`, naming
   what is there now.

**The same shape** means: the same approach (by name), the same severity, the same sample
spacing and terrain-masking flag, and each end within **half a sample spacing** of where it
was. The tolerance is not a matter of taste. The sampler places every sample at a multiple
of the spacing along the whole polyline (DN-12 §9, GAP-118), and a gap's ends are samples,
so a real change -- a sensor dropping to standby, a mask loaded, a sector re-aimed -- moves an
end by at least one whole spacing, while computing the same report twice can move it only
by floating-point noise. Half a spacing is the widest tolerance that cannot swallow a
one-sample change, and the narrowest that cannot be defeated by rounding. A different
spacing or masking flag is a different answer (DN-26 §5) and never the same shape.

**A shape change is any change**: a gap that grew, shrank, split, changed severity or
closed. A smaller gap is a different risk from the one accepted, and the commander accepted
a particular risk, not a ceiling on one.

A re-opened acceptance is over. It does not come back if the gap later returns to its old
shape: the commander is asked again, because the record must never show a risk accepted
for a period nobody accepted it in.

## 6. Who may accept

§4's "Accept coverage gap" row reads `yes` for the commander alone. It stands for a new
coarse action, **`coverage.accept_gap`** (`gungnir_security::actions::ACCEPT_COVERAGE_GAP`),
added to §4's mapping and `role_matrix.rs`'s transcription first and then to
`role_permits`. **The administrator does not hold it**: §4's "Roles without a column" gives
the administrator every decision except the engagement chain and escrow, and the owner's
"only the commander" puts risk acceptance with them. Accepting a gap is a command decision
about the mission's risk, which §1 of the same document keeps from the account that
administers the system; §4's paragraph is amended to say so.

**"The commander who accepted it" is a signed-in commander.** An acceptance is refused with
nobody signed in, even when the desktop's selected role is the commander: DN-23 §5 rule 5's
fallback selection is nobody's authority, and an acceptance that named no person would be a
risk nobody took. A decision can be recorded unattributed because its absence of a name is
itself recorded; an acceptance exists *to* carry one.

## 7. The coverage measure

`CoverageMeasure` is DN-12's measure with acceptance reported beside it, never taken out of
it:

| Field | Meaning |
|---|---|
| `segments`, `accepted_segments` | Gap segments on the approaches, and how many of them a standing acceptance names |
| `uncovered_m`, `uncovered_accepted_m` | Approach metres covered by nothing, and of those, how many are accepted |
| `single_sensor_m`, `single_sensor_accepted_m` | The same for single-sensor coverage |

`uncovered_m` is what it was before this note, accepted gaps included; the accepted figure
is a part of it and is shown as such ("1 200 m uncovered, 400 m of it accepted"). PN-01's
strip, PN-11's layer count and PN-16's coverage column all read it.

## 8. Behaviour

1. **PN-11 lists the gaps** of the live report under its layer controls: approach, extent,
   severity, and either the acceptance that stands for it -- who, as which role, when, and
   the reason -- or an accept control. The control takes a reason and is enabled only for a
   signed-in role holding `coverage.accept_gap`; for any other it says who may accept.
2. **Accepting** journals `GapAccepted`, writes an audit entry under `coverage.accept_gap`
   naming the gap, what it holds for and the reason, and adds it to the ledger.
3. **Re-opening is checked every frame while any acceptance stands**, against the same live
   report PN-11 draws -- except **while terrain the baseline names is still loading**: a
   report computed flat while the mask is on its way would re-open every acceptance at
   every start. Once the terrain has loaded, or failed, the check runs, and a failure is a
   shape change like any other (the answer is now flat). A re-opening journals
   `GapAcceptanceReopened` with its reason and writes an audit entry attributed to nobody,
   because nobody did it. PN-11 lists the acceptances re-opened this session with why.
4. **The viewport draws an accepted gap as the gap it is**, uncovered or single-sensor by
   the same pattern and colour, with an "accepted" mark beside it. Hiding the gap layer
   hides accepted and unaccepted alike, and PN-11's hidden-layer sentence still says so.
5. **PN-16** marks the accepted part of a laydown's coverage answer the same way. An
   acceptance holds for the laydown it was made under, so only that row can show one; a
   planner comparing an option sees its gaps unaccepted, which is the truth about an option
   nobody has accepted anything for.
6. **At start** the ledger is folded from the journal: every `GapAccepted` not followed by
   a `GapAcceptanceReopened` stands until the first check says otherwise. A baseline
   applied since, a laydown now current, or sensors that came up in standby re-open it on
   that check, with the reason, rather than silently.
7. **Retention keeps** every session holding a standing acceptance's `GapAccepted`
   (`retention::protected`), so ageing the journal cannot un-accept a gap.

## 9. What this note deliberately does not do

* **No expiry clock.** US-16's wireframe (WF-17) asked for one, and the owner chose
  re-opening on a change instead: an acceptance lasts exactly as long as the situation it
  was made about. A time limit on top would re-open acceptances whose situation had not
  changed.
* **No warning obligation.** The same wireframe attached one; the owner's decision does
  not, and DN-03's warnings are owed to assets, not to gaps. The reason carries any
  undertaking the commander made.
* **No acceptance of an option.** A gap in a laydown that is not in force is a planning
  question, not a risk anybody is running.
* **No acceptance anywhere but the desktop that recorded it.** A node's coverage answer and
  a second desktop's PN-11 do not carry it: **GAP-194**.

## 10. Configuration and interface delta

None in the baseline. The journal gains `Event::Planning`, additive, so `SCHEMA_VERSION`
stands. No route is added (GAP-194).

## 11. User-interface delta

| Panel | Change |
|---|---|
| PN-11 Coverage layer controls | The gap list, each gap's acceptance or its accept control, and the acceptances re-opened this session |
| PN-02 Viewport | An accepted gap drawn as itself, marked "accepted" |
| PN-16 Planning panel | The coverage cell names the accepted segments and metres |
| PN-01 Status strip | The uncovered-segment count says how many are accepted |

## 12. Verification

One row, Draft, in `../verification-capability-table.md` §2:

| Component | Test | Method | Criterion |
|---|---|---|---|
| `gungnir-app` gap acceptance (CAP-1.4) | `gungnir-app/tests/gap_acceptance.rs` | round 1's baseline and laydowns, with a radar placed so the upper Vell approach has a gap; accept as a signed-in commander, refuse an operator; apply a baseline at the next revision; mark another laydown current; drop a radar to standby; restart | the commander's acceptance is journaled and audited and marked on PN-11, the viewport and PN-16's current row; the operator's is refused and nothing is recorded; each of the three changes re-opens it with its reason, journaled and audited; the measure reports the accepted metres inside the uncovered total |

`gungnir-analytics`'s unit tests hold §5's tolerance to one sample either side, and
`gungnir-security/tests/role_matrix.rs` holds §6 against §4.

## Traceability

GAP-106; D-50, D-118; CAP-1.4, CAP-5.9; `../mission/roles-and-stakeholders.md` §4; US-16,
WF-17; PN-11, PN-16, PN-02, PN-01. Related notes: DN-12 (the gaps this annotates), DN-26
(laydowns, and amendment 1's rehearsal advisory beside this), DN-23 (attribution).

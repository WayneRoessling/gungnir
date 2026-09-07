# Architecture change management

Status: first draft, 2026-09-04. Phase H. How an architecture change is requested,
decided, and recorded, and how the value of the architecture is judged.

## 1. What counts as an architecture change

Not every code change. These, and only these, go through this process:

| Change | Example |
|---|---|
| A new or altered principle or contract | Relaxing the one-way dependency rule |
| A new dependency edge | Anything the drawn graph does not show |
| A new crate in the stack | The transport crates, the inference runtime |
| A pass criterion or measure target | Widening a tolerance, moving a latency budget |
| Moving a capability between increments | Deferring the allocator |
| A new subsystem with its own boundary | The assistant, machine-learning inference |
| Anything touching the recommend-versus-act boundary | Always, without exception |
| A deployment profile's behaviour | Changing what a disconnected desktop may decide |

Everything else is ordinary engineering under the contracts.

## 2. The three drivers of change

TOGAF distinguishes simplification, incremental, and re-architecting change. Here they
have concrete forms:

| Driver | Handled as | Example so far |
|---|---|---|
| **Simplification**: less is needed | A change request, usually cheap | Retiring the claim that one graphics device served both rendering and compute |
| **Incremental**: more is needed within the architecture | The gap register and the closure roadmap. The normal case | Eighty-three gaps, seven of them filed by plans 09 and 10 |
| **Re-architecting**: the architecture cannot carry it | A new architecture vision cycle | None yet. The nearest candidate would be autonomous engagement, which is not a change to this architecture but a different product |

Most change arrives as the middle case, which is why the gap register rather than a change
log is the primary instrument.

## 3. The process

1. **Raise.** Anyone, including an agent, raises the change with the reason and the
   contract or principle it touches. An agent that believes a change needs a contract
   broken stops and raises rather than implementing.
2. **Classify.** Simplification, incremental, or re-architecting, and which phase owns it.
3. **Assess.** What it costs, what it unblocks, what it breaks, and which measure would
   show whether it worked.
4. **Decide.** The owner, per the decision rights table. Consulted parties per the same
   table.
5. **Record.** The outcome goes to the ledger that owns that class of fact, and to no
   other: `ARCHITECTURE.md` §10 for technical decisions, the decisions document for
   scoping and policy ones, the business open-questions document for commercial ones, the
   standards document §2.9 for stack additions.
6. **Propagate.** Every document that restated the changed fact is updated in the same
   change, and the citations that pointed at it are re-pointed. This step is where
   document sets rot if it is skipped.

## 4. What has been decided so far

Twenty-nine decisions are on record, with outcomes:

| Set | Range | Where |
|---|---|---|
| Engineering and scoping | D-01 to D-17 | `../../../mission/gap-analysis/decisions-needed.md`, with the technical ones also in `ARCHITECTURE.md` §10 |
| Business | D-B1 to D-B12, four answered | `../../../business/open-questions.md` |

Three of the seventeen created engineering work directly: adopting the eight roles, the
identifier crate, and the display vocabulary. One moved a capability between increments:
fires from increment 4 to increment 3. One changed the shape of a gap without adding an
edge: the home for anomaly detection.

**The scope lock, D-01, is the one to revisit.** Everything ships in one release, which
means nothing ships until everything does. The business plan asks the revenue question
this raises as D-B2, and it is unanswered. That is the single most consequential open
change request in the set, and it is a business decision wearing an architecture
decision's clothes.

## 5. Value realization

An architecture is worth what it changes about outcomes. Four measures, and where each
comes from:

| Question | Measure | State |
|---|---|---|
| Does the picture help the operator decide? | MOE-01 to MOE-05, MOP-01 to MOP-09 | Targets agreed by D-16; no harness (GAP-056) |
| Does the interface help rather than compete for attention? | MOP-37, usability | No session with participants (GAP-074) |
| Does the architecture description stay true to the code? | Compliance findings per assessment | First run today: two findings |
| Does the governance cost less than it saves? | Governance load against engineering load | Judged at each increment boundary; the tailoring is revised, not the principles |

The first two are the ones that matter to a customer, and neither has a number. That is
the honest state of value realization: the architecture is coherent and unmeasured.

## 6. Change requests currently open

| Request | Class | Owner | State |
|---|---|---|---|
| Revisit the scope lock | Simplification, with commercial consequences | Owner | Open, D-B2 |
| Sign off the transport crates | Incremental | Owner | Open, blocks WP-13 |
| Sign off the inference runtime | Incremental | Owner | Open, GAP-077 |
| Sign off a docking crate | Incremental | Owner | Open, GAP-075 |
| Signed configuration baseline as well as signed binaries | Incremental | Owner | Open in the release governance document |
| Whether a customer framework must be mapped alongside TOGAF and UAF | Incremental | Owner | Partly answered: a DoDAF cross-reference exists (`../framework-cross-reference.md`); no customer framework is committed |

## Traceability

`../preliminary/governance-framework.md` for decision rights;
`../../../mission/gap-analysis/decisions-needed.md`;
`../../../business/open-questions.md`; `../../../../ARCHITECTURE.md` §10;
`../phase-g-implementation-governance/compliance-assessment.md`;
`../../../mission/measures.md` for the measures.

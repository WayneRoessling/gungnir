# Architecture vision

Status: first draft, 2026-09-04. Phase A. What the architecture is for, who cares, what
"good" looks like, and what it may not do.

## 1. The problem being solved

Air and counter-uncrewed-aircraft defence has become a problem of many cheap, slow,
low-flying threats arriving together, mixed with a few fast expensive ones, against a
mosaic of sensors that do not talk to each other, in a contested spectrum, in front of an
authority who cannot decide fast enough to keep up. The mission analysis characterizes
it across five domains; the business plan states the three consequences a buyer feels
directly: their sensors do not interoperate, their command-and-control assumes a network
they do not always have, and nothing tells them what it does not know.

Gungnir is a command-and-control application and services layer that fuses that mosaic
into one picture, prioritizes what threatens defended assets, recommends what to do about
it, and requires a human to decide.

## 2. Stakeholders and their concerns

Summarized here; the full mapping with the view that answers each concern is in
[`stakeholder-map.md`](stakeholder-map.md).

| Stakeholder | Principal concern |
|---|---|
| Operator, supervisor, commander | Can I see it, do I believe it, and can I decide in time? |
| Sensor manager | Is my coverage what I think it is, and what did I lose? |
| Analyst, intelligence analyst | Can I reconstruct what happened and defend the conclusion? |
| Planner | Will this laydown cover the assets that matter? |
| Administrator | Who has what authority, and is the record complete? |
| Integrator, peer system | Will it speak to what we already have? |
| Accreditor, auditor | Can you show me the evidence rather than assert it? |
| Buyer | Does it reduce cost per engagement and time to decision? |
| Owner, investor | Is it buildable by this team, and is the position defensible? |

## 3. Business scenarios

The vignettes are the business scenarios, and phase A adopts them rather than writing new
ones. Ten of them, each tied to a mission thread, each rendered as a UAF interaction view
generated from the same source:

| Vignette | Scenario | Thread |
|---|---|---|
| VG-01 | Night raid on the power station | MT-01 |
| VG-02 | Salvo arriving with the raid | MT-02 |
| VG-03 | Quadcopter over the airfield | MT-03 |
| VG-04 | Uncrewed surface vessel attack on the anchorage | MT-04 |
| VG-05 | The afternoon surface picture | MT-05 |
| VG-06 | Battery on the far bank | MT-06 |
| VG-07 | Raid under satellite-navigation denial with a radar loss | MT-07 |
| VG-08 | Who is that aircraft | MT-08 |
| VG-09 | Planning the week's laydown | MT-09 |
| VG-10 | The port cell loses the node | MT-10 |

VG-07 and VG-10 carry disproportionate architectural weight: degradation and disconnection
are where an architecture that only works when everything works falls over.

## 4. The value proposition, architecturally

Five claims that are properties of the architecture rather than features:

1. **One crate set, three deployment profiles.** The disconnected desktop is the same
   software as the cloud node, so the disconnected case is not a degraded afterthought.
2. **Recommendation only, structurally.** Nothing executes without a recorded human
   decision, and no dependency path exists by which the assistant or a model could.
3. **Honest about itself.** Health is reported, never inferred. Unimplemented capability
   returns a named error. Stale data is drawn as stale.
4. **Verified per capability against named oracles**, with the criteria written before
   the code.
5. **Traceable end to end.** Mission thread to capability to activity to service to crate
   to verification row, generated and checked.

The differentiator to lead with is the third, and its most visible expression is that the
project publishes a register of 83 gaps with an owner and a target for each.

## 5. Target architecture, at one level

The summary view is [`../../uaf/summary-and-overview.md`](../../uaf/summary-and-overview.md).
In one paragraph: a tracking and estimation core of pure numerical crates; a canonical
model that owns the shared types and the versioned views and events; two service facades
that combine the core into application-facing contracts; a productization layer of
twenty-five crates for ingest, time, picture, identity, policy, command, security,
observability, resilience, replay, and reporting; a 3D-data pair; two binaries, a desktop
and a headless node; and a user-interface layer that is the only place graphics
dependencies appear. Dependencies run one way through that list.

Three deployment profiles configure the same crates: a disconnected desktop that owns its
own journal, an on-prem node that several desktops share, and a cloud node that adds
scale and, by decision D-14, may carry the live picture to a model provider.

## 6. Scope and constraints

**In scope.** The product and its system-of-systems interfaces. Air defence and
counter-uncrewed-aircraft lead; maritime and land supporting; intelligence and planning
cross-cutting.

**Out of scope.** Enterprise architecture beyond the product. Effector control software.
Sensor firmware. Autonomous engagement.

**Constraints that shape the architecture rather than the schedule:**

| Constraint | Source | Consequence |
|---|---|---|
| Nothing acts without a human decision | AP-01 | The policy and command crates are human-owned and cannot be optimized around |
| Unclassified and openly sourced only | AP-04 | Identification friend or foe is deferred; corpora are synthetic |
| One team, agent-assisted | Capability assessment | Structural verification gates instead of headcount review |
| Must work disconnected | AP-05 | Local journal, store-and-forward, defined reconciliation, local model for the assistant |
| Scope locked to one release | D-01 | Every gap through increment 4 is release content; nothing is dropped at lock |
| United States jurisdiction | D-B1 | Export determination is an open business question, not a settled position |

## 7. What would falsify this vision

Stated so that the vision is a claim and not a wish:

- If the tracking core cannot meet the latency budgets on desktop hardware, the
  single-crate-set claim fails and the profiles diverge.
- If operators cannot decide faster with the recommendation queue than without it, the
  central value claim fails. That is measure MOP-37 and the round-two usability sessions.
- If no customer will buy from a company that publishes its defect register, claim three
  is a liability rather than a differentiator.

None of the three has been tested. That is the honest state of phase A.

## Traceability

`../../../mission/mission-analysis.md`; `../../../mission/vignettes.md`;
`../../../business/business-plan.md` §2 to §4; `../../uaf/summary-and-overview.md`;
`../../../../ARCHITECTURE.md` §7 and §8; `../preliminary/architecture-principles.md`.

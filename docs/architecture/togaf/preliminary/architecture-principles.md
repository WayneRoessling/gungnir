# Architecture principles

Status: first draft 2026-09-04; **signed by the owner 2026-09-05** (plan 10 method
step 1). Five findings were recorded at signature and are listed at the end of this
document; AP-12 and AP-16 in particular are signed as intended rules whose enforcement
does not yet exist.

Seventeen principles in the four TOGAF categories. Most are not new: they are the rules
the workspace already enforces, written in the form the ADM asks for so that a reviewer
can see what the code is defending and why. Each carries the contract in
[`../phase-g-implementation-governance/architecture-contracts.md`](../phase-g-implementation-governance/architecture-contracts.md)
that enforces it; a principle with no contract is an aspiration, and this set has none
of those.

Format per TOGAF: name, statement, rationale, implications.

## Business principles

### AP-01 Recommend, never act

**Statement.** The system recommends. A human decides. No engagement, no effector
command, and no identity declaration that policy reserves to a person is executed
without a recorded human decision.

**Rationale.** It is the product's central claim and its regulatory position. It is also
what makes the recommendation layer safe to build with agents at all: the worst outcome
of a wrong recommendation is a wrong suggestion to someone who can refuse it.

**Implications.** `gungnir-policy` and `gungnir-command` are human-owned crates. The
assistant has no state-changing tool and no dependency path to one
([`../../../ai/safety-boundaries.md`](../../../ai/safety-boundaries.md)). Machine
learning produces evidence, never a decision
([`../../../ml/README.md`](../../../ml/README.md)). An autonomous mode would be a new
product decision, not a configuration flag. Contract C-01.

### AP-02 Honest status

**Statement.** No health flag, connection state, test, or interface element may claim a
subsystem works when it does not.

**Rationale.** A command-and-control system that lies about its own readiness is worse
than one that is plainly incomplete, because the operator's compensation depends on
knowing what is degraded.

**Implications.** `PIPELINE_IMPLEMENTED` is false and gates the tracking service's
health. Unimplemented capability returns a named error variant, not a plausible default.
Absence is displayed, not hidden. Contracts C-02 and C-03.

### AP-03 Every consequential act has a named actor and a time

**Statement.** Decisions, overrides, promotions, releases, and configuration changes are
attributable to a person, a role, and a mission time, and are recoverable from the
record.

**Rationale.** Accreditation, after-action review, and dispute resolution all reduce to
this. It is far cheaper to design in than to retrofit.

**Implications.** The journal is append-only and replayable. The audit log is a security
concern, not a logging convenience. The assistant's answers are audited before they are
displayed. Contract C-04.

### AP-04 Unclassified and openly sourced

**Statement.** Every artefact in this repository is unclassified and derived from public
sources. No controlled specification, proprietary third-party material, or customer data
enters it.

**Rationale.** It keeps the repository shareable with investors, partners, and hires, and
keeps the export position tractable. It is also a constraint the test-track and
interoperability work has already had to respect.

**Implications.** ASTERIX and STANAG corpora are synthetic or vendor-provided samples.
Identification friend or foe is deferred because its key material is controlled (D-09).
Vehicle data carries a source and a confidence mark. Contract C-05.

### AP-05 One product, three profiles

**Statement.** The disconnected desktop, the on-prem node, and the cloud node are
configurations of one crate set, not forks.

**Rationale.** Three code lines would triple the verification burden for a team of one
and would guarantee divergence in exactly the crates that must not diverge.

**Implications.** Profile differences are configuration and wiring, not conditional logic
in the domain crates. A capability that cannot work in the disconnected profile degrades
honestly there rather than being compiled out silently. Contract C-06.

## Data principles

### AP-06 One canonical model, one owning crate per type

**Statement.** `gungnir-core` and `gungnir-coord` own the primitives, `gungnir-model`
owns the canonical views and events, and everything above re-exports rather than
redefines.

**Rationale.** Two definitions of a track identifier become two meanings of a track
identifier, and the divergence surfaces at the worst moment, in fusion or in the journal.

**Implications.** Nine shared types have exactly one definition today and the compliance
assessment checks it. A new shared type goes into the lowest crate that needs it.
Contract C-07.

### AP-07 Provenance travels with the data

**Statement.** Every detection, track, and derived product carries where it came from,
when it was received, and what produced it.

**Rationale.** Fusion, identification, and after-action review are all arguments about
sources. A figure without a source cannot be defended.

**Implications.** The detection view carries source and receipt time. Identification
evidence names its producer, including the model name and version for a
machine-learning source. The assistant states its model and its tools before its answer.
Contract C-08.

### AP-08 The journal is the record, and replay reproduces it

**Statement.** The event journal is the authoritative record of a session, and replaying
it reproduces the same picture.

**Rationale.** Determinism is what makes after-action review, defect reproduction, and
measure computation possible at all.

**Implications.** Envelopes are sequenced and mission-timed. The journal round-trip is a
gate. Reconciliation is defined (mission-time merge, duplicates dropped, conflicts
reported) rather than implicit. Contract C-09.

### AP-09 Releasability is a property of the data

**Statement.** What may be shared with whom is marked on the view, the report, and the
contract, not inferred from which channel carried it.

**Rationale.** Channel-based control fails the moment a product is forwarded. D-06
settled that it is modelled now and enforced at the interface later.

**Implications.** A marking field in increment 3, per-caller enforcement in increment 4
(GAP-062). The assistant's egress policy is checked on the assembled request, not on the
operator's intent. Contract C-10.

## Application principles

### AP-10 Dependency direction is one-way

**Statement.** Tracking core, then the canonical model, then the service facades, then
the productization crates, then deployment, then the user interface. Nothing points back.

**Rationale.** It is what keeps the numerical core testable in isolation and keeps an
interface concern from reaching into a filter.

**Implications.** No edge may exist that `ARCHITECTURE.md` does not draw. A task that
seems to need a back-edge is a misplaced abstraction and stops for a human. The 3D-data
crates sit between the productization crates and the user interface; the graph in
`ARCHITECTURE.md` §7.1 is authoritative where the standards prose is looser. Contract
C-11.

### AP-11 Traits define the verifiable surface

**Statement.** Every capability the verification table names corresponds to a trait, so
one differential or property harness covers every implementation of it.

**Rationale.** A team this size cannot afford a test suite per concrete type.

**Implications.** New capability arrives as a trait first. The provider abstractions for
the assistant and for machine-learning inference follow the same rule, which is what lets
a fake provider run the whole loop in continuous integration. Contract C-12.

### AP-12 Not-yet-implemented is an explicit error

**Statement.** A capability that does not exist returns a named error variant on any path
a user can reach. `todo!()` is reserved for code nothing calls.

**Rationale.** It is AP-02 expressed in the type system, and it is the reason a scaffold
of this size is safe to publish at all.

**Implications.** Seventeen not-implemented variants and thirty-two `todo!()` bodies
today. The assessment records that nothing yet proves the second set is unreachable
(finding CA-F2). Contract C-03.

### AP-13 Binaries wire, they do not decide

**Statement.** The desktop and node binaries construct, configure, and connect. Domain
logic lives in a library crate with tests.

**Rationale.** Logic in a binary is logic without a unit test and without a second
consumer to keep it honest.

**Implications.** The reviewer checklist asks this question of every change to either
binary. The entry point is one of the three places `unwrap()` is allowed, which is only
tolerable because nothing else happens there. Contract C-13.

## Technology principles

### AP-14 The stack is pinned, small, and signed off

**Statement.** A dependency enters the workspace only by being recorded in
`agentic-coding-standards.md` §2.9, with a stated purpose and consumers, and pinned in
one place.

**Rationale.** Supply chain is the largest attack surface a small team owns, and version
skew across fifty crates is the failure mode that costs the most to unpick.

**Implications.** Twenty dependencies today, every one recorded. Four sign-offs are
pending and each is named: the transport crates, the identifier crate, a docking crate,
and the inference runtime. Licence, advisory, and source policy are enforced by
`cargo deny`. Contract C-14.

### AP-15 The two graphics contexts stay separate

**Statement.** The viewport renders through OpenGL and the compute path uses `wgpu`; they
are pinned so that enabling one never yields two versions of the other.

**Rationale.** It is a real constraint found in the stack analysis, not a preference, and
forgetting it produces a link-time failure that is expensive to diagnose.

**Implications.** The application shell runs on `glow`; `wgpu` is compute-only and pinned
to 22. A change to either is a stack change under AP-14. Contract C-15.

### AP-16 Verification gates are structural and cannot be waived

**Statement.** The oracle, property, Miri, loom, fuzz, and benchmark gates are conditions
of merge, not advice, and neither an implementing nor a reviewing agent can waive one.

**Rationale.** Plausible-looking numerical code is the specific failure mode that agent
assistance introduces. Structural gates are the answer to it.

**Implications.** Human sign-off is mandatory for the low-trust tier. The gates exist as
workflow files and run once the repository is hosted; until then the equivalent commands
run locally and the change description says so. Contract C-16.

### AP-17 A measure has an agreed target before it has a test

**Statement.** Pass criteria and measure targets are agreed by the owner and then
implemented. A criterion is never widened to make a test pass.

**Rationale.** A criterion that moves is not a criterion. This is the single rule most
likely to be eroded quietly under delivery pressure.

**Implications.** D-16 set roughly forty targets before the harnesses exist. The
second-section verification rows stay draft until their criteria are agreed (GAP-067).
Changing a criterion is a change request under phase H, not an edit. Contract C-17.

## Traceability

| Principle | Primary source | Contract |
|---|---|---|
| AP-01 | `../../../agentic-workflow.md` low-trust tier; CAP-4.3 | C-01 |
| AP-02 | `../../../../CONTRIBUTING.md`; `../../../../CLAUDE.md` | C-02, C-03 |
| AP-03 | CAP-6.3; `../../../mission/roles-and-stakeholders.md` §4 | C-04 |
| AP-04 | `../../../plans/README.md` conventions; D-09 | C-05 |
| AP-05 | `../../../../ARCHITECTURE.md` §8 | C-06 |
| AP-06 | `../../../agentic-coding-standards.md` §1.2 | C-07 |
| AP-07 | `../../../../ARCHITECTURE.md` §10 item 5; CAP-1.2 | C-08 |
| AP-08 | CAP-5.2; D-03 | C-09 |
| AP-09 | D-06; CAP-6.6 | C-10 |
| AP-10 | `../../../agentic-coding-standards.md` §1.1; `../../../../ARCHITECTURE.md` §7.1 | C-11 |
| AP-11 | `../../../agentic-coding-standards.md` §1.3 | C-12 |
| AP-12 | `../../../agentic-coding-standards.md` §3.1 | C-03 |
| AP-13 | `../../../../ARCHITECTURE.md` §7.3; the reviewer checklist | C-13 |
| AP-14 | `../../../agentic-coding-standards.md` §2.9; `../../../release-governance.md` | C-14 |
| AP-15 | `../../../../ARCHITECTURE.md` §9 | C-15 |
| AP-16 | `../../../agentic-workflow.md` verification stack | C-16 |
| AP-17 | `../../../verification-capability-table.md`; D-16 | C-17 |

## Signed 2026-09-05, with findings

All seventeen principles and all seventeen contracts were signed by the owner on
2026-09-05, in the four TOGAF batches, after each batch was checked against what the
code actually does rather than against what the document asserts. Five findings were
recorded at signature rather than resolved first, because a signature that hid them
would be worth less than one that names them.

| Finding | Affects | State |
|---|---|---|
| C-01 and C-04 have no running automated check | AP-01 recommend-never-act; AP-03 named actor and time | GAP-039 and GAP-059 are both Open. Until they land, the two contracts guarding the recommend-versus-act boundary and the audit trail rest on the reviewer checklist alone. |
| The journal's durability bound | AP-08 the journal is the record | Signed knowing D-04's desktop buffering means a hard kill can lose up to 5 s of journal (`../../../../ARCHITECTURE.md` §10 items 19 and 32). Replay still reproduces what was written; the bound is on what reaches the disk, not on fidelity. |
| `todo!()` reachability is asserted, not proven | AP-12, C-03 | 22 `todo!()` calls remain in the workspace and GAP-082 is Open. The contract asks for "grep plus a reachability argument"; the grep exists, the argument does not. AP-12 is signed as the intended rule, not as a demonstrated state. |
| Two capabilities are free functions, not traits | AP-11, C-12 | `gungnir_association::solve_assignment` (the Hungarian/Jonker-Volgenant row) and `gungnir_metrics::compute_metrics` (the metrics row) are named by verification-table rows but are not behind traits. Association has `Associator`; metrics has no trait. Recorded as two accepted exceptions rather than adding trait surface for callers that do not need it. |
| AP-16 has no enforcement mechanism | AP-16, C-16 | The workspace is not under version control, so there are no pull requests and the CI workflows under `.github/workflows/` have never run. "Conditions of merge" describes machinery that does not exist yet; the gates are run by hand and said so in the change description. The principle is signed; the enforcement is recorded as absent. |

Two contracts were **run against the tree on 2026-09-05** as part of the signing, rather
than taken on trust:

- **C-07** (a shared type has exactly one definition): passes. `TrackId`, `TrackStatus`,
  `ResourceId`, `SessionId`, `SensorId` and `MissionTime` are each defined once.
- **C-11** (no dependency edge that `ARCHITECTURE.md` does not draw): passes. 43 crates
  carrying edges, zero undrawn, including the `gungnir-metrics` to `gungnir-association`
  edge accepted the same day.

Running C-11 also produced evidence for GAP-081: a naive manifest walk against the
drawn graph fails twice on the document's own notation -- the prose "Both facades" in the
edge table, and inline parenthetical annotations inside the §7.1 tree. Automating these
checks is real work, not a formality.

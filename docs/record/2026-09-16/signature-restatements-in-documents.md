# Signature restatements removed from documents on 2026-09-16

The passages below stated that the owner had signed something, or that something still waited for a signature. They were removed from the current-state documents named in each heading when `docs/signatures.md` became the only place that says what is signed, and are kept here verbatim.

## docs/mission/gap-analysis/data/gaps.yaml

Sentences from the current-state fields (closing action and evidence), as they were;
each gap's history entries were not changed.

GAP-004, `action`:

```text
Human-owned crate touched (`gungnir-ingest`), reviewed and **signed by the owner 2026-09-08** (`../../../ARCHITECTURE.md` §10 item 106).
```

GAP-009, `action`:

```text
, so not human-owned work and not signed.
```

GAP-085, `action`:

```text
**Closed 2026-09-05.** The owner signed off the change the same day.
```

GAP-085, `action`:

```text
needed the owner's sign-off, which it had.
```

GAP-042, `action`:

```text
Closed. DN-03 amendment 3 awaits the owner's signature.
```

GAP-047, `action`:

```text
in `gungnir-command` (human-owned), signed by the owner 2026-09-06.
```

GAP-057, `action`:

```text
Signed 2026-09-06 (`assign_role`, `save`, the `ASSIGN_ROLE` action).
```

GAP-057, `action`:

```text
Built 2026-09-08 (human-owned; signed by the owner 2026-09-08, `ARCHITECTURE.md` §10 item 104):
```

GAP-057, `action`:

```text
Signed by the owner 2026-09-10 (`ARCHITECTURE.md` §10 item 125), after the review found and closed
```

GAP-057, `action`:

```text
every item this entry names is built and signed (2026-09-06, 2026-09-08 and 2026-09-10, `../../../ARCHITECTURE.md` §10 items 104 and 125),
```

GAP-060, `action`:

```text
Built 2026-09-08 (human-owned; signed by the owner 2026-09-08, `ARCHITECTURE.md` §10 item 111):
```

GAP-060, `action`:

```text
Remaining: the owner's review and signature (the same shape GAP-057 is in), and, recorded rather than tracked as a task, the standing design question
```

GAP-084, `action`:

```text
(1) **Done**: the owner's signature, on both halves together (
```

GAP-084, `action`:

```text
the `gungnir-config` reshape), landed 2026-09-10 (`ARCHITECTURE.md` §10 item 127).
```

GAP-084, `action`:

```text
**The wildcard half is done 2026-09-15 and signed by the owner the same day**
```

GAP-065, `action`:

```text
are human-owned and signed by the owner the same day; the outbox
```

GAP-065, `action`:

```text
(amendment 1, signed 2026-09-07)
```

GAP-090, `action`:

```text
The `gungnir-policy` change from this entry is **signed by the owner 2026-09-15** (`../../../ARCHITECTURE.md` §10 item 130), so nothing of this gap's own half is now unsigned;
```

GAP-096, `evidence`:

```text
(`PipelineSnapshot`; written and gated, not signed);
```

GAP-096, `action`:

```text
(5) **Done 2026-09-08, signed by the owner the same day**:
```

GAP-100, `action`:

```text
against the primary text and signed them 2026-09-09 (`ARCHITECTURE.md` §10 item 114);
```

GAP-100, `action`:

```text
Item (1) was signed 2026-09-09;
```

GAP-101, `action`:

```text
against the primary text and signed them 2026-09-09 (`ARCHITECTURE.md` §10 item 123),
```

GAP-103, `action`:

```text
**Signed by the owner 2026-09-09**, the same day he took D-43, so nothing remains: the `gungnir-association` change is human-owned as a numerical stability guarantee (`docs/agentic-workflow.md`) and the signature covers the contract, its tests and the reconciled fuzz postcondition.
```

## docs/architecture.md

```text
**GAP-067's walk done and confirmed by the owner, 2026-09-07.** D-16 agreed every §2
criterion on 2026-09-04; what remained was the owner confirming, row by row, that the
test each row cites actually checks that criterion. That walk is done: every row below
still reading "Tested" has been checked against `verification-capability-table.md` §2 and
is now **Specified** without its wording being rewritten cell by cell, because "Tested"
already named the same citation this walk confirmed.
```

```text
Linear KF **gated and signed off 2026-09-05** (
```

```text
and the particle filter statistically over 40 trials. **Signed by the owner 2026-09-06.** |
```

```text
**gated 2026-09-05, assignment signed off the same day** (
```

```text
because Stone Soup 1.9.1 has no MHT hypothesiser. **Signed by the owner 2026-09-06.** |
```

```text
**Gated 2026-09-05, cumulative confirmation rule signed off the same day** (
```

```text
 **The PHD is signed by the owner 2026-09-06; the three that are not built are not in that signature.** |
```

```text
Implemented 2026-09-06 (GAP-011) and **signed by the owner** the same day.
```

```text
publishes the residual spread of its own fit. **Signed by the owner 2026-09-06.** |
```

```text
Implemented 2026-09-06 (GAP-029) and **signed by the owner** the same day.
```

```text
2026-09-06, **signed by the owner the same day**: the TLS identity issued from the key provider through rcgen (D-29), and the ASTERIX adapter's `FeedStatsSink` (GAP-001), which sits in the human-owned gateway and was put to the owner separately once it was noticed. |
```

```text
(GAP-002, 2026-09-06, signed by the owner the same day). |
```

```text
added 2026-09-06 (GAP-057, GAP-059), signed by the owner the same day. 2026-09-06, **signed by the owner the same day**: `SecurityOfficer`, the passphrase-sealed `PersistentKeyProvider`, and `LocalAccountAuthority::with_lifetime`; later the same day, **signed**: `FileAccountStore::assign_role`
```

```text
GAP-002, GAP-004, GAP-040, 2026-09-06, signed by the owner the same day). |
```

## docs/gungnir-capabilities.md

```text
The owner's review before signing the CPHD (2026-09-09) found the
```

```text
**PHD implemented and gated 2026-09-06, CPHD implemented and gated 2026-09-08 and signed 2026-09-09 after a review correction** (§1 above); the LMB filter is built and gated but not signed (`gungnir-rfs/src/lib.rs` module doc) and the full delta-GLMB remains
```

```text
(DN-27; GAP-096, the snapshot change signed 2026-09-09)
```

```text
(GAP-100, signed 2026-09-09)
```

```text
(GAP-101, signed 2026-09-09)
```

```text
(GAP-099, signed 2026-09-09)
```

## docs/plans/README.md

```text
; targets and maturity levels await the owner |
```

```text
; operational and strategic content awaits the owner, resource content the engineering reviewer;
```

```text
; principles and personas await the owner and reviewers;
```

```text
**Principles and contracts signed by the owner 2026-09-05**, in four TOGAF batches, each checked against the code first; five findings recorded at signature rather than resolved before it,
```

## docs/verification-capability-table.md

```text
Measured, not asserted, pending the owner's confirmation |
```

## docs/mission/capabilities/README.md

```text
Status: first draft 2026-09-04; statements, measures, and maturity targets await
the owner's confirmation (open items in each file).
```

## docs/mission/roles-and-stakeholders.md

```text
(GAP-065; confirmed by the owner 2026-09-08, amended the same day to add Supervisor alongside the Product release row above)
```

## docs/mission/gap-analysis/tools/gen_gaps.py

```text
         "design notes in `../../design/`. That set is signed and reviewed: the owner signed\n"
         "all five human-owned notes, the engineering reviewer accepted the five new\n"
         "dependency edges, and a domain reviewer checked each note against the mission\n"
         "thread step it names. Full design coverage means a reviewed component is named for\n"
```

## docs/gungnir-api-v1.md

```text
the transport crates
were signed off on 2026-09-05 (D-18, `agentic-coding-standards.md` §2.9) and are in the
```

## docs/architecture/uaf/model/elements.yaml

```text
GAP-060's engineering item was built and signed 2026-09-08.
```

## docs/architecture/uaf/README.md

```text
the operational and strategic content inherits the first-draft status of plans 02
and 04 and awaits the owner's review; the resource content awaits engineering
review.
```

## docs/architecture/togaf/README.md

```text
Status: first draft 2026-09-04. The seventeen principles and seventeen contracts were
**signed by the owner on 2026-09-05**, with five findings recorded at signature (see the
findings table in `preliminary/architecture-principles.md`); C-07 and C-11 were run
against the tree that day and both passed. The governance framework is owner-approved
content. Phases B, C, and D await their reviewers.
```

## docs/architecture/togaf/preliminary/architecture-principles.md

```text
Status: first draft 2026-09-04; **signed by the owner 2026-09-05** (plan 10 method
step 1). Five findings were recorded at signature and are listed at the end of this
document; AP-12 and AP-16 in particular are signed as intended rules whose enforcement
does not yet exist.
```

```text
## Signed 2026-09-05, with findings
```

```text
All seventeen principles and all seventeen contracts were signed by the owner on
2026-09-05, in the four TOGAF batches, after each batch was checked against what the
code actually does rather than against what the document asserts. Five findings were
recorded at signature rather than resolved first,
```

## docs/architecture/togaf/phase-g-implementation-governance/architecture-contracts.md

```text
Status: first draft 2026-09-04; **signed by the owner 2026-09-05.** Phase G.
```

## docs/architecture/togaf/phase-a-vision/statement-of-architecture-work.md

```text
| The principles are written and never signed |
```

## docs/architecture/togaf/phase-c-information-systems/application-architecture.md

```text
Restated from the status the repository already carries, not re-judged here:

| Group | Status |
|---|---|
| Productization layer, 25 crates | Implemented and tested |
| The canonical model, eventing, journal, configuration, session lifecycle | Implemented and tested |
| Tracking mathematics, association, lifecycle, random finite sets, fusion, allocation | **Scaffold.** Trait surfaces with `todo!()` or not-implemented bodies; the pipeline flag is false |
| Intercept planning | **Scaffold.** Returns not-implemented |
| Interoperability codecs | **Scaffold.** ASTERIX and STANAG return not-implemented |
| Interface transport | **Scaffold.** The contract is written, the crates are not signed off |
| User interface | Track table, intercept panel, health, alerts, and a 2D viewport are implemented; the 3D scene and the decision panels are designed and not built |
| Both binaries | Start and run without sensors |
```

## docs/agentic-workflow.md

```text
**Low-trust. Human-owned; agents may draft but not merge unsupervised:**

```

```text
  - Signed off so far: `gungnir_association::solve_assignment`'s output contract
    (2026-09-09, D-43 and GAP-103). The first signature under this clause, and worth
    recording as the precedent for where its edge is.
```

```text
decide the edge unilaterally. **What the owner decided
    was the contract, not the fix**: the three answers the register framed were
    mutually exclusive statements about what the function promises, and he chose a
    fourth (`total_cost` became `Option<f64>`).
```

```text
  - Signed off so far: the `PolicyChain` lifetime parameter (2026-09-05, GAP-038), which
    is a type-level change that let the chain hold the borrowing engines the crate
    already shipped. Worth recording as the precedent for where this rule's edge is: the
    rule names *verdict logic*,
```

```text
  - Also signed off: wiring `gungnir_command::queue` into `InMemoryApprovalWorkflow`
    (2026-09-05, GAP-034 and GAP-035). This one
```

```text
  - Also signed off: conforming `OperatorDecision` to DN-10 §3 and the expiry rule in
    `gungnir-collab`'s arbiter (2026-09-05), together with DN-10 amendment 1. This one
```

```text
  - **Signed off 2026-09-05: the finding that GAP-057 cannot be implemented yet**, and
    only that. An agent picking up authentication
```

```text
    - **What was signed is the blocker, not the answer.** D-20 stays open, the crates are
      not chosen, and DN-23 is an unsigned draft. Nothing was implemented.
```

## docs/README.md

```text
raised the same day out of the gaps they block, both signed and implemented;
```

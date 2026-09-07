# Agentic development workflow

This document describes how AI coding agents are used in the Gungnir workspace, what
they are trusted to do unsupervised, and what verification gates stand between
agent-written code and `main`. It exists so a reviewer or adopter can see the guardrails
directly rather than take correctness on faith. It applies to every crate in the
workspace; the coding rules themselves live in `agentic-coding-standards.md` (tracking
core, service layer, productization layer) and `rust-ui-architecture-coding-standards.md`
(UI, rendering, and 3D-data crates). `../CONTRIBUTING.md` is the short form of both.

## Risk triage: where agents fit

**High-leverage, low-risk. Agents may work with light review:**

- Scaffolding trait boilerplate across motion models (CV, CA, CT) once the core `State`
  and `Measurement` traits are human-designed.
- Property-based test and edge-case generation for association and assignment
  algorithms.
- Differential-test harness plumbing (`gungnir-oracle`) against reference
  implementations (`filterpy`, `motpy`, Stone Soup, textbook closed-form solutions).
- Docs, examples, and tutorials, including keeping doc comments and the `docs/` set in
  agreement.
- Dependency upgrades within the pinned set, clippy and lint cleanup, benchmark-report
  generation.
- The async simulation harness (fake sensor streams at different rates): mostly
  plumbing, not correctness-critical.
- egui panel bodies in `gungnir-ui` that read existing `AppState` fields, and file
  loaders in `gungnir-data` that convert a third-party type into an internal one.
- Productization-layer trait scaffolds, in-memory reference implementations, and
  their unit tests, where the trait is already specified in `gungnir-capabilities.md`
  §5.

**Medium-risk. Agent-assisted, mandatory verification gate before merge:**

- Kalman, EKF, UKF, and IMM math, and the assignment algorithms (Hungarian/JV, JPDA,
  MHT).
- The Bellman/DP resource-allocation solver (`gungnir-allocation`).
- The CPU ICP reference and the GPU registration pipeline in `gungnir-data-fusion`;
  the GPU path is gated on agreement with the CPU reference.
- The three-d scene and render loop in `gungnir-viewport3d`, gated on the frame-budget
  and no-per-frame-allocation rules in `rust-ui-architecture-coding-standards.md` §5.
- Parsers in `gungnir-ingest` adapters and `gungnir-interop` codecs, gated on the
  fuzz targets.
- The on-disk journal format in `gungnir-store`, gated on the round-trip test.
- Reconciliation in `gungnir-resilience` and arbitration in `gungnir-collab`, gated on
  their invariant tests, because they decide what the shared record contains.

**Low-trust. Human-owned; agents may draft but not merge unsupervised:**

- Any `unsafe` block, anywhere in the workspace.
- Concurrency correctness in `gungnir-fusion-async`: lock-free structures, channel
  backpressure, out-of-order measurement handling.
- Numerical stability guarantees: covariance must stay positive semi-definite; no silent
  NaN propagation.
- The recommend-versus-act boundary: `gungnir-policy` verdict logic, geofence and
  authority enforcement, and the `gungnir-command` approval workflow. The system must
  never execute an intercept without a recorded human decision, and no agent-written
  change may weaken that.
  - Signed off so far: the `PolicyChain` lifetime parameter (2026-09-05, GAP-038), which
    is a type-level change that let the chain hold the borrowing engines the crate
    already shipped. Worth recording as the precedent for where this rule's edge is: the
    rule names *verdict logic*, the change touched none, and it was still brought to the
    owner because the crate is on this list. Bring the borderline ones; do not decide
    the edge unilaterally.
  - Also signed off: wiring `gungnir_command::queue` into `InMemoryApprovalWorkflow`
    (2026-09-05, GAP-034 and GAP-035). This one *did* change behaviour -- the escalation
    bound and the roles an item is offered to -- in both cases to match DN-10 §5, which
    the uncalled code did not. Both corrections and the trait additions were put to the
    owner together with what they changed and why.
  - Also signed off: conforming `OperatorDecision` to DN-10 §3 and the expiry rule in
    `gungnir-collab`'s arbiter (2026-09-05), together with DN-10 amendment 1. This one
    was not an agent proposing a change to a signed design; it was an agent finding that
    the code had **lost two of the four variants the signed design specified**, and
    conforming to it.
  - A pattern behind all three: **connecting code nobody calls is where these crates'
    latent defects live**, because a module that is fully tested and entirely uncalled is
    self-consistent rather than verified. Read the design note against the code, not the
    code against its own tests -- and read the whole of the relevant section before
    saying what it decided.
- `gungnir-security` authentication, authorization, and audit logging, and the
  validation, authentication, and quarantine gateway in `gungnir-ingest`, because those
  are the trust boundaries for operators and for external data.
  - **Answered by the owner 2026-09-06: `gungnir-remote`'s identity path is on this
    list.** GAP-060 moved the certificate and key-custody path out of `gungnir-node` and
    into `gungnir-remote`, which the list did not name; it was raised rather than folded
    into that day's signature, per the precedent note in the `gungnir-policy` entry, and
    the owner added it.

    **Scoped to the path, not the crate.** What is human-owned is
    `gungnir-remote/src/identity.rs`, and `LinkTls` with `client_config` in that crate's
    `lib.rs`: the code that issues a host's TLS identity from its `KeyProvider` and decides
    what identity it presents. The rest of `gungnir-remote` -- the peer link, the node
    link, the endpoint client -- is ordinary transport work and stays outside, because a
    list that swallowed the whole crate would make routine work need a signature and the
    signatures would stop meaning anything.
  - **Signed off 2026-09-05: the finding that GAP-057 cannot be implemented yet**, and
    only that. An agent picking up authentication found `Authenticator` to be a trait
    with no implementor and **no cryptographic crate anywhere in the workspace** --
    passphrase verification and token integrity each need one, and D-02 chose the
    mechanism without choosing the libraries. That is the same shape as D-18 before the
    transport, so it was raised as D-20 rather than answered in a pull request.
    - **What was signed is the blocker, not the answer.** D-20 stays open, the crates are
      not chosen, and DN-23 is an unsigned draft. Nothing was implemented.
    - Worth recording as the precedent for this crate's edge: the tempting move was a
      session model without credential verification, which would have produced a
      signed-in operator nobody had checked and filled
      `DecisionRecord::operator_id` and `Concurrence` with names the system had not
      verified. That is worse than the `None` and `UnattributedRole` those types carry
      today, and the honesty of both was the point of GAP-038 and GAP-005. **When a
      security gap can only be half-built, the half that produces attribution is the
      wrong half to build first.**
- `gungnir-api` write endpoints, for the same reason.
- Any change to a pass criterion in `verification-capability-table.md` (see
  `agentic-coding-standards.md` §6 rule 3).

## The verification stack

"Tests pass" is not sufficient for numerics and real-time code. An agent can write
plausible-looking filter code that is subtly wrong (for example the wrong Joseph-form
covariance update). The gates are structural:

1. **Differential testing against a trusted oracle** (`gungnir-oracle`, workflow
   `oracle-diff.yml`): the same scenario through a hand-verified reference, numerical
   agreement asserted within the tolerance in `verification-capability-table.md`.
2. **Property-based invariant testing** (`gungnir-testkit`, `proptest`, runs inside
   `cargo test`): posterior covariance stays PSD, track IDs are never reused while
   active, assignment solutions respect the constraint matrix. Agents write the test
   given an invariant; a human authors the invariant.
3. **`cargo miri`** (workflow `miri.yml`) on any PR touching `unsafe`.
4. **`loom`** (workflow `loom.yml`) on `gungnir-fusion-async`: exhaustive interleaving,
   run against the real Scenario 3 async pipeline rather than a synthetic stress
   harness.
5. **Fuzzing** (`gungnir-fuzz`, workflow `fuzz-nightly.yml`) on the sensor-ingestion
   parser and association cost-matrix construction.
6. **Benchmark regression gate** (workflow `bench-regression.yml`): `criterion` versus
   the baseline stored from `main`, hard-fail beyond a p99 threshold.

Two checks apply beyond the tracking core and are not numbered gates:

- GPU point-cloud registration is validated against the pure-CPU reference on
  GPU-enabled runners (`gpu-fusion.yml`), never inside plain `cargo test`
  (`rust-3d-data-ecosystem-build-vs-adopt.md` §3.6). **Dormant, manual dispatch only,
  since 2026-09-07**: the GPU path and its tests do not exist yet (the `gpu-tests`
  feature is empty), and neither does the runner; both are GAP-024. The workflow fails
  a run that executed zero tests, so it cannot pass on a runner alone.
- End-to-end scenario replay through the service facades (detections in, UI-consumable
  state out) catches integration faults no single-crate oracle test can
  (`ARCHITECTURE.md` §6).

The non-core layers have draft pass criteria in `verification-capability-table.md`
§2; a row becomes a gate when its criterion is agreed and its test lands.

### Status of the gates

The workflow files exist under `.github/workflows/` (`gungnir-workspace-structure.md`).
They run on GitHub since 2026-09-07 (D-10 as amended). `gpu-fusion.yml` is the
exception: it runs only on manual dispatch until GAP-024 delivers the GPU path, its
tests, and a self-hosted runner labelled `gpu`.

## Review pipeline

- **Implementer agent** writes the change against a human-authored spec, and against
  the capability-table row before writing code (`agentic-coding-standards.md` §6).
- **Reviewer agent** makes a separate pass with a fixed checklist, explicitly prompted
  to be adversarial rather than confirmatory:
  - numerical stability, error handling, test coverage, doc comments, no unexplained
    `unwrap()`;
  - no dependency edge added that `ARCHITECTURE.md` and `agentic-coding-standards.md`
    §1.1 do not already imply, and no crate added to the stack without
    `agentic-coding-standards.md` §2.9;
  - for UI, rendering, and 3D-data crates, the checklist in
    `rust-ui-architecture-coding-standards.md` §10;
  - for `gungnir-app` and `gungnir-node`, that the change is wiring and not logic;
  - that no health flag, connection state, or test claims a subsystem works when it
    does not (`../CONTRIBUTING.md`, "No fake wiring");
  - for any policy, command, security, or ingest-gateway change, that it is flagged for
    human ownership rather than approved.
- **Verifier gate**: the checks above. Non-negotiable; cannot be waived by either agent.
- **Human sign-off**: required for anything in the low-trust tier. The PR must cite
  *why* an approach was used (for example "Joseph form used here instead of the simpler
  update because it is numerically stable under repeated updates; see Bar-Shalom
  §5.3"), not just that it passes. For `gungnir-fusion-async` the PR must state in
  plain language what interleaving the change could affect
  (`agentic-coding-standards.md` §5).

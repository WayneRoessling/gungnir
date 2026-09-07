# Sc-Pr Security processes

**UAF definition.** The security processes view shows the sequences of steps that
implement security controls.

**Purpose here.** The three processes accreditors ask about: how a caller is
authenticated and authorized, how a decision is gated and audited, and how a
release is assured. Each names the code that performs the step or the gap that will.

Status: first draft, 2026-09-04.

## Authentication and authorization of an API call (SV-20, SV-21, SV-23)

1. The transport presents the caller's credential (mutual TLS for machines, a
   session token for operators; D-02).
2. `Authenticator` resolves it to an `OperatorId` or returns
   `SecurityError::AuthenticationFailed` (trait today; GAP-057).
3. The handler names its action (`picture.view`, `detection.submit`, `plan.decide`,
   `plan.override`); `Authorizer::require(operator, action)` passes or returns
   `Forbidden(action)`, which the API maps to `ApiError::Security`.
4. The handler runs; gated actions write an `AuditEntry` (GAP-059).

On the desktop the same steps run in-process with the local account; the
disconnected profile has no network caller.

## Decision gating and audit (SV-15, SV-16, SV-22)

1. A `PlanView` is proposed by SV-02.
2. `PolicyChain::evaluate` consults every engine; a `Denied { reason }` stops the
   plan; otherwise `RequiresHumanApproval` (the built-in engines never return
   `Approved`; a configured pre-delegation under D-15 may, and is still recorded).
3. `ApprovalWorkflow::request` opens a pending approval; `CommandEvent::ApprovalRequested`
   is published.
4. A decider with `plan.decide` (or `plan.override` for an override) calls
   `decide`; a `DecisionRecord` is written; `CommandEvent::Decided` is published; an
   `AuditEntry` is written (GAP-059).
5. Only a plan with an accepting `DecisionRecord` becomes actionable; the
   integration test and review checklist of GAP-039 prove there is no other path.
6. Expiry and escalation (GAP-034) are recorded as decisions of their own kind.

## Release assurance (CAP-6.5)

1. `cargo deny check` on licences, advisories, bans, and sources on every change
   (`deny.toml`).
2. The release workflow builds reproducibly, runs the gates, produces the SBOM, and
   signs the artefacts (`../../../release-governance.md`); exercised once the
   repository is hosted on GitHub (D-10 as amended 2026-09-07, GAP-061).
3. Advisories: critical within 7 days, high within 30 (D-10 follow-up).
4. Model artefacts (plan 09) are signed and promoted through SV-19 with validation
   evidence.

## Elements used

- SV-02, SV-15, SV-16, SV-19, SV-20 to SV-23; IE-04, IE-16, IE-18 to IE-20;
  CAP-4.3, CAP-6.1 to CAP-6.3, CAP-6.5.

## Notes

- Steps marked with a gap are design; everything else is implemented and tested in
  the named crate, though the binaries do not yet call it (GAP-028).

## Traceability

- Derives from: `gungnir-security`, `gungnir-policy`, `gungnir-command`,
  `gungnir-api`; `../../../release-governance.md`; D-02, D-10, D-15.
- Feeds: the security and command verification rows (MOP-31, MOP-38, MOP-39,
  MOP-41), plan 10 security architecture.

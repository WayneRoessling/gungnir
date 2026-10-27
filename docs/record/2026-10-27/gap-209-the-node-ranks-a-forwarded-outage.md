# GAP-209 the node ranks a forwarded outage by its own accounts

Plan 12 tranche B, the second build. The review's §5 finding 2: reconciliation arbitration
ranks the role a decision was taken under, as the record states it, and a forwarded outage
carries that role to the node. On a disconnected desktop that record is the operator's own
journal. So a forged or edited journal could win a decision conflict on reconnect, and the
node would publish the settlement it claimed. A replayed batch was caught by the
request-id key; a fabricated one was not.

## The owner's four choices, 2026-10-27

Each was the recommended default.

| Question | Chosen |
|---|---|
| A forwarded decision by an operator the node has no account for | Recorded with no role; a conflict over it goes to a person |
| A rule settlement the node's own ranks do not give | The batch is refused whole |
| The desktop's own reconciliation | Unchanged: it ranks and shows its recorded roles, and the node is the check |
| A conflict settled by a person | The resolver's account here must hold `plan.decide` |

## What was built

- **`gungnir-security` (human-owned).** `Role::named`, the one parser for the role names
  records carry. The desktop's reconciliation now calls it too, so the node and the
  desktop cannot read one name two ways.
- **`gungnir-api` (human-owned write path).** `CallerAuthority::role_of` reads an
  operator's role from the node's account store, with no credential. `NodeApi::role_of`
  answers `Known`, `Unknown` or `Unavailable`. `ForwardRefused` gains `RoleMismatch`,
  `ArbitratedOtherwise`, `ResolverNotPermitted` and `CannotVouch`.
- **`gungnir-node`.** Before a forwarded batch is compared with the record or written, it
  is vouched for:
  - each decision's role is re-derived from the node's accounts: a match is recorded, a
    mismatch refuses the batch, and an unknown operator is recorded with no role;
  - each rule settlement is re-run on the node's ranks, using the node's own record of
    the plan where it holds one;
  - each person settlement's resolver must hold `plan.decide` here.

  A refusal is the same whole-batch refusal a contradiction is: audited, and nothing
  applied.

## Why the derivation comes before the contradiction check

The node compares a forwarded record with what it already holds under the same identifier.
If the role were rewritten after that check, a batch forwarded a second time would carry
the desktop's role and contradict the node's own copy. Deriving first means the same batch
always becomes the same records, so a retry is `already_held`.

## Tests

`gungnir-node/tests/forwarded_roles.rs` runs over the real route against a loop running the
node's own approval steps. It covers:

- a role the operator's account here does not carry, refused and on the audit record;
- an honest role recorded as it is, and an unknown operator recorded with no role;
- the same batch again, held rather than contradicted;
- a rule settlement whose remote side is a Commander here, where the desktop said it was an
  Operator, refused; and one the node's ranks agree with, taken;
- a conflict settled by an Analyst, refused.

The existing outage tests on both binaries (`approval_queue`, `cut_off_and_reconnected`,
`failover`, `failover_e2e`) pass unchanged.

## What this does not do

- **The desktop's own verdict is not corrected locally.** A desktop that arbitrated on a
  forged role finds out when the node refuses its batch, and a person settles it on PN-18.
  The desktop cannot ask the node while it is cut off, which is when it arbitrates.
- **An operator unknown to the node is not refused.** Their decision is on the record with
  no role, so it can never win a ranked conflict. A deployment that wants every desktop
  operator known to the node provisions them there.
- **Human-owned standing.** `gungnir-security` and the `gungnir-api` write path are
  human-owned. What the owner has signed of this change is in `../../signatures.md`.

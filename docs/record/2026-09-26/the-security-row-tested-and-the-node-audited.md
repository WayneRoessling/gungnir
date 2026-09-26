# The security row tested and the node audited

GAP-111 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
D-87 and D-88 ([`../../mission/gap-analysis/data/decisions.yaml`](../../mission/gap-analysis/data/decisions.yaml)),
DN-23 §13. Human-owned: `gungnir-security`, and `gungnir-api`'s write paths; what the owner
has reviewed is in [`../../signatures.md`](../../signatures.md). Built on 2026-09-25 under the
owner's delegation of that day.

## What was wrong

The GAP-067 walk held the `gungnir-security` verification row for three reasons: nothing
compared the role matrix in code with the authority matrix it is meant to encode, nothing
checked that an audited act leaves one entry rather than at least one, and the node audited
nothing but its queue -- in memory. The desktop's log was in memory as well, so its record of
who did what lasted exactly as long as its window.

## The matrix, and what comparing it found

`gungnir-security/tests/role_matrix.rs` holds `mission/roles-and-stakeholders.md` §4 as data,
reads the document to check the transcription, turns each cell into a grant by the rule §4 now
states in words, and asks `role_permits` about every role and every action constant --
`REQUIREMENT` and `ASSIGN_ROLE` included, and every constant in the module, which the test reads
from the source so one added later cannot be missed.

Run against the code as it was, it found thirteen disagreements. D-88 settles them:

- **The supervisor could not set weapons control status.** §4, §1 and §2 all say it does; the
  arm left the action out from the day the action was added, the same unrecorded narrowing the
  product-release row was on 2026-09-08. Code corrected.
- **The administrator held the engagement chain and escrow recovery.** `Role::Administrator =>
  true` meant an account made to manage accounts could accept an engagement, override one, set
  weapons control status, and recover a sealed journal's key -- and it topped the escalation
  ladder, so an item nobody below decided was offered to it. §1 and §2 say the administrator is
  not a decision-maker in the engagement chain, the layout already withheld the decision dialog,
  and §4's escrow row says the security officer alone, which is the separation of duties D-30
  made that role for. Code corrected: everything but those four actions.
- **Three actions were held by nobody who does them.** `requirement.state`, `review.conduct`
  and `handover.acknowledge` were the administrator's alone, while §1, DN-11, DN-20 and DN-21
  give the work to the intelligence analyst and the sensor manager, the analyst, and whoever
  takes the watch. Nothing checks them yet, so nothing was refused in practice; the matrix now
  says what the next checker must enforce.
- **`actions::ALL` left out `requirement.state` and `account.assign_role`**, so a baseline naming
  either was refused as a misspelling.

Where the code's grants were long-standing -- viewing the picture, submitting a detection,
overriding, reporting, a sensor manager applying a calibration baseline -- §4 was the side
without a row, and gained one. The reconciliation row follows D-53, which leaves a conflict to
"a person permitted `plan.decide`", and §2 no longer says the operator overrides: the code, GAP-127
and DN-31 §9 row 4 have all held that it does not.

One grant is wider than its cell and was not narrowed: the sensor manager's `config.apply`
applies a whole baseline, control status and authority rules included. Withholding it would take
the calibration baselines §2 gives the role, so the answer is a per-section check, filed as
GAP-162 -- with the finding that `ConfigEditorState::apply` checks no permission at all.

## The desktop, act by act

`gungnir-app/tests/audit_one_entry_per_act.rs` signs in as four roles in turn and performs every
act that writes an audit entry -- a refused sign-in and a sign-in, a decision, a sensor command, a
launch warning, an acknowledgement, an effector report, a baseline applied, a requirement stated,
tasked, declined and satisfied, the five acts of a review, a role assigned, a sign-out -- and
asserts after each that the log grew by exactly one entry naming it and naming the operator. A
third test reads the sources and counts the call sites, so a new one fails until it is an act
here. Resolving a reconciliation conflict needs an outage and a reconnection, so
`gungnir-app/tests/failover.rs`'s own test now counts that one.

## The node

D-87 chose what a node records and how. Every sign-in attempt, every refusal on every route, and
every act a role-gated route performs, with its outcome -- a sensor task and what the registry
said, a report or acknowledgement keyed in or sent by a machine, a publish to exchange -- beside
the queue's decisions and refusals DN-31 already recorded. Each names the operator the token
verified and the machine the handshake verified, which since D-67 is a desktop's own key; never
a claimed operator. Served reads are not recorded one by one: a desktop polls, and the session
reading was recorded when it signed in. Refused reads are.

The handler never touches the disk. It puts its entry in an outbox on `NodeApi` with two
rate-limited lanes, one for entries a verified party stands behind and a much smaller one for
entries nobody does, and the loop drains it once a tick into the log and syncs once. What a lane
turns away is counted and the count is an entry, so a flood from outside is bounded in memory
and on disk, is still on the record as a number, and cannot crowd out an entry a person is owed.
`gungnir-node/tests/node_audit.rs` shows both: each act over the real transport grows the log by
one, each served read by none, and a flood three times the unauthenticated burst is recorded or
counted to the last request while a supervisor's task in the middle of it still is.

**Building it found the picture ungated.** The snapshot, history, stream, coverage and exchange
routes served any valid token, so a security officer's session read the picture D-30 says the
role does not see; the API contract had said `picture.view` all along. Each route now asks it.
Health stays open to any operator, since the security officer's layout is health and the audit
record, and the contract's health row said `picture.view` wrongly and is corrected.

## The record on disk

`gungnir_security::FileAuditLog`, the same in both binaries: one JSON line per entry, each
carrying the hash of the line before it and its own over the entry's exact text, one segment per
run under `<data dir>/audit/`. An edited, removed, inserted or torn line is found where it is, and
a log opened over a damaged segment says so in its first entry. It is not sealed, because an
investigation needs the audit record most when the journal's key is gone, and it holds no
passphrase or token, which the node's test and the provisioning test both check by reading the
files. The account-provisioning command records each change beside the store it changes, opening
the log before it changes anything so a change that could not be recorded is not made.

**What it does not do yet** is GAP-163: nothing ages the segments out, a cut tail still
verifies because nothing outside the file holds the head, and PN-20 shows one run.

## Left for the owner

The `gungnir-security` row in `../../verification-capability-table.md` §2 is unchanged: its three
clauses now have the tests above, and the row is for the owner's walk. The authority changes of
D-88 and the node's audit are in human-owned code.

# How the signature ledger was built, and what it leaves out

Until 2026-09-16 a signature was written wherever it applied: the §10 item, the gap's own
text, the crate's module documentation, the design note's status line, the standards
document. Any one of those copies left saying "not signed" kept reading as open work after
the signature landed, which the record found five separate times before this date.
`docs/signatures.yaml`, rendered into `docs/signatures.md`, is now the only place that
says what the owner has signed, and `docs/tools/signatures.py check` refuses a sentence
outside the history that says something still waits for the owner.

## How it was built

One candidate row per signing sentence in the record on `main` at `6374649`: the §10 items,
`docs/agentic-workflow.md`, the §2.9 sign-offs, the design notes and their index, the
dependency edges, the gap register's text, the decision outcomes, and commit messages. A
row's `commit` is the first commit on `main` whose tree carried the written record, found
by searching history for the signing sentence; a signature from before the repository
existed names the first commit. Every path in every row was checked to exist at its
commit, and `check` repeats that on every run.

That gave 169 candidates. 163 are in the ledger.

## The six left out

Each is left out because a later record contradicts it, or because no record names the
owner or a file. None is a finding that the event did not happen; each is a question.

1. **D-29's provider-issued TLS identity for the node, 2026-09-06** (§10 items 88 and 91).
   Items 126 and 127, the design notes' index, and GAP-060's own text call that day's act
   a trust-tier classification, not a code review. The review and signature of
   2026-09-10 (item 126) is in the ledger.
2. **GAP-060's certificate and key-custody path, 2026-09-06** (item 99), for the same
   reason.
3. **The GAP-067 verification walk as confirmed on 2026-09-07**, which
   `docs/architecture.md` and commit `61bcbfc` recorded. Items 131 and 132 (2026-09-15)
   say thirty-four rows still wait for their own sitting, and the walk sheet's
   owner-confirmation column is empty. The five rows settled with the owner on 2026-09-15
   are in the ledger.
4. **Plan 02's mission content approved by the mission subject-matter expert, 2026-09-05**
   (item 37). No record names the owner as that expert, and the mission documents still
   show their reviewer rows as pending.
5. **Plan 07's vehicle data approved by the same expert, 2026-09-05** (item 37), for the
   same reason. The owner's approval of the three domain tables on 2026-09-07 (GAP-046) is
   in the ledger.
6. **The escrow record written at sign-in, 2026-09-06.** Only the design notes' index
   names it, among other signed things, and no record names its file.

## Rows kept on wording a reviewer may want to confirm

Each of these is in the ledger because a source calls it a sign-off, a confirmation or an
approval by the owner, but the wording is thinner than for a code signature: option B for
the 2026-09-05 breaking change; MOP-25 confirmed in the outstanding sign-offs walk; the
2026-09-05 finding that GAP-057 could not be built yet, which has no path; the owner's
acceptance of the six unmaintained-crate advisories; the Publish-to-coalition-exchange
role row; the doc-comment backtick in `gungnir-security/src/authz.rs`, recorded only in two
commit messages; and the recommendations from the review in pull request 114, recorded
only in the message of the commit that applied them.

DN-26 has two rows, a signature on 2026-09-06 and the owner's confirmation on 2026-09-07
that the signature was theirs, because the first appeared on disk with nobody present to
vouch for it. The `loom` model checks have no row of their own for 2026-09-15: GAP-061's
text says they were signed that day, and §10 item 130 says they were covered by the
signatures of items 115 and 128. The ledger follows item 130.

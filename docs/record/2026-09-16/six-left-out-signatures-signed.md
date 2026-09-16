# The owner signs the six signatures the ledger left out

When the signature ledger was built on 2026-09-16 (`signature-ledger.md` in this
directory), six candidate signatures were left out, because a later record contradicted
each one or no record named the owner. The owner read that list the same day and signed
all six. They are the last six entries of `docs/signatures.yaml`.

## How they are recorded

Each entry is dated 2026-09-16, the date of the signature that nobody disputes. Its
subject names the earlier record it settles and the record that disputed it. Its commit
is the first commit on `main` whose tree carried that earlier record, so its paths are the
state the earlier record described, and every path is checked at that commit. The ledger
is append-only and runs oldest first, so an entry could not be inserted at the date an
earlier record claimed.

| Signed 2026-09-16 | The earlier record | What had disputed it |
|---|---|---|
| D-29's node TLS identity, issued through the key provider | §10 items 88 and 91: signed 2026-09-06 | Items 126 and 127, the design notes' index and GAP-060: a trust-tier classification, not a code review |
| GAP-060's certificate and key-custody path | §10 item 99: signed 2026-09-06 | Item 126 and GAP-060 |
| GAP-067's §2 walk | `docs/architecture.md` and commit `61bcbfc`: confirmed 2026-09-07 | Items 131 and 132 (2026-09-15): thirty-four rows still waited |
| Plan 02's mission content | Item 37: approved by the mission subject-matter expert, 2026-09-05 | No record named the owner as that expert, and the mission documents showed the review pending |
| Plan 07's vehicle data | Item 37, the same | The same |
| The escrow record written at sign-in | The design notes' index: signed 2026-09-06 | No record named its file. It is `gungnir-app/src/keystore.rs`, which the same sentence of the index calls "the keystore code" |

## What this settles, and what it leaves

The six disputes are settled in favour of the earlier records. Where a later record said a
signature had not happened, the owner's signature of this date stands over it.

The verification table's GAP-067 walk sheet, written 2026-09-15, still has an empty
owner-confirmation column. This signature does not fill it: the sheet records the
2026-09-15 sittings row by row, and a change to that document is the owner's own step.

The mission documents said the subject-matter review was pending in three places. Those
passages now point at the ledger, and they read as follows before this change.

`docs/mission/README.md`:

```text
Status: first draft 2026-09-04; subject-matter validation pending (see the
validation record in `mission-analysis.md`).
```

`docs/mission/mission-analysis.md`, its status line:

```text
Status: first draft, 2026-09-04. Subject-matter validation pending; see §11.
```

`docs/mission/mission-analysis.md`, three rows of the validation record:

```text
| pending | Subject-matter reviewer (air defense) | `air-defense-and-counter-uas.md`, MT-01 to MT-03, VG-01 to VG-03 | |
| pending | Subject-matter reviewer (maritime) | `maritime.md`, MT-04, MT-05, VG-04, VG-05 | |
| pending | Subject-matter reviewer (land and fires) | `land.md`, MT-06, VG-06 | |
```

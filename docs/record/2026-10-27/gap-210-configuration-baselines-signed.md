# GAP-210: configuration baselines signed

Plan 12 tranche B, the third build. The review's §5 finding 3: a baseline carries the
authority rules, the sensor allow-list, the exchange agreements and the escrow key, and
nothing signed it. `KeyPurpose::BaselineSigning` had been declared since DN-22, and its only
references were the purpose-code tables. `ARCHITECTURE.md` §8.5 lists signed baselines as a
node need.

## The owner's four choices, 2026-10-27

Each was the recommended default.

| Question | Chosen |
|---|---|
| What is signed | A detached signature over the parsed baseline, beside the file |
| Where the verifying key comes from | A pinned PEM file outside the baseline (`GUNGNIR_BASELINE_KEY`) |
| What is refused | Refuse in the connected profiles when a key is pinned; warn when none is, and always on a disconnected desktop |
| How it is signed and recorded | A CLI in both binaries; the signer's fingerprint on the `config.apply` entry |

## What was built

- **`gungnir-security` (human-owned).** `baseline_signing.rs`:
  - `BaselineSigner` provisions the key, sealed in its own keystore file and never
    replaced, then unlocks it and signs;
  - `BaselineVerifyingKey` reads the pinned public half;
  - `BaselineTrust` judges a signature and `admit` applies DN-22 §15's table.

  `provisioning.rs` adds `baseline-key provision` and the key half of `baseline sign`. Both
  read the passphrase from standard input.
- **`gungnir-config`.** `ConfigBaseline::signing_bytes`, a fixed label and then the
  compact serialization, and `BaselineSignature`, the `.sig` file beside the baseline.
- **`gungnir-node`.** `src/baseline.rs` loads, validates and admits a baseline as a
  connected profile. The binary refuses before it opens a journal or binds a port, and
  gains the two commands.
- **`gungnir-app`.** `src/baseline.rs` does the same, with connected meaning the baseline
  declares the Remote backend. A warning becomes a status alert. PN-14's apply judges the
  candidate by the signature beside its file before anything is written, and the
  `config.apply` entry names what was applied: signed by which key and version, unsigned,
  or unverified.

## Why the parsed baseline and not the file

PN-14's apply writes the baseline back pretty-printed, so a signature over the bytes on disk
would break the first time a signed file was applied. The parsed baseline's compact
serialization survives that and changes with any change of content. No baseline type holds
a hash map, so the order of its fields is fixed by the types. A test in `gungnir-config`
checks that the bytes survive a rewrite and change with an edit.

## A defect found while testing

The first run of the node's test pinned the public key exactly as `baseline-key provision`
printed it, and the node refused it as not a key. The printed output ends with a second
newline, and the PEM parser rejects anything after the closing line. A deployment saving
that output to a file would have hit the same refusal. `BaselineVerifyingKey::from_pem` now
trims whitespace around the PEM.

## Tests

- `gungnir-security`: the signer never replaces a key, refuses a wrong passphrase, and the
  rule's table holds row by row; a signature by another key is named, not tried.
- `gungnir-node/tests/baseline_signing.rs` drives the binary:
  - the key is provisioned, and a second provision is refused;
  - an unsigned baseline with a key pinned stops the node before its journal exists;
  - `baseline sign` with the passphrase admits the file as signed by that key;
  - a wrong passphrase signs nothing;
  - an edit after signing is refused;
  - no pin loads and says so, and a pin that does not read is refused.
- `gungnir-app/tests/baseline_signing.rs`:
  - a node's console refuses an unsigned candidate and writes nothing;
  - the same candidate, signed, applies with the signer on the audit entry;
  - an edit after signing is refused;
  - a desktop on its own applies the unsigned candidate, with an alert and an audit entry
    saying so.

## What this does not do

- **A desktop on its own runs whatever it is given.** That is what GAP-210 asked for: the
  alert and the audit entry are what it offers.
- **The signing key's holder can sign anything.** It belongs where baselines are authored,
  not on the hosts that run them.
- **Binaries are not signed by this**, and whether a release ships a signed baseline is
  still `../../release-governance.md`'s open question.
- **Human-owned standing.** `gungnir-security` is human-owned. What the owner has signed of
  this change is in `../../signatures.md`.

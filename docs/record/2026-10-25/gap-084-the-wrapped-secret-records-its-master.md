# GAP-084: the wrapped secret records its master-key version

D-48, the owner's decision of 2026-09-15, settled the last code item GAP-084 had: the cloud
custody profile's wrapped secret must record the master-key version it was wrapped under,
and every unwrap must ask for that version rather than the latest. This item records the
build. `gungnir-security` is human-owned; its standing is in `../../signatures.md`.

## The failure it closes

Azure Key Vault rotates a key by minting a new version. Before this change,
`azure_call` unwrapped with an empty version, which Key Vault reads as "the latest". A
secret wrapped under version 1 does not open under version 2, so the first restart after
a routine rotation would have left the node unable to open its own keystore. The node
would have refused to start with "did not open", and the cause would have been an
ordinary operation followed exactly as Microsoft documents it. The SDK's own
documentation for `unwrap_key` warns about the same thing.

AWS KMS has no such exposure. It rotates the backing material under one key identifier,
and its ciphertext already names the backing key that opens it.

## What changed

All in `gungnir-security/src/managed_service.rs`, plus two call sites in
`gungnir-security/tests/managed_service_cloud.rs`.

- **The trait.** `CloudKeyService::wrap` returns a `WrappedSecret` (the blob and an
  optional key version). `unwrap_key` takes the version to open under and returns an
  `UnwrappedSecret` (the secret and the version that opened it). `UnwrappedSecret` has no
  `Debug`, since it holds the string that unlocks the keystore.
- **The file.** `keystore.kms-wrapped` now begins with `GUNGNIR-KMS-WRAPPED/1`, then a
  `key-version: <v>` line, then the blob. An empty version means the service names none,
  which is the AWS case. A version containing a line break is refused, not written.
- **Opening a file.**
  - A file with the header is opened under the version it records.
  - A file without the header was written before D-48. It opens under the latest version,
    exactly as before. It is then rewritten, beside-and-rename, to record the version the
    service reports, so the next rotation does not strand it. If that rewrite fails, the
    start continues with a warning: the secret is already open, and the rewrite is tried
    again at the next start.
  - A file that begins with the header but does not parse is refused as damaged, without
    calling the service and without replacing the file. Guessing that it was a pre-D-48
    file would hand the service a blob with the header still on it, and turn a nameable
    fault into "did not open".
- **Azure.** The version comes from the `kid` the vault returns, read with the SDK's own
  `ResourceId` parser. A wrap whose response names no version is refused rather than
  recorded as versionless, because that is the exact secret a later rotation would strand.
  The unwrap passes the recorded version, or the empty string only for a pre-D-48 file.
- **AWS.** Returns no version, either way, and its comment says why.

A first start still costs exactly one service call, and a start that opens a headered file
still costs one. `the_service_is_never_called_on_a_seal` is unchanged and still passes.

## How it was tested

The fake `CloudKeyService` now versions its master key the way Key Vault does. A wrap uses
the latest version and names it, an unwrap uses the version it is given or the latest when
given none, and `rotate_master` adds a version. Five new tests:

- A keystore wrapped before a master-key rotation still opens after it. The test first
  shows that an unwrap under the latest version fails after the rotation, so the fake
  really models the exposure.
- A pre-D-48 file opens, is rewritten to record `v1`, and then survives a rotation.
- A damaged header is refused by name, the file is unchanged, and the service is not
  called.
- The file format round-trips with and without a version, and refuses a version it cannot
  record and a header with no blob.
- `azure_key_version` reads the version from Key Vault's documented `kid` form, and
  returns none for an identifier without one.

A mutation check, run once by hand: making the open path ignore the recorded version
made the first two tests fail and nothing else, and they passed again once it was
restored. The existing corrupted-secret test now flips a byte inside the blob of a
headered file, and still sees "did not open" with the file left unchanged.

`cargo test -p gungnir-security`: 106 unit tests and every integration test pass.
`cargo clippy -p gungnir-security --all-targets` and `-p gungnir-node` report nothing from
this change.

## What is not verified

No call has been made to a real AWS KMS or Azure Key Vault, and no real key has been
rotated. The `#[ignore]`d Azure test in `managed_service_cloud.rs` now also checks that
the vault names a version on wrap, and that an unwrap under that version opens and reports
it. It runs with `GUNGNIR_TEST_AZURE_VAULT` and `GUNGNIR_TEST_AZURE_KEY` set, as before.
The AWS test checks that no version is named. GAP-084 stays In progress on that one item:
a real cloud round trip.

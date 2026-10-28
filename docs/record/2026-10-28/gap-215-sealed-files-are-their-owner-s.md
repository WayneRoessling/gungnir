# GAP-215: sealed files are their owner's

Plan 12 tranche B, the eighth and last build, for the review's §5 finding 10.
- The keystore, the sealed account store and the cloud-wrapped secret took their
  directory's permissions.
- Three secrets stayed in memory after use.
- A desktop with no baseline kept its journal wherever it was started from.

## Decisions taken under the owner's delegation of 2026-10-28

The owner asked for GAP-212 to GAP-215 to be built and merged, and to review and sign
afterwards. DN-22 §17 sets out the choices; in short:

- **Windows permissions by `icacls.exe`, not the Win32 API.** The API needs `unsafe`, which
  GAP-238 is moving the workspace to forbid, and a wrapping crate would be a new §2.9 row.
  The tools are run by their full paths under `%SystemRoot%`.
- **`zeroize` admitted (D-145).** It is already compiled into every build, so no crate is
  added. The rejected alternatives were hand-written clearing, which the compiler may
  remove, and `secrecy`, a second crate for what `Zeroizing` already does.
- **The data directory moves only where nothing is lost.** A desktop with no baseline uses
  the user's profile. An existing `./gungnir-journal` is kept and warned about, and a
  baseline file's `data_dir` is left alone. **This is a change to where a fresh desktop
  keeps its journal**, so the owner's review should look at it first.

## What was built

- **`gungnir-security` (human-owned).**
  - `owner_only.rs`: write beside, restrict, rename. Mode 0600 on Unix; on Windows, an
    access list naming the current user alone with inheritance removed. A restriction that
    fails is logged and returned, never a refused write.
  - The keystore, the sealed account store, the cloud-wrapped secret, the account file
    PN-20 saves, and the provisioning command's account file are all written through it.
  - `account add` now says which restriction was applied, where it said Windows applied
    none.
  - The wrapping key and the token key are `Zeroizing`.
- **`gungnir-remote`.** `Credential` zeroizes its passphrase in `Drop`.
- **`gungnir-app`.** `per_user_data_dir`, used when no baseline file is configured.
- **The stack.** `zeroize` in `[workspace.dependencies]` with its §2.9 row, and D-145.
- **Documents.**
  - The deployment README's data directory and account sections;
  - DN-23's sentence about Windows permissions;
  - the run instructions in `CLAUDE.md`.

## Tests

- **`owner_only` on Windows.** A written file has one access entry, full control, nothing
  inherited. It keeps that when rewritten, and nothing is left beside it. On Unix it is
  mode 0600.
- **A real sealed keystore** is owner-only from its first key.
- **`gungnir-app/tests/data_dir_default.rs`.** Profile directories on both platforms; an
  existing journal kept; a host with no profile keeping the old place.

## A test the Miri gate could not finish

This pull request's diff contains the word `unsafe`, in comments that say why the Win32
API was not used, so the Miri gate ran for the first time since GAP-015 added
`gungnir-tracking-service`'s `a_dense_scan_puts_a_group_on_the_view_and_it_ages_stale_with_nothing_arriving`.
- The test runs the dense-group pipeline and waits ten seconds of wall time for the group.
- Under Miri that pipeline takes hours: `gungnir-fusion-async`'s `dense_group` suite was
  already left out at over five hours.
- So the test failed at its wait, not on undefined behaviour.

It is now in the tracking-service entry's `skip` list with that reason, under D-92's rule
for long runs. The dense-group path is therefore not exercised by Miri at all. It contains
no `unsafe` of its own.

## What this does not do

- **Owner-only keeps out other ordinary accounts.** An administrator or root can still read
  the file.
- **Copies of a passphrase made on the way to the wire are not cleared**, such as the
  sign-in request's own body.
- **The node's default data directory is unchanged.** It runs as a service, and its
  container names one.
- **Human-owned standing.** What the owner has signed of this change is in
  `../../signatures.md`.

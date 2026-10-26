# GAP-204: the desktop provisions its own accounts

Plan 12 tranche A, the second build. The review found that the only writer of an
account file was `gungnir-node account add`, and the release ships the node for Linux
and the desktop for Windows, so a laptop in the field ran with nobody signed in: every
decision then recorded no role, and D-03's arbitration could not rank it on reconnect.

## What was built

- **The commands moved down.** `gungnir-node/src/account.rs` became
  `gungnir-security/src/provisioning.rs`, unchanged in what it does. A binary may not
  depend on a binary (`ARCHITECTURE.md` §7.1), and `gungnir-security` is the crate that
  owns the account stores and that both binaries already depend on; it depends on no
  workspace crate, so the one thing the module took from `gungnir-time`, the wall-clock
  second for the audit entry, is read from `std::time` directly, which is what
  `WallClockAuthority` reads.
- **One implementation, told its name.** `provisioning::run(binary, args)` speaks the
  usage and the examples as the binary that was asked, so a desktop operator is never told
  to run the node. The node's `main.rs` calls it as `gungnir-node`; the desktop's as
  `gungnir-app`, before the state is built, so the command opens no window, no journal and
  no baseline.
- **Tests on both binaries.** The node's nine provisioning tests are unchanged and pass
  against the moved module. `gungnir-app/tests/account_provisioning.rs` drives the desktop
  binary: an account it writes signs in through `LocalAccountAuthority`, the usage names
  the desktop and writes nothing, a second account for the same operator is refused
  without `--replace`, and an empty passphrase writes nothing.
- **The deployment README** gains the provisioning section it never had; `CLAUDE.md` names
  both binaries.

## What this does not do

- The passphrase's echo is still not suppressed, for the reason the module states:
  suppressing it needs a terminal crate the workspace has not admitted under §2.9. Pipe
  it in. Filing that crate is a decision, not a build.
- Nothing changes in what an account is or how a session is established (DN-23).
- `gungnir-security` is human-owned; what the owner has signed of this change is in
  `../../signatures.md`.
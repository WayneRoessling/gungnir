# GAP-212: sessions end, follow the store, and can be revoked

Plan 12 tranche B, the fifth build: five of the review's §5 findings on one token path.
The node's MAC key was its environment secret. A token kept the role it was minted under
until it expired. Nothing revoked one, and expiry was compared on a clock that can be set
back. The desktop's session never ended by default and never locked, and the sign-in
back-off was keyed by operator alone.

## What was decided, and by whom

D-143, taken by the owner on 2026-10-26, set the numbers: twelve hours and fifteen minutes,
both configurable downward only, and a locked screen that keeps the picture and refuses
every action. The owner asked on 2026-10-28 for GAP-212 to GAP-215 to be built and merged,
and to review and sign afterwards. The design choices below were therefore **taken under
the owner's delegation of 2026-10-28**, each to the gap's own stated action; DN-23 §17
lists them. None is a signature.

## What was built

- **`gungnir-security` (human-owned).**
  - The issuer derives its MAC key by HKDF (`from_secret`).
  - Every token carries a random identifier. The issuer holds a registry of what it
    issued on a monotonic clock, refuses anything not in it, and enforces the lifetime and
    idle timeout there.
  - `revoke` and `revoke_operator` remove tokens.
  - `LocalAccountAuthority` has a twelve-hour default, an idle lock, a `touch`, and the
    `SessionState::Locked` state.
  - `ClockStepWatch` detects a clock stepping back, and `clock.stepped_back` is its audit
    event.
- **`gungnir-api` (the human-owned write path).**
  - `AccountTokenAuthority::verify` re-reads the role from the store.
  - The back-off is keyed by address and operator, and by address.
  - `DELETE /v3/session` and `POST /v3/session/revoke`.
  - A confirmation's token is ended once read.
- **`gungnir-config`.** `idle_timeout_s`, and both values refused above D-143's ceiling.
- **`gungnir-node`.** The derived key and the idle timeout, and the clock-step audit entry
  in the loop.
- **`gungnir-app` and `gungnir-ui`.**
  - `AppState::may` is the lock-aware check the desktop's authorization sites now ask.
  - `is_locked` and `touch_session`.
  - Input counts as activity.
  - Every action but a sign-in or a selection is dropped while locked, and a locked
    window is drawn over the workspace.
  - PN-01 and PN-20 say the session is locked.

## Why a registry

A monotonic reading means nothing outside the process that took it, so expiry against a
monotonic clock means only the issuing process can judge a token. Holding what was issued
gives that, and the same structure gives revocation and ends sessions at a restart. A
desktop already signs in again when its token lapses, so the restart costs a sign-in.

## Why the desktop's checks moved behind one helper

A lock that fell back to "nobody signed in" would hand the console to the selected role,
which DN-23 §5 rule 5 lets act unauthenticated. So a locked session keeps its role, for
the layout, and loses its authority, and that needs every check to ask about the lock.
The desktop's authorization sites asked `role_permits(state.role(), ...)` in a dozen
places. They now ask `state.may(...)`, which refuses everything while locked. The sites
that read the signed-in session refuse already, because a locked session is not signed
in. `main.rs` drops actions at the one place every action passes, as a second line.

## A decision that went to a new gap

The gap's action named a key-provider purpose for the MAC key, with HKDF as the bridge. A
provider would need a MAC operation, which none has. The trait exposes no key bytes by
design, and its signature schemes are asymmetric. The bridge is built; the provider half is
GAP-243, filed now.

## Tests

- `gungnir-security`, on a hand-moved monotonic clock:
  - an idle session locks, and only a sign-in unlocks it;
  - a set-back clock extends neither a session nor a token;
  - idle and revoked tokens are refused;
  - a restarted issuer believes nothing it did not issue;
  - the key is derived and a short secret is refused.
- `gungnir-api/tests/session_tokens.rs`, over the routes:
  - promotion, demotion and removal take effect at the next request;
  - a removed account's token stays refused when the account returns;
  - sign-out, and revocation by an administrator only;
  - a stranger's failures do not slow the operator's own address.
- `gungnir-app/tests/session_lock.rs`: a desktop locks, keeps its role, refuses four
  actions and a baseline apply, ignores pointer movement, and unlocks on sign-in. The
  configuration's and the enforcement's D-143 numbers are held equal.
- `gungnir-config`: a longer lifetime or idle timeout is refused.

## What this does not do

- **A node's idle timeout measures requests**, which a linked desktop keeps making. The
  operator's idleness is the desktop's lock.
- **Only the node audits a clock stepping back.**
- **The MAC key is not yet in a key provider** (GAP-243).
- **Human-owned standing.** What the owner has signed of this change is in
  `../../signatures.md`.

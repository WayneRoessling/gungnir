# A sign-in during an outage no longer ends it

GAP-143 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
found while GAP-134 was being built and fixed straight after it, as the owner asked.

## The defect

On a desktop configured for a node, a sign-in is what establishes the link (DN-23 §5).
`gungnir-app/src/session.rs`'s `connect_if_remote` did that the same way whatever state the
desktop was in: it built a new link, put the remote tracking and intercept services back,
set the backend to the node, and cleared the fallback. Its guard read the *configured*
backend, which stays "a node" throughout an outage, rather than the one in force.

So an operator who signed in while the desktop was cut off -- or after its node had
answered and before anybody switched back, which a session that expired mid-outage invites
-- ended the outage without the person D-15 requires, and threw away three things:

- **the reconciliation** PN-18 was holding for a person to see;
- **what the old link carried for the node**: the observations the outage tee had queued
  while cut off, and the exchange outbox;
- since GAP-134, **the forwarding**: the decisions taken while cut off would never reach the
  node's record, and would never be compared with the node's engagements by track.

Nothing said so. The line dates from the workspace's first commit; GAP-134 did not cause it,
but made it cost more.

## The fix

**Only a person switching back ends an outage** (D-15, `failover::switch_back`), and a
sign-in now leaves one where it is. When a fallback is in force, `connect_if_remote`
changes only who the outage's existing link signs in as, and tells the operator the outage
continues.

That needed the link to let its credential change. It used to move the credential into its
task, where nothing could reach it. It now holds it in state it shares with the task, read
afresh at every connection, and `NodeLink::replace_credential` sets it. A connection already
open keeps the token it signed in with; during an outage the change takes effect when the
node answers. A machine link refuses: its identity is its certificate (D-02), and a person
signing in on the desktop is no reason to change what a machine presents.

**Not a new link**, which would have been the smaller edit and the wrong one: the new link
would have started empty, and everything the outage had queued for the node would have gone
with the old one.

## One thing the fix had to guard against

`NodeLink` derives `Debug` and sits inside the application state, and `Credential` derived
`Debug` too, printing the passphrase. While the credential lived only in the link's task,
nothing could format it. Holding it in the link would have put the passphrase into any log
line or panic message that formatted a link, or anything holding one. `Credential`'s `Debug`
now prints the operator and `<redacted>`, which protects every holder rather than this one.

## The tests, and why each can fail

- `gungnir-remote/tests/credential.rs` holds three observations in a link whose node is not
  answering, then serves a node that knows **only operator 8** while the link still holds
  operator 7's credential. The link is refused and delivers nothing -- the half that makes
  the rest mean something. After `replace_credential(8)` its next attempt gets in and the
  three observations arrive, in order.
- `gungnir-app/tests/failover_e2e.rs`'s new test signs out and in again while a real node is
  down, then checks that the fallback, the embedded services and the link are all still
  there, and that when the node answers the reconciliation is computed over its history,
  which only the kept link could have fetched. The outage ends when a person switches back.
  **Run against `main`'s `session.rs`, it fails at its first check**, with the alert trail
  showing the defect: "signed in as operator 7", then "connected to node …; running remote",
  mid-outage.
- A unit test in `gungnir-remote/src/link.rs`: a machine link refuses a credential, and no
  formatting of a link or a credential prints a passphrase.

## Left as it was

A link signs in again with the credential it holds, whoever is signed in on the desktop at
that moment: it held the previous operator's credential across an expiry before this change
too. A decision's own record names who took it, whatever token carries the forwarding.
Whether the link should stop signing in at all while nobody is signed in on the desktop is
a question about DN-23 §5 rather than about outages, and it is not taken here.

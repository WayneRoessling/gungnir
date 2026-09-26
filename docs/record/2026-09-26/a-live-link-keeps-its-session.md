# A live link keeps its session

GAP-165 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
D-89 ([`../../mission/gap-analysis/data/decisions.yaml`](../../mission/gap-analysis/data/decisions.yaml)),
taken under the owner's delegation of 2026-09-25. Found while building GAP-120
([`../2026-09-25/one-scenario-through-both-backends.md`](../2026-09-25/one-scenario-through-both-backends.md)).

## What was wrong

A node-issued token always expires (DN-23 §5): after the baseline's session lifetime, or
900 s where the baseline names none (`gungnir-node/src/auth.rs`). The event stream is
authenticated once, when it subscribes, and the node does not close it when the token that
opened it lapses. The link signed in once per connection and handed that one token to every
request it made for as long as the connection lasted.

So fifteen minutes after sign-in, on a default baseline, every write a linked desktop made
was refused `401`. Its detections, sensor tasks and exchange sets waited in their outboxes.
A decision an operator took on the node's queue came back to PN-07 as refused, and nothing
on the screen connected the refusal to a token. The stream kept the picture moving, so the
link looked healthy throughout. Where the baseline names no lifetime the desktop's own
session never expires, so nothing ever dropped the link and started it again.

## What was built

The link now keeps its session for as long as it is up (`gungnir-remote/src/link.rs`,
`LinkSession`). It renews in two cases:

- **Ahead of expiry.** The node's sign-in answer carries the token's expiry, and the
  snapshot taken straight after carries the node's own time, so the lifetime is read off
  the node's clock. At three quarters of it the link signs in again with the credential it
  already holds for reconnecting (GAP-143). That leaves a quarter of the lifetime, nearly
  four minutes at the default, for a slow node to answer.
- **On refusal.** Any request answered `401` marks the token refused. On the next forward
  tick the link signs in again before it offers anything.

A `401` is never taken as the node's answer to what was sent:

- detections and sensor tasks stay queued;
- a decision or an outage's batch goes again under the same key;
- an exchange set is not recorded as a refusal of the publisher, which only a new sign-in
  would have lifted (GAP-146).

If the renewal itself is refused or unreachable, the connection ends. The link then
reconnects from the start, so the desktop sees a link that is down rather than one that
looks up while every write is refused.

The desktop's own session still bounds all of this: a desktop whose session expires drops
the link (`session::sweep_expiry`). Renewing keeps the node's side current only for as long
as the desktop may act.

Not human-owned: the change is in the link's session handling. It does not touch
`gungnir-security`, the node's authentication, `gungnir-api`'s routes, or the TLS identity
path.

## How it is tested

`gungnir-remote/tests/token_renewal.rs` drives the node's clock with `NodeApi::set_now`
instead of waiting for a token to lapse.

- **Refusal.** The first test moves the node's clock past the token's expiry in one step.
  The next detection is refused, the link renews once, and that same detection is delivered.
- **Ahead of expiry.** The second test gives the node's tokens two seconds of its own time.
  The link renews twice before anything is refused, stays up, and still delivers.
- **Arithmetic.** A third test pins the renewal point: three quarters of the lifetime, and
  none when the node sends no time.

The first two tests were run once with the renewal disabled, and both failed.

## Left as it is

- **A node that sends no time** (older than GAP-140) is renewed on refusal only.
- **Deployments with a set session lifetime.** Where a baseline names a lifetime, the
  desktop's session and the node's token last equally long. The link renews the node's side
  at three quarters of it, and the desktop's own expiry still ends the session when it
  comes; that is DN-23 §5's intent.

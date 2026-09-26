# The outage tests' proxy held a socket open

GAP-167 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
found while building GAP-165.

## What was seen

`cut_off_and_reconnected.rs::an_outage_outlives_the_desktop_that_fell_into_it` failed once
locally with a timeout. The run took 33 s where it usually takes 14 s.

Timing every wait in the file showed which one had run out. "The desktop to fall back"
normally takes 7.0 s: the heartbeat timeout, measured from the last beat before the cut.
Its budget is 30 s, and 33 s is about 3 s of setup plus that whole 30 s spent waiting for
something that never happened.

It was not load. Every wait in the file finishes within about a quarter of its budget, and
sixteen copies of the test binary run at once all passed in about 16 s each.

## The mechanism

The test cuts desktop A from its node through a loopback TCP proxy, whose loop did three
things in order for each accepted connection:

1. checked `open`;
2. connected upstream;
3. registered the pair in the list `cut` closes.

A cut landing between the first step and the third drained the list without the new
connection. The accept loop then registered it and piped it anyway, leaving one live
socket through a cut proxy.

The tests cut the moment the link reports connected. The link sets `connected` when its
snapshot is answered, and only then opens its event stream, so the stream's connection
is the one that arrives in that window. It went on carrying the node's heartbeat every
two seconds, so the desktop was never silent and never fell back. This is the same
pattern as GAP-136 and the failover end-to-end test's own note: "connected" read as
"subscribed".

Widening that gap to 300 ms reproduces the failure every time, with exactly "timed out
waiting for the desktop to fall back".

## The fix

The proxy now checks `open` and registers the connection under the lock `cut` and
`restore` hold, so every connection is either registered before the cut and closed by it,
or admitted after the cut and refused. With the 300 ms gap still in place, every outage
test passes.

`CountingProxy` in `gungnir-remote/tests/transport.rs` is a copy of the same proxy,
written for GAP-146, and had the same race. It is fixed the same way.

Test code only. No product behaviour changes: a real cut closes every socket at once,
which is what the proxy now does.

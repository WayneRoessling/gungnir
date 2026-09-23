# The register's contract is decided

D-68 ([`../../mission/gap-analysis/data/decisions.yaml`](../../mission/gap-analysis/data/decisions.yaml)),
DN-18 §11, GAP-137.

## What this item is for

[`a-partner-hears-what-the-node-decided.md`](a-partner-hears-what-the-node-decided.md),
written the same day, says the exchange register's shape is "what this change proposes".
It was: the code was built on a recommendation and D-68 was filed open, because the
contract a coalition partner reads is the owner's to settle rather than a build's.

**The owner answered as proposed on 2026-09-23**: one set per producer, merged on read.
The alternatives are in D-68's own outcome, with why each was not taken. Nothing about the
built behaviour changed with the answer -- what changed is that the register now has a
decided contract behind it rather than a recommendation, and this is the item that says
where the proposal stopped being one.

## What the answer covers

The register keyed by item and then by the name the connection was verified under; a
publish replacing that producer's set alone; a read concatenating every producer's with
this node's first, and answering `NotHeld` only when none holds any; the bound of
sixty-four producers an item, refused rather than evicted.

Three human-owned paths carried this: the `gungnir-api` write path, the node's decision
path (D-65), and the amendment itself as a design. What the owner has signed is
[`../../signatures.md`](../../signatures.md), which is the only place that says so.

## What it does not settle

GAP-145 stands as filed: the register has no lifecycle. A node restart still loses every
producer's set, no desktop republishes until it next issues a handoff, and no producer is
ever forgotten. The decision above is about how two writers share a register, not about
what a partner may believe of a producer that has gone quiet.

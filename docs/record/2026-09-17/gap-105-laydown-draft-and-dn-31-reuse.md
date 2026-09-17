# GAP-105 laydown draft and DN-31 reuse

GAP-105's design was drafted on 2026-09-16 and never merged. It is on the branch
`claude/gap105-laydown-placement` at `e356be1`, and this item says what is wrong with it
so that whoever picks the gap up does not merge it as it stands or rewrite it from
nothing. GAP-105 stays open.

**The DN number was taken in the meantime.** The draft is
`docs/design/DN-31-re-observation-for-a-laydown.md`. `DN-31` on main is
`DN-31-node-approval-queue.md`, signed and amended since. Two designs cannot share a
number: the draft needs the next free one, in its file name, its `docs/design/README.md`
row, and every citation of it. Check `docs/design/README.md` for what is free rather than
assuming `DN-32`.

**Its D-53 is also taken.** The draft records the decision that scoped it as D-53; D-53
on main came from the GAP-067 walk. The decision it describes needs a free id in
`data/decisions.yaml`, and GAP-105's `deps` then cite that instead.

**It predates the rules overhaul (#121).** The draft edits `gap-register.md`,
`closure-roadmap.md`, `decisions-needed.md` and the Python literals in `gen_gaps.py`. All
four are generated now, or were removed: the gap data lives in
`data/gaps.yaml` and `data/decisions.yaml`, and a gap's status is its last history entry.
So the branch's changes outside `docs/design/` do not apply at all, and the generator
edit least of all.

**What is still worth having** is the design text: a rehearsal re-observing a fixture's
recorded truth with the deployment's own sensor models at a laydown's placements, inside
the production binary, plus the four sub-decisions and the containment argument for
synthetic detections that go with it. That is the part to port.

**Why it is recorded rather than deleted or fixed now.** Fixing it means renumbering a
design, filing a decision and redoing the register wiring, which is the work of taking
GAP-105 on, not of tidying a branch. Deleting the branch would lose a 266-line draft
whose only defect is that the world moved. The branch stays; this item is the warning
that comes with it.

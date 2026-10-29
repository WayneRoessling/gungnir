# The owner's walk of 2026-10-29: D-146 to D-148

DN-34 (`../../design/DN-34-sensor-dead-spots.md`) raised three decisions out of the
owner's request of the same day for terrain dead spots on the 3D map, each with a
recommended default. The owner walked them the same day, in one batch, and took all
three to the default. Nothing is built by this change; each decision's outcome is in
`../../mission/gap-analysis/data/decisions.yaml`, and GAP-246, the gap all three
unblock, carries a history entry dated today.

| Decision | What the owner chose | Against the default? |
|---|---|---|
| D-146 | Dark means *claimed and unseen*: at least one searching or tracking sensor claims the cell by range, sector and band, and none can see it past the terrain | No |
| D-147 | One default height above ground, 100 m (`analytics.dead_spot_heights_agl_m = [100.0]`); 30 m and 300 m are entries a deployment adds | No |
| D-148 | Desktop only; the node computes no field and `GET /v3/coverage` is unchanged; the layer says so when linked | No |

## What the walk settled for the build

- **The rules table in DN-34 §4 stands as written.** Rule 3, the one choice that changes
  the operator's reading, is confirmed: a place beyond every ring stays the colour of the
  rings, and only a claim the terrain denies goes dark.
- **One field by default.** The first recompute after a DEM loads is one sweep per active
  sensor at one height, inside the MOP-14 budget DN-34 §6 plans for; a deployment that
  wants the low or the high band adds the entry to its baseline.
- **The node is untouched.** No DEM on a node's baseline, no second height field to keep
  in agreement, and the coverage route keeps its flat answer, which DN-33 amendment 1
  already distinguishes from the desktop's masked one.

## What the walk did not decide

- Whether the field is ever computed on the node: D-148 names the condition (a headless
  consumer, such as DN-13's recommender on the node, wanting shadow area) and leaves the
  decision to that day.
- Anything in DN-34 §13: the other 3D elements are an inventory and an order, gated on
  GAP-226, and none of them is raised as a decision here.

## Facts the questions rested on

- The three questions were written from the code at `7f324e3c` on the day they were
  taken; the walk changed no evidence, only what to do about it.
- The three decisions were raised and taken within one branch (pull request #257), so
  none was ever merged open; the generator's append-only check on `origin/main` sees
  three new resolved entries, not three edits.

# GAP-227: DN-36's verification rows agreed and gated

On 2026-10-31, asked whether DN-36 §8.1's tests should become pass criteria, the owner said
yes. Eleven rows go into `docs/verification-capability-table.md` §2 as gates, under "Rows
added by DN-36": rows a to g from §8.1, and h to k from amendments 1 and 2. This follows
DN-34's six (PR #259): the rows, the design note's amendment, a `kind: criterion` entry in the
signature ledger, and the tests.

**What was agreed, and what the owner was told.** The question put to the owner named the
rows and said what each would promise; it also said that the tests have no external oracle,
that the criteria needed rewording in places, and that they would stay in CI whether or not
they became criteria. The criteria below are my wording, written against what each test
asserts, and the owner has not reviewed the text of each. The "agreed" in the table is that
yes, on those terms. The ledger entry says the same and names the restated rows.

**Four rows differ from the draft in substance** (DN-36 amendment 7):
- **c**: the draft's "finest level whose cells are at least `MIN_CELL_POINTS`" is not
  asserted by the test, so it is not claimed. The row says what is: one level and stride, full
  cover of the visible store, no overlap, within the budget, "past the edge" set exactly when
  the view leaves the store.
- **e**: the build did not give `terrain_surface` a range argument, so the draft's wording
  does not apply. The row says the GAP-252 surface tests pass unchanged. No test compares
  bytes with a surface built before the store, and the row says so.
- **f**: the test holds line of sight's height field as the same allocation across a pan, and
  does not compute a dead-spot field; the dead spots follow from the field alone.
- **g**: widened to the stand-in and the fill sentence, which are tested.

**What the rows do not promise**, stated in the table and not left implied: no external
oracle (the loader rows rest on `pyproj`; these on the design's arithmetic and, for row b,
hand-worked means); a real-CRS store's seams across tile origins, which
`a_utm_store_is_placed_and_drawn` checks only in a local `crs` build and `ci.yml` does not run;
and row j checks that the outline is requested and the words are on screen, not the pixels.
§8.2's display checks stay the owner's.

**CI.** Rows a to g run in the workspace test run. Rows h to k need `terrain-fetch`, which
that run never compiles, so they depend on the step added in the fill cache's PR (#302),
which runs `terrain_store` and `terrain_fill` with the feature on.

**Also changed.** The "draft, not a gate" comments in the tests and DN-36's §8.1
introduction now point at the table. Earlier record items and gap history that call the rows
drafts are left as written, being history; amendment 7 supersedes them.

**Not done.** The two later consumers of a store, the UAS ground model (PR #306) and the node
(PR #307), have tests but no rows: §8.1 never drafted any for them, and none is invented here.

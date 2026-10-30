# Identity resolution stops growing with the session

`the-slow-frames-at-scenario-4-were-identity.md` made each comparison a new track id makes
against the known lineages cheap, and left one thing for the owner: a new id was still
compared with **every** lineage the session had minted, so the cost of a new track grew with
the session. The owner chose the remedy that changes no answer: compare only with the
lineages the comparison could accept.

## How much of it was churn

Scenario 4 (seed 4, `DenseSwarm { target_count: 200 }`) replayed to its end, release:

| Mission time | Track ids seen | Never confirmed | Lineages |
|---|---|---|---|
| 60 s | 1 599 | 1 300 | 1 597 |
| 120 s | 5 257 | 4 241 | 5 239 |
| 240 s | 11 521 | 9 249 | 11 484 |

Every track is resolved at first sight, while tentative, so every one mints a lineage. Four
in five never confirm and about three in five die within 5 s. Fewer than one id in two
hundred correlated to an existing lineage. Lineages grew about 50 a second throughout.

## What changed

`InMemoryIdentityResolver` files each lineage's last state in buckets of 10 mission seconds,
by when it was seen, and moves it when a new sighting lands in another bucket.
`best_match` reads only the buckets that can hold a lineage last seen inside
`CorrelationSettings`' gap window (`min_gap_s` to `max_gap_s`, 1 to 300 s by default), plus
one each side for rounding. `similarity` has always refused everything outside that window,
so nothing it could accept is left unread and **the resolver's answers are unchanged**. Where
the window is empty or not a number nothing is compared, as before; a mission time of
10¹⁵ s or more, where rounding could approach a bucket, is compared with every bucket.

Nothing is evicted. A lineage outside the window stays filed and is read again if a later
track's time brings it back inside, as a later session whose clock starts again at zero
would. The test that holds the resolver to a comparison with every lineage now runs its
clock past the window, back to zero and beyond 10¹⁵ s, under three windows.

## The figures

A scratch test, not committed: 50 new track ids a second for an hour of mission time,
resolved through `IdentityResolver::resolve`, timing the 50 at each mark. Release, the
resolver on main before this change against this one, run one after the other:

| Mission time | Lineages | Before, per new id | After, per new id |
|---|---|---|---|
| 60 s | 3 050 | 8.7 µs | 8.0 µs |
| 300 s | 15 049 | 46.8 µs | 39.6 µs |
| 600 s | 30 048 | 46.9 µs | 54.1 µs |
| 1 200 s | 60 045 | 110.7 µs | 41.4 µs |
| 2 400 s | 120 041 | 323.5 µs | 39.0 µs |
| 3 599 s | 179 986 | 639.0 µs | 49.1 µs |

Before, a snapshot bringing 50 new ids an hour in cost 32 ms on the frame; after, about
2.5 ms, as it does from the fifth minute on. Scenario 4's own replay ends at 240 s, inside
the window, so its frames are unchanged by this: p99 2.0 ms in its last 30 s, two runs.

## What this does not change

- **What the session holds still grows.** The lineages, and the per-track records and
  waypoints `gungnir-app`'s `IdentityState` keeps for the order of battle and the pattern of
  life, grow with every track id the session sees, about 50 a second in this raid. That is
  memory, not frame time, and bounding it is a question about what DN-19's products must
  keep.
- **The cost inside the window is the raid's churn rate times 300 s.** A denser raid, or a
  longer `max_gap_s`, puts more lineages inside it.
- **Tentative tracks are still resolved**, and journaled, at first sight. Resolving only
  confirmed tracks would cut the lineages about five times; it changes what DN-19's
  products show, and the owner chose not to take it here.

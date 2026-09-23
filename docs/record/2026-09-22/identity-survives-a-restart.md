# Identity survives a restart

GAP-123 closes. An object now keeps its `GlobalEntityId` across a restart and a replay,
which is the continuity CAP-2.7 exists for. Two defects stood in the way, and the one the
gap did not name was the worse of them.

## A track number meant nothing across sessions, and was being trusted anyway

`gungnir-track`'s `TrackManager` restarts `next_id` at zero in every process, so session
two's track 0 is not session one's track 0. The resolver's first lookup was the bare
`TrackId`, across every session it had folded and the live one, so after a restart a track
was handed whatever entity happened to hold its number: no similarity check, no
correlation recorded, nothing on screen or in the journal to say a join had been made. It
was reproduced before it was fixed. Two objects 300 km apart, session one's track 0 and
session two's track 0, came back from one resolver as a single entity with an empty
correlation list.

The existing cross-session tests never caught it because each of them used a different
track number in the second session, which is the one case that cannot collide.

Every lookup is now keyed by `Sighting`, the session and the track together:
`InMemoryIdentityResolver::resolve` takes the session, a lineage records sightings rather
than track numbers, and the desktop's own `records` and `seen_tracks` maps are keyed the
same way. A match across sessions can now only be made by `similarity`, which records its
confidence and basis, so a join is always something a reviewer can ask about.

## The identities were minted again at every start

Both binaries rebuilt the resolver by re-resolving journaled tracking events, which minted
a fresh UUID v7 for the same object on every start and in every replay. The node has
journaled `IdentityEvent::Minted` and `Correlated` since GAP-025 and then ignored them on
recovery, contradicting its own record. The desktop journaled no identity events at all.

Recovery now reads them back. `InMemoryIdentityResolver::restore` binds a sighting to the
identity the journal recorded, restoring a journaled correlation's confidence and basis
with it, so a restored lineage can still say why two sightings were joined. Each session's
identity events are read before its tracking events, because a journal may hold the
identity either side of the track update it is about. A track no identity event names --
a session journaled before those events existed -- is resolved as before, which mints once
and is stable from then on.

The desktop also publishes the events now, as the node does: one per track per session,
not one per tick, because an identity is a claim about what a track *is* and repeating it
every frame would bury the tracking events it sits beside.

**D-11 is untouched.** Identities stay UUID v7, minted the same way, keeping the mint
ordering D-11 chose. Deriving them deterministically would have made recovery trivial and
cost that ordering, and it would have needed D-11 amended; reading back what was recorded
does not.

## What is tested

- `gungnir-identity`: a track number reused in a later session is a different entity, with
  no correlation recorded for a join that was not made; and a journaled identity is
  restored rather than minted, with its correlation's basis.
- `gungnir-node`: a session's journal folded at start gives back the identity it recorded,
  and the live session that follows recognises the same object, on similarity, without
  minting a second entity.
- `gungnir-app` (`identity_survives_a_restart.rs`): the gap's own criterion. One object in
  session one, session two after a restart, and a replay of both: the same identity in all
  three, joined across the restart on similarity, and the lineage naming both sessions.
  The second test holds the collision case end to end. With recovery made to ignore the
  journaled identities, the first test fails with "the object was minted a second identity
  after a restart", so it tests the fix rather than passing beside it.

## What this does not do

The picture is unchanged: `TrackView` still carries no `GlobalEntityId`, for the reason
`gungnir-node/src/entities.rs` gives. Identities minted before this change are not
retrofitted: a journal written without identity events gets one mint on its first fold
under the new code, and is stable afterwards.

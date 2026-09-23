# PN-14 first apply compares with the running revision

GAP-128 closes. PN-14 refused the first apply of every edited baseline as
`RevisionNotAdvanced`, whatever revision the edit carried.

**Why.** PN-14 reads its candidate from the desktop's own baseline file: an administrator
edits the file, reloads, validates and applies. `FileConfigStore::apply` refuses a
candidate whose revision does not advance past the one in force, and took the revision in
force from what the store had applied this session or, failing that, from the file on
disk. On a session's first apply the store had applied nothing, so the revision in force
came from the file the candidate had just been read from. The candidate was compared with
itself, and `2 <= 2` refused it.

**The fix.** The store is told the revision the process started with,
`FileConfigStore::set_running_revision`, and compares a candidate with that before it
would fall back to the file. `AppState::with_config_and_store` tells it, which is the one
place a desktop gets its store, so the startup path and every test that builds a desktop
with a store get the same behaviour. The running revision is kept apart from
`applied()`, which still means "applied in this process" and stays `None` until something
is. The file remains the last fallback, for a caller that says nothing; the node keeps no
store after start-up and is unaffected.

**Tests.** `gungnir-config` gains one covering all three cases: an edit to revision 2
applies over a running revision 1; an edit that kept revision 1 is refused against the
running revision; and a store told nothing still compares with the file, pinned as the
documented fallback. `gungnir-app`'s `sustainment.rs` gains the case through PN-14 as an
administrator uses it, reload, validate and apply from a desktop started from the file:
the advanced edit applies on the first apply, and the unadvanced one is refused. With the
one line in `with_config_and_store` removed, that test fails, so it tests the fix rather
than passing beside it.

# Every float kept, old sessions purged on purpose

GAP-126 and GAP-122 ([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml)),
D-77 and D-78, both taken under the owner's delegation of 2026-09-25. It raised GAP-152
and GAP-153. Both gaps were filed by the GAP-067 walk against the `gungnir-store` rows of
`docs/verification-capability-table.md`; neither row is changed here, and both await the
owner's walk.

## GAP-126: a non-finite float in an envelope

**What was wrong.** JSON cannot spell NaN or an infinity, and `serde_json` writes both as
`null`. `read_session` then failed the whole session on such a line mid-file, and dropped
it as a torn tail when it was last. An `Option<f64>` was worse: `null` read back as
`None`, so the value was not refused but changed.

**Whether it happens.** Every float reachable from `Event` was surveyed, with its
producers. One live producer was found: PN-10's requirement deadline. Rust parses "inf"
and "NaN" as numbers, both pass the panel's `m <= 0.0` check, and `1e308` minutes is
finite until it is added to the clock. The requirement was journaled with an infinite or
NaN `needed_by`, and read back as a requirement with **no deadline**. The ingest gateway
refuses non-finite measurements and times, identity confidence and plan geometry are
guarded, and the configuration is JSON, which cannot hold one. Track views carry
covariances a diverging filter can overflow -- the plain Kalman step checks nothing and
`assert_psd` is debug-only -- but no production path publishes a `TrackView` event today,
so that source is latent.

**What was built (D-77).** The deadline is refused where it is typed and again where the
requirement is made (`RequirementError::BadDeadline`, checked before an identifier is
taken). And the journal now carries a non-finite float rather than refusing the envelope.
A refusal would have been correct for the one producer found and wrong for the next: the
envelope that carries a diverged covariance is the record an investigation of that
divergence needs, and a journal that refused it would lose the evidence to keep the
format tidy.

The encoding is chosen so nothing that worked changes. An envelope whose floats are all
finite is written exactly as before, byte for byte, so every existing journal and the
gated round-trip row are untouched. An envelope carrying a non-finite float is written as
`~` followed by JSON in which each such float is the string `"\u0000f64:<16 hex digits>"`
(or `f32` and eight), and a genuine string beginning with U+0000 gains a second one so it
cannot be taken for a token. Bits rather than a name, so NaN's sign and payload survive.

It is an adapter around `serde_json`'s serializer and deserializer
(`gungnir-store/src/nonfinite.rs`), not an attribute on each field. Several hundred float
fields reach the journal, and some -- an extent's radius, a plan's intercept point -- sit
inside internally tagged enums, which serde buffers before the field's own type sees
them. A token read as a string at that point cannot become a float later, so the
deserializer turns a token into a float the moment it reads one, at any depth. `append`
decodes and re-encodes a marked line before writing it and refuses one that does not come
back identical (`StoreError::Unfaithful`): a journal must never hold a line it cannot
read back.

**What the tests hold.** `gungnir-store/tests/non_finite_floats.rs` journals a diverged
track (an infinite variance, a NaN state entry, a NaN `f32` latency), the "inf" deadline
inside a circle of infinite radius, and an envelope stamped with a signed NaN carrying a
payload -- mid-session and as the last line, under both D-04 profiles, sealed and in the
clear, read by the journal that wrote it and by a fresh one -- and compares every float's
bits. A session whose only line is non-finite is not read as torn. Finite envelopes are on
disk as `serde_json` writes them. A journal written before this reads as it did.

**What it did not settle.** The node streams envelopes to its desktops, and serves
`GET /v3/history`, as plain `serde_json`. A non-finite value still goes out as `null`,
which the desktop reads as the node ending the stream. That is an interface change for
both ends of the link: GAP-153.

## GAP-122: the retention purge

**What was wrong.** `RetentionPolicy` had a 90-day default and two predicates nothing
called. Journals grew without bound, and because nothing returned `NotImplemented`,
`docs/unbuilt.md` did not list it.

**What was built (D-78).** The purge, not a refusal.

- **Declared, or nothing is purged.** `ConfigBaseline::retention` is optional. The data
  architecture leaves periods to the customer's record-keeping obligation, and a product
  that began deleting mission records on an upgrade because of a default nobody chose
  would be the worse of the two failures. Both binaries say at start which it is.
- **Age is days since the journal was last written**, from the file's own time. The last
  write, so a week-long session is kept the full period after it ended; the file's time,
  so a session sealed under an ephemeral key (DN-22 §5), which no later process can read,
  is still reached. A time in the future is no age.
- **When.** Both binaries purge once the live session exists -- so there is a session to
  protect and one to record the purge in -- and hourly after that. Never while opening
  the journal, which the store's own design forbade.
- **What is never purged.** The session open for appending; a session under a hold, a
  file beside the journal whose text is the reason, which the after-action review places
  when it opens and releases when it closes and an administrator may place by hand (the
  administrator's task analysis names "purging a session under review" as the error to
  design out); and whatever the caller protects. On the desktop that is every session an
  open requirement has events in, because the requirement is rebuilt from all of them and
  purging the one that tasked it would bring it back as merely stated; the sessions
  holding the highest requirement identifier and the highest launch-warning serial,
  because each serial continues past the highest the journal holds; and an unfinished
  outage's session, which the merge reads. Each expired session kept is logged with its
  reason.
- **Crash safety.** Per session, the journal is renamed out of the listing, the mission
  record beside it is removed, and only then is the file deleted. A purge interrupted at
  any step leaves every session whole and listed or out of the listing, never half, and
  the next purge finishes it. A purge removes whole sessions and never rewrites a line.
- **Seen.** Each removal is a `RetentionEvent` in the live session -- the only record of
  a session whose own journal is gone -- a log line, a count on the node's health line
  and the desktop's state, and an alert on the desktop naming the sessions. A failed
  purge alerts once per distinct failure and is tried again in an hour.

**What building it found.** The desktop continued the launch-warning serial from the
*count* of warnings recovered. The two agree only while nothing is removed from the
journal; after a purge the count falls below the serials still on record and a new
warning would have taken an old one's number. It now continues from the highest serial.
The requirement serial already continued from the highest identifier, but a purge of
every session naming it would have reissued it, which is why that session is protected.

And the policy's second field, the audit log's age, has nothing to purge: the audit log
is held in memory for the life of the process. That is GAP-152, human-owned
(`gungnir-security`).

**What the tests hold.** `gungnir-store/tests/retention_purge.rs` builds a journal with
sessions one second below, exactly at, and one second above the limit, one far above, and
expired sessions that are held, protected and open, with ages set on whole seconds so "at"
is exact on any filesystem: only the two above the limit go, `retire` is called for
exactly those, and every other session reads back exactly from a fresh journal. It also
holds a released hold, a future time, and a purge interrupted between the rename and the
delete, after which the journal reads and the next purge finishes the job.
`gungnir-mission` holds that a purged session's record goes with it and that no identifier
is reissued afterwards. `gungnir-app/tests/retention.rs` holds the desktop's
protections, the journaled and alerted removals, the serials after a restart, a session
held for exactly as long as its review is open, and a baseline without a policy removing
nothing. `gungnir-node/tests/retention.rs` runs the real node over an aged data
directory with a hand-placed hold.

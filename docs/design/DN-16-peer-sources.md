# DN-16 Peer track and warning ingestion

Closes GAP-009. Status: first draft, 2026-09-05. **Design only; no code exists.**

## 1. The gap and the thread step it blocks

Higher command and neighbouring sectors see threats before we do. MT-01 and MT-02 depend
on those minutes. The interface publishes a snapshot and an event stream **outward**, so a
peer can watch us, and there is no path by which their tracks enter our picture with their
provenance and staleness intact.

## 2. The owning component

`gungnir-ingest`, behind the existing `IngestAdapter` boundary. A peer is a source, and the
gateway that validates and quarantines every other source must validate this one too. That
is the whole reason not to add a special path: a peer feed is the least trustworthy input
the system has, because it is remote, delayed, and written by somebody else's software.

## 3. Types

In `gungnir-model`:

```rust
/// Where a peer-sourced track came from and how far behind it is.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PeerOrigin {
    /// Configured name of the peer node, not its address.
    pub peer: String,
    /// The peer's own track identifier, kept so a correction can be matched.
    pub remote_track: String,
    /// Mission time the peer stamped, and when we received it. The difference
    /// is the age the operator must see.
    pub peer_time: MissionTime,
    pub receipt_time: MissionTime,
    /// Quality the deployment assigns to this peer, from configuration. Not a
    /// number the peer sends about itself.
    pub assigned_quality: f32,
}
```

`Provenance` gains an optional `peer: Option<PeerOrigin>`. A track fused from local and
peer detections carries both.

In `gungnir-ingest`:

```rust
/// Consumes another node's v1 event stream and presents it as a source.
pub struct PeerSourceAdapter {
    pub peer: String,
    pub assigned_quality: f32,
    /// Beyond this age, envelopes are accepted but marked stale rather than
    /// entering fusion. Never silently discarded.
    pub max_age_s: f64,
}
```

## 4. Edges

**None.** `gungnir-ingest` already owns adapters; the transport it uses arrives with
GAP-041.

## 5. Behaviour

**A peer is a source, so it goes through the gateway.** Validation, authentication, and
quarantine apply exactly as they do to a radar. A peer that sends a malformed or
implausible track gets its track quarantined with a reason, and the quarantine appears on
the stream like any other.

**Quality is assigned by us, not claimed by them.** `assigned_quality` comes from our
configuration. A peer that marks everything high confidence cannot raise its own weight in
our fusion. This is the single most important rule in the note.

**Age is always visible.** The difference between `peer_time` and `receipt_time` is carried
on every peer-sourced track and shown. A thirty-second-old peer track drawn identically to
a live local one is a lie the operator cannot detect.

**Merge policy:**

1. Peer tracks enter fusion as a source with the assigned quality, not as authoritative
   tracks that overwrite local ones.
2. Where a peer track and a local track are the same object, the identity resolver
   correlates them and the lineage records both, which is what `gungnir-identity` already
   does for local sources.
3. Where correlation is uncertain, **both are shown**. A wrongly merged pair is harder to
   notice and harder to undo than a duplicate, and a duplicate is visible.
4. Beyond `max_age_s`, a peer track is marked stale and is not allocated against, which is
   the existing rule for stale tracks and needs no exception.

**Launch warnings** arrive on the same path as a distinct message rather than as a track,
because a warning is a statement about the future with no kinematic state. It raises an
alert with the peer named, and it never creates a track: a track we have not observed is a
track we cannot maintain.

**Failure.** A peer that stops sending is reported through `gungnir-observability` as a
source that went quiet. Its existing tracks age and go stale on the normal path rather than
disappearing.

## 6. Configuration and interface delta

`ConfigBaseline.peers: Vec<PeerConfig>` with a name, an endpoint reference, an assigned
quality in 0.0 to 1.0, and a maximum age. Validation: unique names, quality in range,
positive age, and the endpoint exists.

Interface: no new endpoint on our side. This is a client of another node's event stream,
which is why the contract needs no change to support it, and that is a point in favour of
the contract as designed. The peer's path version is the peer's, not ours: a peer still
serving `/v1/events` is consumed at `/v1`, and the schema version on each payload decides
whether we can read it. That is exactly what the versioning rule exists for, and it is why
our own move to `/v2` does not strand a peer.

`Provenance.peer` is additive.

## 7. User-interface delta

| Panel | Change |
|---|---|
| PN-03 Track table | A peer column and an age column; peer tracks sortable and filterable as a group |
| PN-02 Viewport | Peer-sourced tracks drawn distinctly, with the distinction being pattern and label rather than colour alone |
| PN-04 Track detail | The peer, its assigned quality, and both timestamps |
| PN-09 System health | Per-peer connection state, message rate, and quarantine count |
| PN-01 Status strip | A peer that has gone quiet |

## 8. Verification

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-1.6 Peer early warning | Two nodes in one test process, one feeding the other, plus fault injection | A peer track never carries a quality higher than its assigned value; peer age is present on every peer-sourced track; a malformed peer track is quarantined with a reason and does not enter the picture; an uncertain correlation leaves both tracks visible; a launch warning creates an alert and no track | Generated peer streams; TT-01 and TT-02 replayed as the peer's picture |

## 9. Amendment 1: the launch-warning type, and which exchange item gates it (2026-09-06, unsigned)

§5 says what a launch warning **is** and what it must never do. §3 names the types this
note owns and does not name one for it, and §5 names no `ExchangeItem` under which it may
be sent. Both were needed to build it, and this records what was chosen rather than
leaving the note to be read as though it had said.

```rust
/// A peer's statement that something has been launched. No position, no velocity, no
/// covariance -- deliberately, because §5's reason for the message existing is that a
/// launch warning is a statement about the future with no kinematic state.
pub struct LaunchWarningReport { pub id: String, pub what: String, pub at: MissionTime,
                                 pub releasability: Releasability }

/// The report as this deployment received it. Split from the report so that the peer's
/// name and the receipt time are **stamped by us and never read off the wire**, which is
/// the rule §4 already applies to quality.
pub struct PeerLaunchWarning { pub peer: String, pub report: LaunchWarningReport,
                               pub receipt_time: MissionTime }
```

**"It never creates a track" is held structurally.** Nothing in the ingest path can
produce a `DetectionView` from a warning: the two ride separate queues with separate
accessors, so a caller that only knows about tracks keeps compiling and keeps being right.
A check that could be forgotten would have been the weaker guarantee.

**Kept apart from DN-03's warnings.** That ledger is an obligation *we* owe an asset and
is what MOE-01 measures; this is what a peer told us. Folding them would put a peer's
claim into our own performance record.

**Gated under `ExchangeItem::Warnings`, and that is a choice the owner may overturn.** It
is the restrictive reading: a party not permitted our warnings does not get a peer's
either. The alternative is a sixth `ExchangeItem`, which would be a change to the
canonical model made by the crate that publishes it rather than by a note, so it was not
made here. **A warning received from one peer is never forwarded to another**, whatever
the agreement says: a partner able to read our inbound warnings can read our peer list off
the stream.

## Traceability

GAP-009; CAP-1.6; D-08 for the agreement model; depends on GAP-041 for the transport;
feeds DN-18; `../gungnir-api-v1.md`; `../ux/wireframes/WF-03-track-table.puml`;
principles AP-02, AP-07.

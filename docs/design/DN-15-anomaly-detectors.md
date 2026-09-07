# DN-15 Track and feed anomaly detectors

Closes GAP-021. Status: first draft, 2026-09-05. **Design only; no code exists.**

**This note follows D-13 and does not reopen it.** The owner decided on 2026-09-04 that
detectors are pure functions in `gungnir-analytics` over track and health snapshots, that
the binaries call them on the tick and raise alerts through `gungnir-observability`, and
that no new dependency edge is added. The 2026-09-05 rule allowing edges applies to
closures decided from that date; a resolved decision stands.

## 1. The gap and the thread step it blocks

MT-05 asks the system to notice a vessel that switched its transponder off, a report that
does not match its track, or a craft loitering where nothing should loiter. MT-07 asks it
to notice a sensor emitting implausible data. Today the operator notices, or nobody does.

## 2. The owning component

`gungnir-analytics`, as pure functions. The binaries own the calling and the alerting.

## 3. Types

In `gungnir-analytics`:

```rust
/// What a detector found. Never a verdict: an anomaly is a reason to look, and
/// the operator decides what it means.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Anomaly {
    pub kind: AnomalyKind,
    pub subject: AnomalySubject,
    pub observed_at: MissionTime,
    /// Why the detector fired, in the words the alert shows. Never empty.
    pub detail: String,
    /// The detector's own name and version, so an alert can be traced to the
    /// rule that raised it and a noisy rule can be found and tuned.
    pub detector: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AnomalySubject {
    Track(TrackId),
    Sensor(SensorId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AnomalyKind {
    /// A cooperative identity that was present and stopped.
    CooperativeIdentityLost,
    /// A cooperative report whose position disagrees with the track.
    CooperativeMismatch,
    /// Persistent slow motion inside a watched area.
    Loitering,
    /// Kinematics outside the envelope of any known class.
    ImplausibleKinematics,
    /// A feed whose rate, timing, or values are outside its own history.
    FeedImplausible,
    /// A feed that stopped without a mode change explaining it.
    FeedSilent,
}

/// Each detector is a pure function of a snapshot. Signature shared so the tick
/// can call them uniformly and so each is independently testable.
pub type TrackDetector = fn(&[TrackView], &AnomalySettings) -> Vec<Anomaly>;
pub type FeedDetector = fn(&[SensorHealthSnapshot], &AnomalySettings) -> Vec<Anomaly>;
```

`SensorHealthSnapshot` is a plain struct of the facts a detector needs: sensor identifier,
mode, last message time, message rate, and rejection count. It is defined in
`gungnir-analytics` and populated by the binary, which is what keeps the edge out.

## 3a. Correction made during implementation, 2026-09-05

Section 3 typed the track detectors as `fn(&[TrackView], ...)`. That cannot be built:
`gungnir-analytics` does not depend on `gungnir-model`, and adding that edge is exactly
what D-13 forbids and what `ARCHITECTURE.md` does not draw. The note contradicted its own
section 4.

**The track side gets a snapshot too.** `TrackSnapshot` carries primitives, the binary
projects the model types into it, and identifiers are plain integers that the caller maps
back when it raises the alert. That is what `SensorHealthSnapshot` already did; the
correction is to apply the same rule on both sides.

One consequence worth stating: the cooperative-mismatch comparison moves to the caller,
because the classification type belongs to the model. The detector still owns the rule,
the wording, and the stated limit; the caller only supplies whether the two disagree.

## 4. Edges

**None**, per D-13. The binaries assemble the snapshots and call the detectors.

This is the one place where the 2026-09-05 rule and an existing decision could have been
read as conflicting, so it is stated explicitly rather than left to inference.

## 5. Behaviour

Each detector is a rule, and each rule states what it cannot know:

| Detector | Fires when | What it cannot know |
|---|---|---|
| `CooperativeIdentityLost` | A track that carried a cooperative identity for a configured minimum goes a configured period without one | Whether the transmitter failed or was switched off. It never says "went dark deliberately" |
| `CooperativeMismatch` | A cooperative report's position differs from the fused track by more than the combined uncertainty allows | Which of the two is wrong |
| `Loitering` | Speed stays below a threshold inside a watched extent for longer than a threshold | Whether loitering is suspicious here. That is the watched area's configuration, set by a person |
| `ImplausibleKinematics` | Speed, acceleration, or climb rate is outside every class envelope in the catalogue | Whether the track is real and unusual or a tracking artefact |
| `FeedImplausible` | Message rate, timestamp ordering, or value ranges depart from the feed's own recent history | Whether the sensor is faulty, jammed, or spoofed |
| `FeedSilent` | No message for longer than the feed's expected interval, with no mode change explaining it | Whether the link or the sensor failed |

The right-hand column is not decoration. It is the text that goes on the alert, so the
operator sees the limit of the inference at the moment they act on it.

**Rules that keep the detectors from becoming noise or authority:**

1. **No detector quarantines, drops, or downgrades anything.** They raise alerts. The
   ingest gateway quarantines; a detector that could suppress a feed would be acting.
2. **Every anomaly names its detector and version.** A detector that fires constantly is
   found by grouping on that field, and tuning it is a configuration change with a record.
3. **Detectors are stateless between calls**, taking whatever history they need from the
   snapshot. That is what makes them testable and replayable, and it is why the feed
   snapshot carries a rate rather than the detector computing one from remembered state.
4. **A detector that cannot evaluate returns nothing**, not a default anomaly. A missing
   class catalogue means `ImplausibleKinematics` is silent and the health summary says the
   detector is unavailable.

**The learned successor.** Plan 09's ML-04 replaces none of these; it adds a detector
alongside them with the same output type and the same inability to act, and the model name
and version go in the `detector` field, which is why that field is a string.

## 6. Configuration and interface delta

`ConfigBaseline.analytics.anomaly: AnomalySettings` with thresholds per detector and the
watched extents for loitering, each validated as finite and positive. A detector with no
configuration is **off**, and the health summary lists which detectors are running, so an
unconfigured detector is visibly absent rather than silently missing.

Interface: anomalies surface as alerts on the existing alert path; no new endpoint.
`Alert` gains an optional `anomaly: Option<Anomaly>` so the panel can show the detail and
the detector.

## 7. User-interface delta

| Panel | Change |
|---|---|
| PN-08 Alerts and incidents | Anomalies as alerts, grouped by detector, with the detail and the stated limit |
| PN-04 Track detail | Anomalies raised against this track |
| PN-09 System health | Which detectors are running and which are unconfigured |
| PN-03 Track table | A marker on tracks with an open anomaly, sortable, so the operator can work them |

## 8. Verification

| Capability | Method | Pass criterion | Data source |
|---|---|---|---|
| CAP-2.9 Anomalies | Unit tests per detector against crafted inputs, plus replays of the two threads | Each detector fires on its positive case and stays silent on its negative case; no detector alters a track, a feed, or a health flag; every anomaly carries a non-empty detail and a detector name; an unconfigured detector produces nothing and is reported as not running | TT-05 for the surface cases; TT-07 for the feed cases; generated envelope violations |

The false-alarm rate per detector is measured on the ten sample sets and recorded, so the
learned detector in ML-04 has a baseline to beat rather than an assertion to displace.

## Traceability

GAP-021; CAP-2.9; D-13; MT-05, MT-07; `../ml/use-cases.md` ML-04 for the learned
successor; `../test-tracks/` for the class envelopes;
`../ux/wireframes/WF-08-alerts-incidents.puml`; principles AP-01, AP-02, AP-07.

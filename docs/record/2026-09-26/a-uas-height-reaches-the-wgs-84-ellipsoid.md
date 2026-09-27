# A UAS height reaches the WGS-84 ellipsoid

GAP-196, closed; GAP-198 and GAP-199, filed
([`../../mission/gap-analysis/data/gaps.yaml`](../../mission/gap-analysis/data/gaps.yaml));
D-123 and D-124
([`../../mission/gap-analysis/data/decisions.yaml`](../../mission/gap-analysis/data/decisions.yaml)),
both taken under the owner's delegation of 2026-09-26. Built on 2026-09-26, on top of
GAP-108's grid ([`a-converted-height-carries-its-vertical-datum.md`](a-converted-height-carries-its-vertical-datum.md), D-121).

**Human-owned code is touched.** The Category 129 arm of the ASTERIX adapter in
`gungnir-ingest` (`src/adapters/asterix.rs`), where the height is now corrected or
flagged, is the `gungnir-ingest` gateway, and so is the new `gungnir-ingest/src/geoid.rs`
interface it reads. Human-owned; see [`../../signatures.md`](../../signatures.md).
Nothing else touched is on the list in [`../../agentic-workflow.md`](../../agentic-workflow.md):
`gungnir-model`, `gungnir-interop`, `gungnir-data`, `gungnir-app`, `gungnir-ui` and
`gungnir-node`'s feed binding.

## What was wrong

An ASTERIX Category 129 report states a UAS's height only above mean sea level
(I129/090). The codec put that number in `Geodetic::alt_m`, which everywhere else in the
picture is a WGS-84 ellipsoidal height. So the UAS sat off by the local geoid separation:
+34.7 m over the Baltic, -22.6 m in Oregon, anywhere from -106.9 m to +85.8 m. The only
trace was a sentence in the report's `conversion_loss`. The detection built from the
report did not carry that sentence, and nothing on the picture showed it.

## What the specification says, checked again

EUROCONTROL-SPEC-0149-29, Category 129, **edition 1.2** (12 June 2019), read from the
primary PDF again on 2026-09-26:

- **§5.2.9, I129/090.** Definition: "Altitude above Mean Sea Level (AMSL)". Three
  octets, LSB 0.1 m, negative values below MSL in two's complement. Nothing else: no
  geoid model and no source, GNSS or barometric.
- **The change history** has one entry for the item. Edition 1.1 (15 August 2018) added
  the negative-value note, which it calls "editorial only".
- **§5.2.8, I129/080** is latitude and longitude in WGS-84 and holds no height. **§5.2.10,
  I129/100** is height above ground level. **I129/110** is a GNSS accuracy, so the
  position, at least, is GNSS.
- **Edition 1.2 is still the latest.** EUROCONTROL's own publication page for the
  category lists editions 1.0, 1.1 and 1.2 and nothing later (checked 2026-09-26). GAP-101
  found the same in the category-status list of 22 October 2025.

So the specification does not say which "mean sea level" a sender means. A GNSS
receiver's MSL output is its ellipsoidal height less its own built-in geoid, commonly
EGM96 and sometimes a coarser table. A barometric source with a QNH setting is an
orthometric height of a different kind again.

## D-123: the height is read as an EGM2008 height, and corrected in the adapter

**What was decided.** I129/090 is read as a height above the EGM2008 geoid. It is placed
on the ellipsoid by adding EGM2008's separation N at the report's position.

- EGM2008 is the one geoid model this deployment verifies (D-121).
- Its difference from EGM96 or from a receiver's built-in model is decimetres to metres.
  The uncorrected error is 35 m here and up to 107 m elsewhere.
- The residue is stated on every corrected report, so it is said rather than assumed
  away: "a sender whose mean sea level is EGM96 or a receiver's coarser model differs
  from EGM2008 by decimetres to metres".

**Why this does not contradict D-121.** D-121 refuses to convert a file that *states*
EGM96 or NAVD88 with the EGM2008 grid, because that file states what its numbers are.
A Category 129 sender states nothing more than "mean sea level", and the specification
defines nothing more. The honest reading is the best available model, with its residue
said.

**Where the correction runs.**

- **Not in the codec.** `gungnir-interop` reaches no grid, and `ARCHITECTURE.md` §7.1
  gives neither it nor `gungnir-ingest` an edge to `gungnir-data`.
- **In the ingest adapter.** The adapter is what places the report in the deployment's
  frame. It reads a `GeoidSeparation` (`gungnir_ingest::geoid`), which a host lends
  through a `GeoidHandle` in the `FeedSinks` it binds with.
- **The desktop lends it.** Once its start-up grid check verifies the grid
  (`gungnir_app::geoid::lend_to_feeds`), it sets that handle. The handle can change after
  binding, because the desktop binds its feeds before the 80 MB hash has settled.
- **No dependency edge is added.** `gungnir-app` already reaches both `gungnir-ingest` and
  `gungnir-data`.

**One PROJ lookup thread.** `gungnir_data::geoid::undulations` builds its pipeline once
per call, which opens the grid each time. That suits a file converted once, not a feed
asking once per report. The live lookup, `UndulationService`, builds the same written-out
`vgridshift` pipeline once, with no ballpark fallback and the verified file's absolute
path.

- `proj` 0.31's `Proj` is neither `Send` nor `Sync`, since it holds PROJ's raw context
  pointers, and the adapter is owned by the gateway and must be `Send`.
- So one thread owns the pipeline and answers over a channel. The service is the sending
  end, cheap to clone and shared by every feed.
- A lookup that is not answered within 250 ms is a refusal, and the height stays flagged.
- The alternative was `unsafe` FFI to move a PROJ context between threads: human-owned,
  and unneeded.

**The datum is data, not only prose.** `UasIdentificationReport` gains
`altitude_reference: UasAltitudeReference`, which is `GeoidCorrected { model,
separation_m }`, `MeanSeaLevelUncorrected { reason }` or `NoAbsoluteHeight`. The codec
emits the second, as sent; the adapter settles it. The report's `conversion_loss` now
holds only the losses that are not the height, and `losses()` joins the two, because the
height's loss is not settled until the adapter has run.

**Rejected alternatives.**

- Keeping the recorded loss: a known 35 m error that the grid can remove.
- Refusing EGM2008 here as D-121 does for a stated EGM96: nothing is stated.
- Correcting in the codec: it reaches no grid.
- A per-feed declaration of which geoid the sender uses, now that EGM96 is pinned too
  (D-125, merged while this was built): edition 1.2 gives a sender no way to state its
  geoid, and a gateway rarely knows which receiver each of its senders flies, so the
  declaration would be a guess made once for every UAS on the feed. The EGM2008/EGM96
  difference stays the stated residue instead. The lookup goes through the multi-grid
  model D-125 introduced: `lend_to_feeds` lends the EGM2008 grid's status and nothing
  else.

## D-124: without the grid, the height is flagged, weighed as a bias and counted

**When there is no grid.** No correction is possible in these cases:

- a build without `crs`, which is the Windows desktop `release.yml` publishes;
- a grid that is missing or refused;
- a position off the grid;
- the node, which links no libproj (GAP-198).

**What happens then.** The height is kept as sent, and it is never passed off as
ellipsoidal:

- the report says `MeanSeaLevelUncorrected` and why;
- the detection's `Provenance::conversion_loss` carries the report's every loss, so the
  flag reaches the picture and not only the report (before this, the detection carried
  none);
- the detection's up variance is widened by the square of EGM2008's largest separation
  anywhere;
- PN-03 marks the track's U "MSL" in the warning colour, and PN-04 says the height and
  why;
- `AsterixFeedStats` counts corrected and uncorrected heights, PN-09's radar-feed line
  shows both, and the node's health line shows the uncorrected count.

**The bound is the pinned grid's own extreme.** GDAL 3.11.3's `gdalinfo -mm` over the
whole pinned `us_nga_egm08_25.tif` (SHA-256 checked first) gives a minimum of -106.909 m
and a maximum of 85.824 m. `EGM2008_MAX_ABS_SEPARATION_M` is 106.91 m, rounded up.

- A 30 m sigma on a height that may be 107 m off is the silent ellipsoidal reading this
  ends.
- Treating the bound as one sigma is conservative on purpose: the tracker weighs the
  height as weak vertical evidence rather than as a measurement.

**Rejected alternatives.** Dropping the height or the report, which loses a real track;
a zero separation; the baseline variance alone.

## How it was checked

- **`gungnir-ingest`, with a fixed stand-in model.** The adapter's own tests cover:
  - no model: flagged with the reason, counted, variance widened, and the flag on the
    provenance;
  - a model lent after binding: corrected, with the report and the detection agreeing;
  - a model with no separation at the position: flagged with its reason;
  - a record with no I129/090: neither count moves.
- **`gungnir-data/tests/geoid.rs`, with the `crs` feature.** The live lookup, over the
  committed clip:
  - it gives the independent `pyproj` undulations to a micrometre, from its own thread
    and another;
  - it refuses a point off the clip (the Category 129 fixture's own 10 N, 20 W) and a
    non-finite one;
  - it refuses a grid removed after verification.
  - Without `crs`, `start` is refused, naming the feature.
- **`gungnir-app/tests/uas_identification.rs`, the desktop's own wiring.** The tests put
  real datagrams through the adapter `bind_feed` builds, holding the desktop's handle:
  - a record over the Baltic at a grid node (54.5 N, 15.0 E, 120.0 m AMSL);
  - the committed `cat129.raw` fixture.
  - **With the clip installed as the verified grid (a `crs` build):** the Baltic height
    gains N = 34.678051 m, the node value from the GAP-108 `pyproj` run, to 0.1 mm. The
    fixture, off the clip, stays flagged with PROJ's refusal. PN-04 says ellipsoidal,
    and PN-03 marks nothing.
  - **With a grid directory holding no grid:** the Baltic height stays 120.0 m, flagged
    "is not there". PN-04's line says NOT corrected, and PN-03 marks the track.
  - **Without `crs`:** a verified file is not lent, and the reason names the feature.
- **`ci.yml`'s `proj-crs` job** now runs `uas_identification` with the feature on, into
  the same `geoid.log` its anti-vacuous gate counts.
- **Run before pushing.** The `crs` build and tests were run in `rust:1.98-slim-bookworm`
  with the job's own packages, and the full pinned grid's `#[ignore]`d tests with it.

## What is not done

- **GAP-198.** The node's Category 129 heights stay above mean sea level, flagged and
  counted. Correcting them needs an edge from `gungnir-node` to `gungnir-data` with
  `crs`, and libproj in the node image.
- **GAP-199.** A record with only I129/100, height above ground, is placed at height 0
  with the baseline vertical variance. The report says the height is not a measurement,
  and PN-04 says so, but the tracker still weighs it.
- **The residue between EGM2008 and a sender's own "mean sea level"** stays. No field in
  edition 1.2 says which model a sender used. The residue is stated on every corrected
  report, not removed.

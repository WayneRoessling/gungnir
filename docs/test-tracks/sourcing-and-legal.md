# Sourcing and legal policy

Status: first draft, 2026-09-04. This policy governs every figure in the test-track
suite. It was written first, per plan 07, and applies to the catalogue, the class
profiles, the sensor models, and the scenarios.

## 1. Open sources only

- Every figure (speed, altitude, endurance, signature class, emission behaviour,
  count in a scenario) comes from a publicly available source: a manufacturer's
  published specification, a public standard, a published analysis by a research
  institute or press organisation, or an encyclopaedic article that cites one of
  those. Nothing comes from controlled, classified, export-controlled, proprietary,
  or leaked material, and nothing comes from a person's non-public knowledge.
- Where public figures disagree, the catalogue records a range and the sources of
  both ends; it never picks a single "true" value.
- Where no public figure exists, the field is `unknown` and the class profile uses
  a conservative envelope with the reason stated. A blank is preferable to an
  invented number.

## 2. Source recording

Every platform entry in `catalogue.yaml` carries a `sources` list. Each source has:

- `ref`: the source's name and kind (for example "manufacturer public
  specification", "Wikipedia article, accessed 2026-09", "published open-source
  analysis"), specific enough for a reviewer to find it;
- `fields`: which figures it supports;
- `note`: any caveat (a claimed rather than observed figure, a figure for a
  different variant).

The rendered catalogue pages repeat the sources under each platform so that the
audit is possible from the documents alone.

## 3. Confidence marks

| Mark | Meaning |
|---|---|
| high | A manufacturer or public-standard figure, consistent across sources |
| medium | Multiple independent public reports agree within the stated range |
| low | A single report, a claimed figure, a derived estimate, or contested values |
| unknown | No public figure; the profile uses a conservative envelope |

Confidence is per platform entry for its kinematic envelope and, where a single
figure is much weaker than the rest, per field in the `note`. The class profile's
ranges are set so that every platform in the class, at every confidence, fits
inside them; the tracker is tested against the envelope, not against any one
platform's claimed number.

## 4. What is deliberately coarse

- Signatures are classes (radar cross-section: very small, small, medium, large;
  infrared: low, medium, high; acoustic: quiet, moderate, loud), never values in
  square metres or decibels. The sensor models use the class to pick a detection
  range band, which is all the tracker needs.
- Tactics are limited to what public reporting describes at the level of "streams
  along a river valley", "decoys mixed in", "shoot and move within minutes".
- Electronic-attack behaviour is modelled as its effect on sensors (clock skew,
  dropouts, false tracks), not as any system's actual technique.

## 5. Sides and platforms

The catalogue lists platforms in use by both belligerents because a tracker must
handle both; listing a platform is not a statement about its origin, ownership, or
legality of use. Where a platform's operator is publicly reported for both sides
(captured or supplied), the entry says `both`.

## 6. Export-control note

The compiled catalogue is a collection of published, approximate performance
figures organised for software testing. Compilations of public data are not
normally controlled, but a compilation can attract scrutiny that its parts do not.
Before the catalogue or the generated data is released outside the project (with
the product, to a customer, or publicly), plan 01 asks counsel whether an
export-control review is needed; until then the suite is internal to the project.
Nothing in the suite is derived from ITAR, EAR-controlled technical data, or any
national equivalent, and the `sources` records are the evidence.

## 7. Geography and identity

- Scenarios use the fictional Vell estuary from `../mission/vignettes.md`; no real
  site, route, launch area, or unit is described.
- Entities in generated data carry synthetic identifiers; no real airframe, hull,
  or vehicle identity appears.

## 8. Review and audit

- Each domain table is reviewed by a subject-matter reviewer named in
  `../mission/mission-analysis.md` §11; the review is recorded in
  `vehicle-catalogue.md`'s validation record with the date and the fields checked.
- `tools/build_catalogue.py` refuses to render a platform entry with a figure that
  has no source or no confidence mark; the CI check runs it.
- A figure found to be wrong is corrected with its source updated; the change is
  recorded in the catalogue's change log, never silently.

## 9. Exclusions

Not modelled, by policy: detailed signature values; specific electronic-attack
techniques; warhead, guidance, or seeker details; any figure whose only source is
non-public. The class profiles say where an exclusion limits the model's fidelity.

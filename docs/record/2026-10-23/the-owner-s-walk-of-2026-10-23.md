# The owner's walk of 2026-10-23

Five gaps filed on 2026-09-26 were each waiting on a design choice rather than on
engineering: GAP-194 and GAP-195 came out of GAP-106 and GAP-107, and GAP-198, GAP-199 and
GAP-200 out of GAP-196 and GAP-197. The owner walked them on 2026-10-23, four questions at
a time, each with a recommended default. Six decisions resulted, D-127 to D-132, because
GAP-200 split into the frame step and the older realisations. Nothing is built by this
change. Each gap stays open, its closing action now naming what to build.

| Gap | Decision | What the owner chose |
|---|---|---|
| GAP-200 | D-127 | EPSG's time-dependent "ITRF2014 to NAD83(2011) (1)" Helmert in place of PROJ's null step, at the picture's epoch (the current date, which a baseline field may pin), horizontal and vertical together. |
| GAP-200 | D-128 | Older realisations convert through NOAA's NADCON5 chain. An unqualified NAD83 file must be declared by the baseline or is refused. CORS96 is read as NSRS2007. |
| GAP-198 | D-129 | The node links `gungnir-data` with `crs` and corrects heights through the same verified grid as the desktop. |
| GAP-199 | D-130 | A height above ground is placed on a loaded DEM. Without one, it carries a vertical variance that says the height is unknown. |
| GAP-194 | D-131 | The node holds gap acceptances for its linked desktops, as it holds the queue (D-55). |
| GAP-195 | D-132 | The degraded-condition acknowledgement goes on the decision record, and `decide` enforces it for accept and override. |

## Facts the questions rested on

- **The node knows no laydown.** `gungnir-node/src/main.rs` never names one, and D-118
  re-opens an acceptance when the laydown changes. So D-131 has to tell the node which
  laydown is in force. It is part of that build, not a detail of it.
- **The node image has no libproj.** It is `debian:bookworm-slim`, and `proj-sys` needs
  libproj 9.6.2 or later, so D-129 builds PROJ from source in the image's build stage,
  as `ci.yml`'s `proj-crs` job already does. The runtime image then needs libtiff and
  `proj.db`.
- **The degraded tick gates accept only.** `gungnir-ui/src/panels/decision_dialog.rs`
  `accept_enabled` checks `degraded_acknowledged`, and `override_enabled` does not. D-119
  made the rehearsal acknowledgement gate override too, on the grounds that an override
  acts on a plan the way an acceptance does. D-132 applies the same reasoning.
- **Autzen is unqualified NAD83.** The fixture's WKT is `NAD83 / Oregon GIC Lambert (ft)`,
  EPSG:2992, as most United States LIDAR is labelled. Under D-128 its tests declare a
  realisation.
- **PROJ-data's NADCON5 chain** for the conterminous United States runs NAD83(1986) to
  HARN, HARN to FBN, FBN to NSRS2007, and NSRS2007 to 2011. That is four files totalling
  95 308 700 bytes, from `cdn.proj.org/files.geojson` as read on 2026-10-23. There is no
  CORS96 file.

## A figure corrected during the walk

The GAP-200 question said that choosing the wrong epoch costs about a centimetre a year.
Measured afterwards with pyproj 9.8.1 (PROJ 9.8.1) at Autzen (123.075 W, 44.05 N, 100 m),
the Helmert moves a point 1.399 m horizontally and -0.384 m vertically at epoch 2010.0,
and 1.596 m and -0.361 m at 2026.8. The two epochs differ by 0.337 m horizontally, about
2 cm a year. That is ITRF moving relative to the plate, and it is the error a fixed
epoch would carry. Plate-interior deformation is what remains *at* the right epoch, and
it is the smaller term. The correction strengthens the option chosen: with a fixed
2010.0 default, a picture today would carry a third of a metre.

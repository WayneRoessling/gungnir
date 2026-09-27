# Deployment

Gungnir ships as two binaries built from one crate set (`ARCHITECTURE.md` §8):

| Binary | Profile | Host | Build |
|---|---|---|---|
| `gungnir-app` | Disconnected desktop, or the desktop half of a connected profile | Windows 11 with a discrete GPU | `cargo build --release -p gungnir-app` |
| `gungnir-node` | On-prem or cloud service node | Linux x86_64 container | `deploy/node/Dockerfile` |

## Service node container

```bash
docker build -f deploy/node/Dockerfile -t gungnir-node:dev .
```

```bash
docker run --rm -p 7410:7410 -v gungnir-data:/var/lib/gungnir gungnir-node:dev
```

That compiles the node inside the image, with `--locked`, and is the build that ships.
A binary built elsewhere can be put into the same runtime stage instead -- `ci.yml`
does this on every pull request with a binary it compiles in the same `rust` image on
the runner with its cargo cache mounted in, so the image check takes seconds rather
than a cold six-minute compile:

```bash
docker buildx build -f deploy/node/Dockerfile -t gungnir-node:dev --build-context build=<dir> .
```

where `<dir>/out/gungnir-node` is the binary. The named context replaces the
Dockerfile's `build` stage, so nothing is compiled; the runtime stage is the same
either way, and the binary has to be linked against `debian:bookworm-slim`'s glibc
(2.36): one built natively on Ubuntu 24.04 needs GLIBC 2.38 and 2.39 and does not
load. `ci.yml` still compiles from source on main and on any pull request that
changes a manifest, the lockfile, the toolchain file, or this directory.

The same applies to the `gungnir-node` binary `release.yml` publishes as a bare
artifact: it is built natively on `ubuntu-latest`, so it runs on hosts with that
runner's glibc or newer, not on Debian bookworm; the container image is the build
for older hosts.

The image runs as an unprivileged user, keeps the journal on the
`/var/lib/gungnir` volume, and reads `/etc/gungnir/config.json`
(`deploy/node/config.example.json` is baked in as a starting point; mount your own
over it). Port 7410 is reserved for `gungnir-api`; the transport is not implemented
yet, so nothing listens on it in this build and the node logs that at startup.

## Desktop pointing at a node

Set `backend` in the desktop's config baseline and point `GUNGNIR_CONFIG` at the
file:

```json
{ "version": 1, "backend": { "kind": "remote", "endpoint": "http://node.local:7410" } }
```

If the node cannot be reached the desktop falls back to the embedded services and
shows an alert (`ARCHITECTURE.md` §8.4).

**State how long a delegation survives a lost node.** A desktop cut off from its node
keeps the delegations in force when the node went silent for
`policy.delegation.disconnected_lapse_s` seconds and then lets them lapse, and makes no new
one while cut off (D-15; `docs/design/DN-31-node-approval-queue.md` §6.7 and §7). The
setting has **no default**: leave it out and no delegation survives the disconnection at
all, which is the strictest reading rather than an interval this build chose for you. A
stated value must be finite and positive, or the baseline is refused.

```json
{
  "version": 1,
  "backend": { "kind": "remote", "endpoint": "http://node.local:7410" },
  "policy": { "delegation": { "disconnected_lapse_s": 300 } }
}
```

When the node answers again the desktop reconciles the outage on PN-18 -- two engagements
of one track on the two sides are shown first, for a person -- and forwards every decision
it took while cut off to the node, which puts each on its record once.

## The geoid grids

A DEM or point cloud in a real-world coordinate reference system has its heights made
WGS-84 ellipsoidal ones before they are placed (GAP-108 and D-121; GAP-197 and D-125).
A height the file or the baseline states as ellipsoidal needs nothing. A height above a
geoid needs that geoid's grid, which is installed beside a deployment rather than shipped
in the repository. Three are pinned, each by the SHA-256 PROJ-data publishes for it in
[`geoid/SHA256SUMS`](geoid/SHA256SUMS):

| Heights | EPSG | Grid | Bytes | Covers |
|---|---|---|---|---|
| EGM2008 height | 3855 | `us_nga_egm08_25.tif` (NGA, 2.5') | 80 585 622 | the globe |
| EGM96 height (SRTM, most DTED) | 5773 | `us_nga_egm96_15.tif` (NGA, 15') | 2 710 815 | the globe |
| NAVD88 height (also 6360 in US survey feet, 8228 in feet) | 5703 | `us_noaa_g2018u0.tif` (NOAA GEOID18, 1') | 16 742 155 | the conterminous United States |

All three are public domain ("Derived from work by NGA" or "by NOAA", in each file).
Install the ones your data needs; a missing grid refuses only the heights that need it.
Every other vertical datum is refused by name, and so is a NAVD88 height outside
GEOID18's grid: Alaska and Hawaii NAVD88 is NOAA's GEOID12B, which is not pinned.

**What a NAVD88 height is worth.** GEOID18 turns a NAVD88 height into a NAD83(2011)
ellipsoidal height, and the desktop reads that as a WGS-84 one, the same null step PROJ
itself takes for NAD83 horizontally. The two frames differ by one to two metres
horizontally and up to a metre vertically across the United States (1.4 m and -0.38 m at
the Autzen fixture in Oregon), so a NAVD88 surface lands to the metre in WGS-84 terms,
not to the centimetre. EGM2008 and EGM96 heights land to the grids' own interpolation,
centimetres.

Fetch the grids from PROJ's own CDN and check them before use:

```bash
mkdir -p /opt/gungnir/geoid && cd /opt/gungnir/geoid
for grid in us_nga_egm08_25.tif us_nga_egm96_15.tif us_noaa_g2018u0.tif; do
  curl -fsSLO "https://cdn.proj.org/$grid"
done
sha256sum -c /path/to/gungnir/deploy/geoid/SHA256SUMS
```

```powershell
New-Item -ItemType Directory -Force C:\ProgramData\Gungnir\geoid | Out-Null
foreach ($grid in 'us_nga_egm08_25.tif', 'us_nga_egm96_15.tif', 'us_noaa_g2018u0.tif') {
  Invoke-WebRequest "https://cdn.proj.org/$grid" -OutFile "C:\ProgramData\Gungnir\geoid\$grid"
  (Get-FileHash "C:\ProgramData\Gungnir\geoid\$grid" -Algorithm SHA256).Hash
}
```

Leave out any grid you do not need; `sha256sum -c --ignore-missing` then checks the ones
you fetched. Each PowerShell hash must equal the one in `geoid/SHA256SUMS` (it prints in
capitals). For a disconnected desktop, fetch and check them on a connected machine and
carry the files across; the desktop never fetches a grid itself.

Then name the directory in the baseline:

```json
{ "version": 1, "geoid_grid_dir": "/opt/gungnir/geoid" }
```

Without `geoid_grid_dir` the desktop looks in `PROJ_DATA` (then the older `PROJ_LIB`),
the directories PROJ itself reads, grid by grid. At start it hashes each file it finds
off the render thread, and PN-09 gives one line per grid saying which of these holds:

- **verified**, with the path and where it was named;
- **refused**, with the reason -- missing, not the pinned file, or unreadable -- and an
  alert;
- **none installed**.

A grid is used only when it is verified. A `geoid_grid_dir` that holds none of the three
is taken for a mistake: every grid is refused as missing, with one alert naming the
directory. One that holds some has just not installed the rest. Without a height's grid,
the terrain or point-cloud pair that states that height is refused and not drawn. It is
never converted with another model's separation, and never used unconverted.

Two notes on scope:

- **Only a desktop built with `gungnir-data`'s `crs` feature converts a real-world CRS at
  all.** That feature links libproj, and libtiff too for the grids; D-51 keeps it off by
  default. The Windows desktop `release.yml` publishes is built without it, so it
  refuses every real-world CRS by name whether or not a grid is installed, and PN-09 says
  that too.
- **The node does not convert terrain or point clouds**, so a node image needs no grid.

**A UAS's height above mean sea level uses the same grid (GAP-196, D-123, D-124).** An
ASTERIX Category 129 report states its height only above mean sea level (I129/090). On
a `crs` desktop with the grid verified, every bound radar feed adds the EGM2008
separation at the report's position, so the UAS is placed at a WGS-84 ellipsoidal
height like everything else. Without the grid, on a desktop built without `crs`, and on
the node (which links no libproj; GAP-198), the height is kept as mean sea level and
flagged, never passed off as ellipsoidal:

- PN-03 marks the track's U "MSL", and PN-04 says why the height is uncorrected;
- the detection's provenance says so, and its vertical variance is widened by the square
  of EGM2008's largest separation, 106.91 m;
- PN-09's radar-feed line, and the node's health line, count such heights.

A DEM whose `GeoTIFF` keys state no vertical system, which is most of them, needs
`terrain.vertical` declared: `"ellipsoidal"`, `"epsg:3855"`, `"epsg:5773"` or
`"epsg:5703"`. A point-cloud pair that states none -- a WKT with no `VERT_CS`, a LAS
1.0-1.3 file with no `VerticalGeoKey` -- needs `point_cloud.vertical`. Where the file
does state one, a baseline that contradicts it is refused. A DEM's heights must be
metres (a `VerticalGeoKey` of 6360 or 8228 is refused at load); a point cloud's are
scaled from the unit the file states.

### Pinning a further geoid grid

A deployment whose data is in another vertical datum -- NAVD88 in Alaska (GEOID12B), a
European national datum, a local one -- is refused by name until that datum's grid is
pinned. Pinning one is a change to this repository, reviewed like any other, never a
file dropped beside a deployment:

1. **Find the grid in PROJ-data**, the PROJ project's own collection. Its index,
   `https://cdn.proj.org/files.geojson`, lists every file with its `source_crs_code`,
   `target_crs_code`, `type`, `area_of_use`, `file_size` and `sha256sum`. The grid must be
   of type `VERTICAL_OFFSET_GEOGRAPHIC_TO_VERTICAL` and its `target_crs_code` must be the
   vertical CRS your data states. Note its `source_crs_code`: if it is not WGS 84 (4979),
   the ellipsoidal heights it gives are in that frame, and the difference from WGS 84
   must be stated in the decision, as D-125 does for GEOID18's NAD83(2011).
2. **Download it from `cdn.proj.org` and check its SHA-256** against the index; they must
   match. Record the decision (`docs/mission/gap-analysis/data/decisions.yaml`): which
   datum, which file, why this one, and what its source frame leaves.
3. **Pin it in two places a test holds equal**: a line in `geoid/SHA256SUMS`, and a
   `GeoidModel` variant in `gungnir-data/src/geoid.rs` with the file, URL, digest and
   size, mapped from its vertical CRS codes in `VerticalDatum::from_epsg` (and its datum
   code in `from_datum_epsg`, and each code's unit in `vertical_crs_unit_metres`).
4. **Test it the way the three above are**: a clip of the grid cut by GDAL by node
   offsets, committed under `testdata/geoid/` with its provenance and digest in
   `testdata/geoid/SOURCE.md`, and undulations and a conversion held to a micrometre
   against an independent `pyproj` run with the full grid on its path. `ci.yml`'s
   `proj-crs` job fetches every grid the manifest lists and runs the full-grid tests.
5. **Install it** as above; PN-09 gains its line.

## Cloud

The same image runs unchanged in a cloud container platform. Differences from
on-prem are posture, not code: encryption at rest for the journal volume, key
management off-host, and a stricter `gungnir-security` configuration
(`ARCHITECTURE.md` §8.5). Kubernetes manifests are added here once the API
transport exists and there is something to expose.

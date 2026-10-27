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

That compiles the node inside the image, with `--locked` and its `crs` feature, and is
the build that ships. libproj is built from source in the Dockerfile's `toolchain` stage,
which makes a cold build several minutes longer; "The node's geoid grid" below says what
the feature is for. A binary built elsewhere can be put into the same runtime stage
instead -- `ci.yml` does this on every pull request with a binary it compiles in that
same `toolchain` stage on the runner with its cargo cache mounted in, so the image check
takes seconds rather than a cold compile:

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
over it). `gungnir-api` is served on `node.bind_addr`, which the example config sets to
`0.0.0.0:7410`. That is not a loopback address, so the node serves it only under mutual
TLS: set `GUNGNIR_TLS_CERT`, `GUNGNIR_TLS_KEY` and `GUNGNIR_TLS_CLIENT_CA`, or
`GUNGNIR_TLS_CLIENT_CA` alone to serve with the identity the node issues itself (the
machine-identity section below). Without them the node logs that it is not serving and
keeps tracking and journalling, rather than listening in plaintext
(`gungnir-node/src/main.rs`, `spawn_transport`). A loopback `bind_addr` is served in
the clear.

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

## Provisioning operator accounts

Both binaries carry the same `account` commands (GAP-204, one implementation in
`gungnir_security::provisioning`), so a desktop with no node provisions itself:

```bash
printf '%s' 'the passphrase' | gungnir-app account add accounts.json 7 commander
```

```bash
gungnir-app account list accounts.json
```

`gungnir-node account ...` is the same, speaking as the node. The passphrase is read from
standard input and never from an argument, because arguments are visible to every process
on the host; its echo is not suppressed, so pipe it in rather than type it where somebody
can read the screen. `add` refuses an operator that already has an account unless
`--replace` is given, lists never show the hash, and every change writes one
`account.assign_role` entry to the `audit` directory beside the file, attributed to nobody
because the command runs outside any session (DN-23 §5). `add-os-keystore` and
`list-os-keystore` seal the accounts under the operating system's keystore instead of a
file (D-39). On Windows the file inherits its directory's permissions; the command says so.

Point the baseline at the file with `security.authentication.local_accounts.accounts_path`
and sign in on PN-20. The roles are `operator`, `supervisor`, `analyst`, `sensor-manager`,
`administrator`, `commander`, `planner`, `security-officer` and `intelligence-analyst`.
## Time on a site with no NTP or GNSS

Mission time is the host's clock unless the baseline says otherwise. A laptop with no NTP
and no GNSS time drifts against a GPS-disciplined radar, and once the drift passes the
late-data buffer (one second by default) the tracker drops that radar's reports as late
and PN-09 calls its clock out of sync. Two remedies, in order of preference (GAP-205):

**Discipline the clock from a sensor.** Name a configured sensor whose reports carry its
own clock's time, a radar's GNSS-disciplined time of day for instance:

```json
{ "version": 1, "time": { "discipline": { "sensor": 4 } } }
```

Mission time then follows that sensor's time stamps, to within its least transit delay.
Until the sensor is first heard, mission time is the host clock and PN-09 says
`waiting`; it locks on the first report, even one the gateway refused because the host
is behind; it goes into `holdover` after 30 s of silence and keeps the last offset; and it
refuses an offset larger than an hour, because a mission clock stepped by hours would move
every deadline with it. **Set the host clock to within an hour before starting.** Each
change of state is on the journal as `HealthEvent::ClockDiscipline` and in the alerts. Both
the desktop and the node honour the setting; the sensor must be one the baseline
configures.

**Or widen the late-data buffer.** A host with no time source and no discipline should run

```json
{ "version": 1, "time": { "late_data": { "kind": "buffer-and-reorder", "max_lateness_s": 5.0 } } }
```

which tolerates five seconds of drift before reports are dropped, at the cost of holding
every report up to five seconds for reordering. The default stays one second (D-98, D-99);
a site with a time source should leave it there.

## Machine identities

Every node and desktop presents its own certificate, named from a fingerprint of its own
key, and each certificate is limited to one role (D-135):

| Machine and role | Name | Key usage | Pin it from |
|---|---|---|---|
| A node serving | `node-<fingerprint>` | serverAuth | `node-identity.pem` beside the node's journal |
| A node on its peer links | `node-peer-<fingerprint>` | clientAuth | `node-peer-identity.pem` beside the node's journal |
| A desktop | `desktop-<fingerprint>` | clientAuth | the desktop's certificate (GAP-141) |

A name with `-ephemeral-` in it belongs to a key that does not survive a restart, because
the machine's baseline does not use the operating system's keystore
(`security.key_provider`) or the keystore was unreachable. Pin only the others.

A node's peer identity is a key of its own, never its serving key. Pin it in the
client-authority file of each peer that accepts it, and name it in that peer's agreement
and `machine_identities` by its `node-peer-` name. A deployment that runs its own
certificate authority sets `GUNGNIR_TLS_CERT` and `GUNGNIR_TLS_KEY` on the node instead,
and that certificate is what its peers see.

**Upgrading from before D-135:** a node's serving certificate was named `gungnir-node`, and
its peer-link certificate was made fresh at every start, so no peer could pin it. Re-pin
each node's serving certificate once, and pin its peer certificate for the first time.

## A SAPIENT effector

A resource can hand its engagements to an effector that speaks SAPIENT, through the same
middleware a SAPIENT sensor's feed uses (GAP-207). The desktop and the node both do it, and
the effector's answers come back on the connection they open, so a desktop with no node
still learns the outcome.

```json
{
  "version": 1,
  "sapient_node_id": "a1b2c3d4-0000-0000-0000-000000000000",
  "endpoints": [{ "name": "jammer", "kind": "sapient", "address": "10.0.0.20:7600" }],
  "sapient_effectors": [
    { "endpoint": "jammer", "destination_id": "7c0e6a2b-1d3f-4e5a-9b8c-0d1e2f3a4b5c", "node_type": "jammer" }
  ],
  "resources": [
    { "id": 1, "position": [0.9599, 0.2094, 0.0], "capacity": 2, "layer": "point",
      "handoff_endpoint": "jammer", "intercept_speed_mps": 300.0 }
  ]
}
```

- **`address`** is the middleware's `ip:port`, serving the protobuf-JSON mapping one message
  per line.
- **`destination_id`** is the effector's own SAPIENT node id. Answers from any other node
  are refused.
- **`node_type`** is `kinetic`, `jammer`, `cyber`, `ldew` or `rfdew`. An effector that
  registers as another type is refused.
- **`intercept_speed_mps`** must be set on the resource. The task says where to engage,
  and with no solved intercept point the handoff is refused rather than sent.

The effector is sent a `Task` with `command.request` set to `engage`. BSI Flex 335 v2.0 has
no engage command, so the effector's integrator has to accept that word. One that does not
rejects the task, and PN-06 shows the refusal. An acceptance reads as acknowledged, a
completion as finished with no effect stated, and a failure as ineffective. A middleware
that is down at start is said and connected again when the first handoff is sent.

## Releasing tracks to a partner

Nothing leaves a deployment until its baseline says so. Two gates decide whether a track
reaches a partner, and both must agree (DN-18 §5):

1. **The exchange agreement** says what kind of thing the partner may receive. A partner
   with no agreement, or one without `tracks` outbound, receives no track.
2. **The marking** says whether this particular track may go to that partner. A track is
   marked by the most restrictive of the sources that fed it (DN-17 §5, GAP-206).

The `releasability` section sets the markings. Its default applies to every sensor with no
row of its own, and is `internal` when absent:

```json
{
  "version": 1,
  "releasability": {
    "default": { "kind": "internal" },
    "sensors": [
      { "sensor": 4, "marking": { "kind": "parties", "parties": ["node-peer-3f9a..."] } },
      { "sensor": 7, "marking": { "kind": "all-peers" } }
    ]
  }
}
```

The three markings are `internal`, `parties` with the names that may receive it, and
`all-peers`. A named party must be a peer, an endpoint or an exchange agreement's party in
the same baseline, so a misspelt name is refused at load rather than releasing nothing.
A track once fed by an `internal` sensor stays internal for the rest of its life, because
its estimate carries what that sensor saw.

**A track a peer fed is always internal.** A partner's tracks arrive as a peer source, and
releasing them again would send each one back to where it came from, as a new source to
fuse with itself. A `peers` row in this section may only say `internal`, until the
re-ingest guard (GAP-218) is built.

A partner that receives a filtered picture is told how many items were withheld, so it
knows its picture is partial.

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
ellipsoidal height, and since GAP-200 (D-127) the desktop carries that to WGS 84 by EPSG's
time-dependent ITRF2014 to NAD83(2011) transformation at the picture's epoch, so it lands
to GEOID18's own centimetres rather than the metre the null step left (-0.38 m at the
Autzen fixture). EGM2008 and EGM96 heights land to the grids' own interpolation,
centimetres. "NAD83 positions and the frame epoch" below has the horizontal half.

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

A grid is used only when it is verified. A `geoid_grid_dir` that holds none of the pinned
grids is taken for a mistake: every grid is refused as missing, with one alert naming the
directory. One that holds some has just not installed the rest. Without a height's grid,
the terrain or point-cloud pair that states that height is refused and not drawn. It is
never converted with another model's separation, and never used unconverted.

Two notes on scope:

- **Only a desktop built with `gungnir-data`'s `crs` feature converts a real-world CRS at
  all.** That feature links libproj, and libtiff too for the grids; D-51 keeps it off by
  default. The Windows desktop `release.yml` publishes is built without it, so it
  refuses every real-world CRS by name whether or not a grid is installed, and PN-09 says
  that too.
- **The node does not convert terrain or point clouds**, so it needs no grid but
  EGM2008, and that only for UAS heights (below).

**A UAS's height above mean sea level uses the same grid (GAP-196, D-123, D-124).** An
ASTERIX Category 129 report states its height only above mean sea level (I129/090). On
a `crs` desktop, and on the node image (GAP-198, D-129), with the grid verified, every
bound radar feed adds the EGM2008 separation at the report's position, so the UAS is
placed at a WGS-84 ellipsoidal height like everything else. Without the grid, on a
desktop built without `crs`, and on a node binary built without it, the height is kept as
mean sea level and flagged, never passed off as ellipsoidal:

- PN-03 marks the track's U "MSL", and PN-04 says why the height is uncorrected;
- the detection's provenance says so, and its vertical variance is widened by the square
  of EGM2008's largest separation, 106.91 m;
- PN-09's radar-feed line, and the node's health line, count such heights.

### The node's geoid grid

The node image is built with `gungnir-node`'s `crs` feature (GAP-198, D-129), so it reads
the pinned EGM2008 grid the same way the desktop does: from the directory the baseline's
`geoid_grid_dir` names, else `PROJ_DATA`, verified against its digest at start. The grid
is never in the image. Fetch and check `us_nga_egm08_25.tif` as above, mount its directory
read-only, and name it in the node's baseline:

```bash
docker run --rm -p 7410:7410 -v gungnir-data:/var/lib/gungnir \
  -v /opt/gungnir/geoid:/opt/gungnir/geoid:ro gungnir-node:dev
```

```json
{ "version": 1, "geoid_grid_dir": "/opt/gungnir/geoid" }
```

The node says at start, and on every health line, which of these holds: **verified**,
with the path and the start of its digest; **refused**, with the reason -- missing, not
the pinned file, or unreadable by PROJ; **none installed**; or, for a node binary built
without the feature, **not read**. `release.yml`'s bare Linux binary is built without
it, so only the image corrects UAS heights; the bare binary keeps D-124's flag.

A DEM whose `GeoTIFF` keys state no vertical system, which is most of them, needs
`terrain.vertical` declared: `"ellipsoidal"`, `"epsg:3855"`, `"epsg:5773"` or
`"epsg:5703"`. A point-cloud pair that states none -- a WKT with no `VERT_CS`, a LAS
1.0-1.3 file with no `VerticalGeoKey` -- needs `point_cloud.vertical`. Where the file
does state one, a baseline that contradicts it is refused. A DEM's heights must be
metres (a `VerticalGeoKey` of 6360 or 8228 is refused at load); a point cloud's are
scaled from the unit the file states.

A UAS that reports its height only above ground (ASTERIX Category 129's I129/100, with no
I129/090) is placed on the desktop's DEM, at the ground's height plus its own, when the
DEM covers it and the baseline states the DEM's vertical error, one standard deviation in
metres, as its producer gives it (GAP-199, D-130):

```json
{ "terrain": { "path": "...", "frame": "epsg:32633", "vertical": "epsg:3855",
               "vertical_sigma_m": 4.0 } }
```

Without that error, without a loaded DEM, off its edge, and always on the node, which
loads no terrain, such a height is carried as unknown: held at 0, marked on PN-03, counted
on PN-09, and weighed as unknown to within 10 km.

### NAD83 positions and the frame epoch

A DEM or point cloud whose horizontal system is NAD83 -- most United States data -- is
carried to WGS 84 by its realisation and the picture's epoch (GAP-200, D-127, D-128), not
by PROJ's null step, which left it one to two metres off:

- **NAD83(2011)** goes straight to WGS 84 by EPSG's ITRF2014 to NAD83(2011) Helmert at the
  epoch, WGS 84 read as ITRF2014 to centimetres. No grid is needed.
- **An older realisation** is first carried to NAD83(2011) through NOAA's NADCON5 grids
  for the conterminous United States, one per step, then the same Helmert:

  | Step | Grid | Bytes |
  |---|---|---|
  | NAD83(1986) to NAD83(HARN) | `us_noaa_nadcon5_nad83_1986_nad83_harn_conus.tif` | 3 522 682 |
  | NAD83(HARN) to NAD83(FBN) | `us_noaa_nadcon5_nad83_harn_nad83_fbn_conus.tif` | 9 111 570 |
  | NAD83(FBN) to NAD83(NSRS2007) | `us_noaa_nadcon5_nad83_fbn_nad83_2007_conus.tif` | 42 709 110 |
  | NAD83(NSRS2007) to NAD83(2011) | `us_noaa_nadcon5_nad83_2007_nad83_2011_conus.tif` | 39 965 338 |

  A file in NAD83(HARN) needs the last three; one in NAD83(NSRS2007) the last one.
  NAD83(CORS96) has no grid of its own and is read as NSRS2007, which held the CORS at
  their CORS96 coordinates. The grids are pinned in `geoid/SHA256SUMS` beside the geoid
  ones, installed in the same directory, verified the same way, and listed on PN-09 the
  same way. A position outside them is refused by name.
- **A file in the unqualified NAD83** (EPSG datum 6269, the code most US LIDAR carries)
  is ambiguous: the code names the 1986 adjustment, but most surveys labelled with it are
  NAD83(2011), and the two sit up to a metre apart. Declare which it is, by the
  realisation's geographic code, or the file is refused by name:

  ```json
  { "terrain": { "path": "...", "frame": "epsg:26910", "vertical": "epsg:5703",
                 "nad83_realisation": "epsg:6318" } }
  ```

  `"epsg:6318"` is NAD83(2011), `"epsg:4759"` NSRS2007, `"epsg:8860"` FBN, `"epsg:4152"`
  HARN, `"epsg:6783"` CORS96 and `"epsg:4269"` the 1986 adjustment. `point_cloud` takes
  the same field. A file that names its realisation is taken at its word, and a
  declaration that contradicts it is refused.

**The epoch is the picture's.** A NAD83(2011) point's coordinates do not change, while its
place in WGS 84 moves about 2 cm a year, and the tracks a surface is drawn against are
measured now. So the epoch is the current date unless the baseline pins one, for a
replay or a test that must give the same answer every year:

```json
{ "version": 1, "frame_epoch": 2026.5 }
```

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
(`ARCHITECTURE.md` §8.5). No Kubernetes manifests are kept here; a deployment gives the
container what the `docker run` line above does: the journal volume, port 7410 and the
TLS environment.

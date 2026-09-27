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

## The EGM2008 geoid grid

A DEM or point cloud in a real-world coordinate reference system has its heights made
WGS-84 ellipsoidal ones before they are placed (GAP-108, D-121). A height the file or the
baseline states as ellipsoidal needs nothing. A height stated as **EGM2008 height**
(EPSG:3855) needs NGA's EGM2008 geoid grid, which is too large to ship in the repository
and is installed beside a deployment instead. Any other vertical datum is refused by name.

The grid is PROJ-data's GeoTIFF of it, pinned by SHA-256 in
[`geoid/SHA256SUMS`](geoid/SHA256SUMS) (80 585 622 bytes; public domain, derived from
NGA's work). Fetch it from PROJ's own CDN and check it before use:

```bash
mkdir -p /opt/gungnir/geoid && cd /opt/gungnir/geoid
curl -fsSLO https://cdn.proj.org/us_nga_egm08_25.tif
sha256sum -c /path/to/gungnir/deploy/geoid/SHA256SUMS
```

```powershell
New-Item -ItemType Directory -Force C:\ProgramData\Gungnir\geoid | Out-Null
Invoke-WebRequest https://cdn.proj.org/us_nga_egm08_25.tif -OutFile C:\ProgramData\Gungnir\geoid\us_nga_egm08_25.tif
(Get-FileHash C:\ProgramData\Gungnir\geoid\us_nga_egm08_25.tif -Algorithm SHA256).Hash
```

The PowerShell hash must equal the one in `geoid/SHA256SUMS` (it prints in capitals).
For a disconnected desktop, fetch and check it on a connected machine and carry the
file across; the desktop never fetches it itself.

Then name the directory in the baseline:

```json
{ "version": 1, "geoid_grid_dir": "/opt/gungnir/geoid" }
```

Without `geoid_grid_dir` the desktop looks in `PROJ_DATA` (then the older `PROJ_LIB`),
the directories PROJ itself reads. At start it hashes the file off the render thread, and
PN-09 says which of these holds:

- **verified**, with the path and where it was named;
- **refused**, with the reason -- missing, not the pinned file, or unreadable -- and an
  alert;
- **none installed**.

The grid is used only when it is verified. Without it an EGM2008 height is refused, and
the terrain or point-cloud pair that states one is not drawn. It is never converted with
some other separation, and never used unconverted.

Two notes on scope:

- **Only a desktop built with `gungnir-data`'s `crs` feature converts a real-world CRS at
  all.** That feature links libproj, and now libtiff too for the grid; D-51 keeps it off
  by default. The Windows desktop `release.yml` publishes is built without it, so it
  refuses every real-world CRS by name whether or not a grid is installed, and PN-09 says
  that too.
- **The node does not convert terrain or point clouds**, so a node image needs no grid.

A DEM whose `GeoTIFF` keys state no vertical system, which is most of them, needs
`terrain.vertical` declared: `"ellipsoidal"` or `"epsg:3855"`. A point-cloud pair whose
WKT names no `VERT_CS` needs `point_cloud.vertical`. Where the file does state one, a
baseline that contradicts it is refused.

## Cloud

The same image runs unchanged in a cloud container platform. Differences from
on-prem are posture, not code: encryption at rest for the journal volume, key
management off-host, and a stricter `gungnir-security` configuration
(`ARCHITECTURE.md` §8.5). Kubernetes manifests are added here once the API
transport exists and there is something to expose.

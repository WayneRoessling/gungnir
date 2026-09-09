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
does this on every pull request with the binary its own cached build just produced,
so the image check takes seconds rather than a cold six-minute compile:

```bash
docker buildx build -f deploy/node/Dockerfile -t gungnir-node:dev --build-context build=<dir> .
```

where `<dir>/out/gungnir-node` is the binary. The named context replaces the
Dockerfile's `build` stage, so nothing is compiled; the runtime stage is the same
either way, and the binary has to run on `debian:bookworm-slim`'s glibc. `ci.yml`
still compiles from source on main and on any pull request that changes a manifest,
the lockfile, the toolchain file, or this directory.

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

## Cloud

The same image runs unchanged in a cloud container platform. Differences from
on-prem are posture, not code: encryption at rest for the journal volume, key
management off-host, and a stricter `gungnir-security` configuration
(`ARCHITECTURE.md` §8.5). Kubernetes manifests are added here once the API
transport exists and there is something to expose.

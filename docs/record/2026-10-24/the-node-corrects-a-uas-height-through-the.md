# The node corrects a UAS height through the verified grid

GAP-198, built to D-129 (the owner's walk of 2026-10-23). The node binds the same ASTERIX
Category 129 feeds as the desktop. Until now it served every UAS height above mean sea
level uncorrected, flagged under D-124 and about 35 m off over the Baltic, because it had
no edge to the crate that reads the geoid grid.

## What was built

- **Edge (ab), `gungnir-node` to `gungnir-data`.** It is shown in `ARCHITECTURE.md` §7's
  table and noted under §7.1, recorded in `docs/design/dependency-edges.md` §20, and
  named among `dependency_graph.rs`'s recorded edges, all in this change. §8.3's row now
  says the node takes `gungnir-data` for its geoid module only.
- **Optional, behind a new `crs` feature on the node.** The feature turns on
  `gungnir-data` and its own `crs` together. libproj cannot be built on Windows MSVC, and
  `cargo check --workspace` compiles the node there, so a non-optional edge would have
  broken the workspace's default build on Windows. A node built without the feature keeps
  D-124's path, and its health line says it was built without the grid.
- **`node_geoid`, at start.** It finds the EGM2008 grid by the desktop's rule, verifies
  its digest, starts one PROJ lookup service, and lends it to every radar feed through
  the `GeoidHandle` each one already holds. Otherwise every feed holds the reason. The
  start-up log and every "node health" line carry a `geoid` field: verified, refused,
  none installed, or not read.
- **One rule for finding a grid.** The desktop's private `candidate` moved into
  `gungnir_data::geoid::locate`, so both binaries find a grid the same way: the named
  directory, which is final, else `PROJ_DATA`.
- **The node image.** `deploy/node/Dockerfile` gains a `toolchain` stage: the Rust image
  plus cmake, make, g++, pkg-config, SQLite and libtiff, since `proj-sys` builds libproj
  from source and bookworm's own is older than it requires. The build stage compiles the
  node with `--features crs`. The runtime stage adds libtiff6, libsqlite3-0 and
  libstdc++6, and a `/opt/gungnir/geoid` mount point. The grid is never in the image.
- **CI.**
  - **The container job** builds the `toolchain` stage first and compiles the node in it
    with `crs`. Its cache key changes, because what it puts in `target/` changes shape.
    Its existing start-up check now also proves the runtime stage carries libproj's
    libraries.
  - **`proj-crs`** checks and lints the node with `crs`, and runs its tests.

## Choices stated

- **No proj.db in the image.** The node hands PROJ one written-out `vgridshift` pipeline
  over the verified file's absolute path. PROJ builds that without its database, and the
  node converts no system by code. A later node feature that does would need the
  database added.
- **The released bare Linux binary stays without `crs`.** It is built natively on the
  runner, and with libproj it would need libtiff and SQLite on the host too. D-129 names
  the image, and `deploy/README.md` says the bare binary keeps D-124's flag.
- **The grid is checked synchronously at start.** That is about a fifth of a second for
  the 80 MB file in release, before any feed is bound. So no report is placed before the
  answer is known, and nothing needs re-lending later, unlike the desktop, whose check
  runs off the render thread.

## How it was checked

- **Default build, on this machine:**
  - `gungnir-node`'s `a_node_with_no_grid_lends_none_and_says_why`, and its existing
    health-line test.
  - `dependency_graph.rs`, with (ab) recorded and checked against `ARCHITECTURE.md`.
  - A workspace `cargo check` with warnings as errors.
- **Not compiled on this machine:** the `crs` build and the image. libproj cannot be built
  on Windows MSVC, and the image build needs packages fetched inside the container. They
  run in CI: `proj-crs` runs `a_verified_grid_is_lent_to_the_feeds_and_a_wrong_one_is_refused`,
  where the committed EGM2008 clip gives 34.9247587 m at 54.148 N, 15.0 E, as `pyproj`
  does over the full grid. The container job builds the image and starts it.

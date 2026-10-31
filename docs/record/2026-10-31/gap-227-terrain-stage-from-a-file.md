# GAP-227: terrain stage, from a file

The fourth increment of `docs/design/DN-36-staged-terrain-and-map-tiles.md`, its first half:
amendment 1 item 2's `terrain stage` subcommand (D-162), staging from a DEM file already on
the machine. Fetching the source from a service, and the fill cache (D-160), need an HTTP
client with TLS, which is a §2.9 decision. Both wait on the owner's answer to that.

**One implementation in both binaries.** `gungnir_data::terrain_store::cli::run` is called by
`gungnir-app terrain ...` and `gungnir-node terrain ...`, as `account` is one implementation
behind both. Each binary answers it before any state is built and exits. In the desktop, the
five commands that run and exit (`account`, `audit-key`, `baseline-key`, `baseline` and now
`terrain`) moved into one `subcommand` function. Their behaviour is unchanged, and `main` is
back under clippy's length limit. The node's edge (ab) to `gungnir-data` is now
unconditional, because the command needs no libproj; its `crs` feature turns on only
`gungnir-data/crs`. `docs/design/dependency-edges.md` §20 says so. No letter is added, and the
dependency graph test is unchanged.

**What it does.** Every argument is checked before anything is read: the frame's grammar, a
digest of 64 hexadecimal digits, a box with its maxima past its minima, and a tile size from
2 to 4096. A URL given as `--source` is refused by name, with the reason. Then the source is
hashed against `--source-sha256` when one is given, and nothing is written for a file that
differs. A frame the file's own GeoKeys contradict is refused. The file is cropped to
`--bbox` (the cells whose centres lie inside it) and handed to `terrain_store::stage`, which
increment 1 built. Last, the manifest's digest is printed with the two baseline fields to set.
A directory that already holds a manifest is refused.

**Tests.** The module's own tests check the argument refusals, the crop, and the frame
check. `terrain_stage_stages_a_checked_source_and_prints_the_digest_to_pin` stages the
committed `small.tif`. A wrong digest writes nothing. The right one, from
`testdata/dem/SOURCE.md`, stages a store that opens under the printed digest and reads tile
by tile. A second run into the same directory is refused, and a local-frame claim against the
file's UTM tags is refused by name.

**Run once by hand.** `cargo run -p gungnir-node -- terrain stage` staged the committed
Oregon clip (`testdata/dem/usgs_1_n44w124_clip.tif`, digest checked) as 17 tiles over 3
levels: 4 by 3, 2 by 2, and 1, as the geometry rules give for 1556 by 1038 cells. Its
manifest SHA-256 was `a8b09fe764a0b7a15639dd9b7d9f376f4e2b40c8f9a0bcdff892fba8b7d0c13e`. A
throwaway test, not committed, then opened that store under round 1's Oregon baseline in a
`crs` build with the GEOID18 clip installed. In about three seconds of a debug build, all
twelve level-0 tiles the view wanted were placed (NAD83(2011) degrees and NAVD88 heights to
local ENU) and drawn at stride 16, 6 936 vertices, with the past-edge sentence. Line of sight
was built at level 1, 45 m cells over 1536 by 1337, because the coverage extent of round 1's
two radars is too wide for level 0's 22 m cells within the field's budget. The store was not
committed: it is 7 MB, and the clip it was cut from already is.

**Not done.** The source fetch, the fill cache, and staging from the fill cache (amendment 2)
wait on the HTTP client. The tiles carry no TIFF predictor (DN-36 amendment 3 item 2).

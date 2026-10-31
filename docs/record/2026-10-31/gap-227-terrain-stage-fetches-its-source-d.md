# GAP-227: terrain stage fetches its source (D-173)

**The decision.** DN-36 amendment 1 put a fetch in the binaries. Its HTTP client was a §2.9
question, put to the owner on 2026-10-30 through the session that started this build. On
2026-10-31 the owner chose `reqwest` with `blocking`, behind `gungnir-data`'s new
`terrain-fetch` feature, off by default, over `ureq`. That is D-173, and DN-36 amendment 5
records what follows from it. §2.9's `reqwest` row is widened, not duplicated.

**The roots.** The existing row has `reqwest` configured with the event stream's own
`rustls::ClientConfig`, which trusts the deployment's roots. The owner asked which root store
a fetch from a public source uses. It is the operating system's: `reqwest` 0.13's `rustls`
feature brings `rustls-platform-verifier`, and `reqwest::blocking::Client::builder()` with no
roots of its own verifies through it. Only `https://` is fetched, or `http://` to
`127.0.0.1`, `::1` or `localhost`, which is a mirror on the same machine and is what the test
uses. Plain `http` to another host is refused, as is any other scheme.

**The graph.** `Cargo.lock` gains two edges and no package: `gungnir-data` to `reqwest`, and
`reqwest` to `futures-channel`, which `blocking` turns on. `futures-util` was already
reqwest's. `cargo tree -d` over `gungnir-data` with the feature shows base64 0.13 and 0.22
and memchr 1 and 2 side by side, and both pairs were already linked in both binaries, through
`gltf` and `vtkio` on one side and the API stack on the other. The workspace's
`[workspace.dependencies]` line is unchanged; the feature is asked for on `gungnir-data`'s
own dependency line.

**The command.** `--source` takes a URL whose path ends in `.tif`, `.tiff` or `.asc`. It is
fetched into `<out>.source.<ext>` beside the output (a new file, never overwritten, at most
4 GiB, with a 30 minute timeout), checked against `--source-sha256` when one is given,
staged, and removed whether staging succeeded or not. The output now prints the source's
own SHA-256 first, for a file or a URL, so a fetch can be pinned later even when the source
published no digest. The manifest records the URL. A build without the feature refuses a URL,
naming the feature.

**Tests.** `terrain_stage_fetches_a_source_from_a_loopback_mirror`, in
`gungnir-data/tests/terrain_store.rs` under `--features terrain-fetch`, serves the committed
`small.tif` from a `TcpListener` on the loopback. The fetched file checks against
`testdata/dem/SOURCE.md`'s digest, the store opens under the printed manifest digest, the
manifest records the URL, and the download is gone. A 404 stages nothing. The module's tests
cover the scheme and host rules, and `a_url_without_terrain_fetch_is_refused_by_name` covers
the default build. No fetch from the public internet was run here: the test reaches nothing
beyond the loopback.

**Not done.** The fill cache needs a form for `terrain.fill_source` that DN-36 never named.
Amendment 5 raises it as D-174, with a recommended default, and the fill waits on the owner's
answer.

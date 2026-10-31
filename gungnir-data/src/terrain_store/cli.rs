// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! `terrain stage` (GAP-227, D-162, `docs/design/DN-36-staged-terrain-and-map-tiles.md`
//! amendment 1 item 2): one implementation behind `gungnir-app terrain stage` and
//! `gungnir-node terrain stage`, as `account` is one implementation behind both.
//!
//! Given a DEM, a bounding box in the DEM's own frame and an output directory, it checks
//! the DEM against its published SHA-256 when one is given, crops it to the box, cuts and
//! averages the pyramid ([`super::stage`]), writes the tiles and the manifest, and prints
//! the manifest's digest for the baseline.
//!
//! **The source is a file, or a URL with `terrain-fetch`** (D-173). A URL is fetched with
//! `reqwest`'s blocking client into a file beside `--out`, which is staged from and then
//! removed. TLS is verified against the operating system's trust store
//! (`rustls-platform-verifier`, which `reqwest`'s `rustls` feature selects), since a
//! source such as the USGS is signed by a public authority, not by the deployment's own
//! roots. Only `https://` is fetched, or `http://` to this machine (a mirror on the
//! loopback interface), so nothing crosses a network unauthenticated. A build without the
//! feature refuses a URL by name, as a build without `crs` refuses a real-world CRS.
//!
//! The binary only ever reads a store; this command is the one place the workspace writes
//! one, and it refuses a directory that already holds a manifest.

use std::path::{Path, PathBuf};

use super::{stage, ManifestSource, StageOptions};
use crate::geospatial::{GridCrs, HeightGrid};

/// A store's tiles are this many cells on a side unless told otherwise (DN-36 §4.1).
pub const DEFAULT_TILE_CELLS: u32 = 512;

/// The usage line for `binary`.
#[must_use]
pub fn usage(binary: &str) -> String {
    format!(
        "usage: {binary} terrain stage --source <dem.tif|dem.asc|https://...> --out <directory> \
         --frame <local-enu|epsg:CODE> --licence <text> --retrieved <date> \
         [--source-url <url>] [--source-sha256 <hex>] [--bbox <min_x,min_y,max_x,max_y>] \
         [--vertical <ellipsoidal|epsg:CODE>] [--nad83-realisation <epsg:CODE>] \
         [--tile-cells <n>]"
    )
}

/// Run `terrain ...` with the arguments after `terrain`. Returns what to print.
///
/// # Errors
///
/// The usage, or why the store was not staged, in words.
pub fn run(binary: &str, args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("stage") => stage_command(binary, &args[1..]),
        _ => Err(usage(binary)),
    }
}

/// The parsed `terrain stage` arguments.
#[derive(Debug, Clone, PartialEq)]
struct Stage {
    source: Source,
    out: PathBuf,
    frame: String,
    licence: String,
    retrieved: String,
    source_url: Option<String>,
    source_sha256: Option<String>,
    bbox: Option<[f64; 4]>,
    vertical: Option<String>,
    nad83_realisation: Option<String>,
    tile_cells: u32,
}

fn parse(binary: &str, args: &[String]) -> Result<Stage, String> {
    let mut source = None;
    let mut out = None;
    let mut frame = None;
    let mut licence = None;
    let mut retrieved = None;
    let mut source_url = None;
    let mut source_sha256 = None;
    let mut bbox = None;
    let mut vertical = None;
    let mut nad83_realisation = None;
    let mut tile_cells = DEFAULT_TILE_CELLS;
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let value = rest
            .next()
            .ok_or_else(|| format!("{flag} needs a value\n{}", usage(binary)))?
            .clone();
        match flag.as_str() {
            "--source" => source = Some(value),
            "--out" => out = Some(PathBuf::from(value)),
            "--frame" => frame = Some(value),
            "--licence" => licence = Some(value),
            "--retrieved" => retrieved = Some(value),
            "--source-url" => source_url = Some(value),
            "--source-sha256" => source_sha256 = Some(value),
            "--bbox" => bbox = Some(parse_bbox(&value)?),
            "--vertical" => vertical = Some(value),
            "--nad83-realisation" => nad83_realisation = Some(value),
            "--tile-cells" => {
                tile_cells = value
                    .parse()
                    .ok()
                    .filter(|n| (2..=4096).contains(n))
                    .ok_or_else(|| format!("--tile-cells {value:?} is not between 2 and 4096"))?;
            }
            other => return Err(format!("{other} is not an option\n{}", usage(binary))),
        }
    }
    let missing = |name: &str| format!("{name} is required\n{}", usage(binary));
    let source = Source::parse(&source.ok_or_else(|| missing("--source"))?)?;
    let frame = frame.ok_or_else(|| missing("--frame"))?;
    if frame != "local-enu" && epsg_code(&frame).is_none() {
        return Err(format!(
            "--frame {frame:?} is not \"local-enu\" or \"epsg:<code>\" with a non-zero code"
        ));
    }
    if let Some(digest) = &source_sha256 {
        if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!(
                "--source-sha256 {digest:?} is not 64 hexadecimal digits"
            ));
        }
    }
    Ok(Stage {
        source,
        out: out.ok_or_else(|| missing("--out"))?,
        frame,
        licence: licence.ok_or_else(|| missing("--licence"))?,
        retrieved: retrieved.ok_or_else(|| missing("--retrieved"))?,
        source_url,
        source_sha256,
        bbox,
        vertical,
        nad83_realisation,
        tile_cells,
    })
}

fn parse_bbox(text: &str) -> Result<[f64; 4], String> {
    let values: Vec<f64> = text
        .split(',')
        .map(|v| v.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .map_err(|_| format!("--bbox {text:?} is not four numbers"))?;
    match values.as_slice() {
        &[x0, y0, x1, y1] if values.iter().all(|v| v.is_finite()) && x1 > x0 && y1 > y0 => {
            Ok([x0, y0, x1, y1])
        }
        _ => Err(format!(
            "--bbox {text:?} is not min_x,min_y,max_x,max_y with each maximum past its minimum"
        )),
    }
}

/// `"epsg:<code>"` to its code, case-insensitive on the prefix, ASCII digits only, never
/// zero: `terrain.frame`'s own grammar, which this crate cannot reach.
fn epsg_code(frame: &str) -> Option<u32> {
    let lower = frame.to_ascii_lowercase();
    let digits = lower.strip_prefix("epsg:")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().filter(|code| *code != 0)
}

/// Where the source DEM is.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    File(PathBuf),
    /// Fetched with `terrain-fetch`; the extension its path ends in, which the loader reads
    /// the format by.
    Url {
        url: String,
        extension: String,
    },
}

impl Source {
    /// A path, or a URL this command will fetch: `https://`, or `http://` to this machine,
    /// whose path ends in `.tif`, `.tiff` or `.asc`.
    fn parse(text: &str) -> Result<Source, String> {
        let Some((scheme, rest)) = text.split_once("://") else {
            return Ok(Source::File(PathBuf::from(text)));
        };
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let host_only = match host.strip_prefix('[') {
            Some(v6) => v6.split(']').next().unwrap_or_default(),
            None => host.split(':').next().unwrap_or_default(),
        };
        let loopback = matches!(host_only, "127.0.0.1" | "localhost" | "::1");
        match scheme.to_ascii_lowercase().as_str() {
            "https" => {}
            "http" if loopback => {}
            "http" => {
                return Err(format!(
                    "--source {text:?} is plain http to another machine; only https:// is \
                     fetched, or http:// to this machine's loopback"
                ))
            }
            other => return Err(format!("--source {text:?}: {other}:// is not fetched")),
        }
        let path = rest.split(['?', '#']).next().unwrap_or_default();
        let extension = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .filter(|e| matches!(e.as_str(), "tif" | "tiff" | "asc"))
            .ok_or_else(|| {
                format!("--source {text:?} does not end in .tif, .tiff or .asc, a DEM this reads")
            })?;
        Ok(Source::Url {
            url: text.to_string(),
            extension,
        })
    }
}

/// The source on disk: the file named, or the URL fetched beside `out`, which the caller
/// removes once staged. Also what the manifest records it as.
fn resolve(options: &Stage) -> Result<(PathBuf, Option<PathBuf>, String), String> {
    match &options.source {
        Source::File(path) => Ok((path.clone(), None, format!("file:{}", path.display()))),
        Source::Url { url, extension } => {
            let name = options
                .out
                .file_name()
                .map_or_else(|| "store".into(), |n| n.to_string_lossy().into_owned());
            let download = options
                .out
                .with_file_name(format!("{name}.source.{extension}"));
            fetch(url, &download)?;
            Ok((download.clone(), Some(download), url.clone()))
        }
    }
}

/// The most a fetched source may be: past this it is not a DEM tile.
#[cfg(feature = "terrain-fetch")]
const MAX_FETCH_BYTES: u64 = 4 << 30;

/// Fetch `url` into a new file `dest`, verifying TLS against the operating system's trust
/// store.
#[cfg(feature = "terrain-fetch")]
fn fetch(url: &str, dest: &Path) -> Result<(), String> {
    use std::io::Read as _;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_mins(30))
        .build()
        .map_err(|e| format!("the HTTP client did not start: {e}"))?;
    let response = client
        .get(url)
        .send()
        .map_err(|e| format!("{url} was not fetched: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("{url} answered {}", response.status()));
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .map_err(|e| format!("{}: {e}", dest.display()))?;
    let copied = std::io::copy(&mut response.take(MAX_FETCH_BYTES + 1), &mut file);
    let refuse = |why: String| {
        let _ = std::fs::remove_file(dest);
        Err(why)
    };
    match copied {
        Ok(n) if n > MAX_FETCH_BYTES => refuse(format!(
            "{url} is larger than {MAX_FETCH_BYTES} bytes, past any DEM tile"
        )),
        Ok(_) => Ok(()),
        Err(e) => refuse(format!("{url} was not fetched whole: {e}")),
    }
}

/// Without `terrain-fetch` there is no HTTP client in this binary.
#[cfg(not(feature = "terrain-fetch"))]
fn fetch(url: &str, _dest: &Path) -> Result<(), String> {
    Err(format!(
        "--source {url:?} is a URL, and this binary was built without the terrain-fetch \
         feature, which holds the only HTTP client that fetches one (D-173); download the \
         file, check it, and give its path"
    ))
}

fn stage_command(binary: &str, args: &[String]) -> Result<String, String> {
    let options = parse(binary, args)?;
    let (source, download, recorded) = resolve(&options)?;
    let staged = stage_from(binary, &options, &source, recorded);
    if let Some(download) = download {
        let _ = std::fs::remove_file(download);
    }
    staged
}

fn stage_from(
    binary: &str,
    options: &Stage,
    source: &Path,
    recorded: String,
) -> Result<String, String> {
    let digest_of = || -> Result<String, String> {
        let file = std::fs::File::open(source).map_err(|e| format!("{}: {e}", source.display()))?;
        crate::geoid::sha256_hex(std::io::BufReader::new(file))
            .map_err(|e| format!("{}: {e}", source.display()))
    };
    let found = digest_of()?;
    if let Some(expected) = &options.source_sha256 {
        if !found.eq_ignore_ascii_case(expected) {
            return Err(format!(
                "{recorded} is not the published source: its SHA-256 is {found}, and the \
                 source publishes {}",
                expected.to_ascii_lowercase()
            ));
        }
    }
    let grid = crate::geospatial::load_height_grid(source).map_err(|e| e.to_string())?;
    check_frame(&options.frame, grid.crs)?;
    let grid = match options.bbox {
        Some(bbox) => crop(&grid, bbox)?,
        None => grid,
    };
    let source_url = options.source_url.clone().unwrap_or(recorded);
    let (manifest, digest) = stage(
        &grid,
        &options.out,
        &StageOptions {
            source: ManifestSource {
                url: source_url,
                retrieved: options.retrieved.clone(),
                licence: options.licence.clone(),
            },
            staged_by: format!("{binary} terrain stage {}", env!("CARGO_PKG_VERSION")),
            frame: options.frame.clone(),
            vertical: options.vertical.clone(),
            nad83_realisation: options.nad83_realisation.clone(),
            tile_cells: options.tile_cells,
            apron_cells: 1,
            levels: None,
        },
    )
    .map_err(|e| e.to_string())?;
    Ok(format!(
        "source {} SHA-256 {found}\n\
         staged {} tiles over {} levels into {}\nmanifest SHA-256 {digest}\n\
         pin it in the baseline: terrain.store {:?}, terrain.manifest_sha256 {digest:?}",
        manifest.source.url,
        manifest.tiles.len(),
        manifest.levels,
        options.out.display(),
        options.out.display().to_string()
    ))
}

/// A frame the source's own tags contradict is refused here, before anything is written,
/// as the desktop refuses it when it places a tile.
fn check_frame(frame: &str, crs: GridCrs) -> Result<(), String> {
    let stated = match crs {
        GridCrs::Geographic { epsg: Some(code) } | GridCrs::Projected { epsg: Some(code) }
            if code != 32_767 =>
        {
            Some(u32::from(code))
        }
        _ => None,
    };
    match (epsg_code(frame), stated) {
        (None, Some(code)) => Err(format!(
            "the source states EPSG:{code}, not the local frame --frame claims; give --frame \
             epsg:{code} if that is correct"
        )),
        (Some(declared), Some(code)) if declared != code => Err(format!(
            "--frame declares EPSG:{declared} and the source states EPSG:{code}"
        )),
        _ => Ok(()),
    }
}

/// The cells of `grid` whose centres lie inside `bbox` (`[min_x, min_y, max_x, max_y]` in
/// the grid's own frame), as a grid of their own.
// Cell indices are bounded by the grid's own `u32` shape.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn crop(grid: &HeightGrid, bbox: [f64; 4]) -> Result<HeightGrid, String> {
    let [dx, dy] = grid.cell_size;
    let [x0, y0] = grid.origin;
    let first_column = ((bbox[0] - x0) / dx - 0.5).ceil().max(0.0);
    let last_column = ((bbox[2] - x0) / dx - 0.5)
        .floor()
        .min(f64::from(grid.columns) - 1.0);
    // Rows are stored north first; count from the south, then turn round.
    let first_south = ((bbox[1] - y0) / dy - 0.5).ceil().max(0.0);
    let last_south = ((bbox[3] - y0) / dy - 0.5)
        .floor()
        .min(f64::from(grid.rows) - 1.0);
    if !(first_column <= last_column && first_south <= last_south) {
        return Err(format!(
            "--bbox {bbox:?} holds no cell of the source, which covers {:?}",
            grid.bounds()
        ));
    }
    let (c0, c1) = (first_column as u32, last_column as u32);
    let (s0, s1) = (first_south as u32, last_south as u32);
    let (r0, r1) = (grid.rows - 1 - s1, grid.rows - 1 - s0);
    let mut heights = Vec::with_capacity(((c1 - c0 + 1) * (r1 - r0 + 1)) as usize);
    for row in r0..=r1 {
        let start = row as usize * grid.columns as usize;
        heights.extend_from_slice(&grid.heights[start + c0 as usize..=start + c1 as usize]);
    }
    Ok(HeightGrid {
        columns: c1 - c0 + 1,
        rows: r1 - r0 + 1,
        origin: [x0 + f64::from(c0) * dx, y0 + f64::from(s0) * dy],
        cell_size: grid.cell_size,
        nodata: grid.nodata,
        crs: grid.crs,
        vertical: grid.vertical,
        heights,
    })
}

#[cfg(test)]
// The cropped grid's figures are whole numbers a float carries exactly.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn the_arguments_are_checked_before_anything_is_read() {
        let base =
            "stage --source a.tif --out o --frame epsg:4269 --licence pd --retrieved 2026-10-31";
        assert!(parse("g", &args(&base[6..])).is_ok());
        for (extra, said) in [
            (" --frame wgs84", "--frame"),
            (" --bbox 1,2,3", "--bbox"),
            (" --bbox 3,2,1,4", "--bbox"),
            (" --tile-cells 1", "--tile-cells"),
            (" --source-sha256 abc", "--source-sha256"),
            (" --nonsense x", "not an option"),
        ] {
            let err = parse("g", &args(&format!("{}{extra}", &base[6..]))).expect_err(said);
            assert!(err.contains(said), "{said}: {err}");
        }
        let source = |s: &str| Source::parse(s);
        assert!(
            matches!(source("https://x.example/a/b.TIF?x=1"), Ok(Source::Url { extension, .. }) if extension == "tif")
        );
        assert!(source("http://127.0.0.1:8080/b.asc").is_ok());
        assert!(source("http://[::1]/b.tif").is_ok());
        assert!(source("http://user@localhost/b.tiff").is_ok());
        let plain = source("http://example.com/b.tif").expect_err("plain http");
        assert!(plain.contains("only https://"), "{plain}");
        assert!(source("ftp://x/b.tif").is_err());
        assert!(source("https://x/b.zip")
            .expect_err("not a DEM")
            .contains(".tif"));
        assert_eq!(
            source("dem/b.tif"),
            Ok(Source::File(PathBuf::from("dem/b.tif")))
        );
        assert!(run("g", &args("fetch"))
            .expect_err("usage")
            .starts_with("usage: g terrain stage"));
    }

    /// A build without `terrain-fetch` has no HTTP client, and says so for a URL source.
    #[cfg(not(feature = "terrain-fetch"))]
    #[test]
    fn a_url_without_terrain_fetch_is_refused_by_name() {
        let err = run(
            "g",
            &args("stage --source https://x.example/y.tif --out o --frame local-enu --licence p --retrieved d"),
        )
        .expect_err("no client");
        assert!(err.contains("terrain-fetch"), "{err}");
    }

    #[test]
    fn a_crop_keeps_the_cells_whose_centres_lie_inside() {
        let grid = HeightGrid {
            columns: 5,
            rows: 4,
            origin: [100.0, 200.0],
            cell_size: [10.0, 10.0],
            nodata: None,
            crs: GridCrs::Unstated,
            vertical: None,
            heights: (0..20_u8).map(f32::from).collect(),
        };
        // Columns 1-2 (centres 115, 125) and south rows 1-2 (centres 215, 225), which are
        // stored rows 1-2 north first.
        let cropped = crop(&grid, [112.0, 212.0, 128.0, 228.0]).expect("cropped");
        assert_eq!((cropped.columns, cropped.rows), (2, 2));
        assert_eq!(cropped.origin, [110.0, 210.0]);
        assert_eq!(cropped.heights, vec![6.0, 7.0, 11.0, 12.0]);
        assert!(crop(&grid, [0.0, 0.0, 50.0, 50.0]).is_err());
    }

    #[test]
    fn a_frame_the_source_contradicts_is_refused() {
        let utm = GridCrs::Projected { epsg: Some(32633) };
        assert!(check_frame("epsg:32633", utm).is_ok());
        assert!(check_frame("epsg:4326", utm)
            .expect_err("contradicts")
            .contains("32633"));
        assert!(check_frame("local-enu", utm).is_err());
        assert!(check_frame("local-enu", GridCrs::Unstated).is_ok());
    }
}

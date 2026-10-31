// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The fill cache's data half (GAP-227, D-160, D-164, D-174,
//! `docs/design/DN-36-staged-terrain-and-map-tiles.md` amendments 1, 2 and 5): one-degree
//! source tiles named by their north-west corner, fetched from the template a baseline
//! names in `terrain.fill_source`, each staged into its own sub-store under the data
//! directory with `terrain stage`'s code, never into the pinned store.
//!
//! **What a fill tile is worth.** Its digest is recorded at fetch time in
//! `<name>.fetched.json` beside its sub-store, with the URL and the source file's own
//! digest. That records what was fetched, not that anybody vouched for it (D-164): a fill
//! tile is drawn, marked, and never enters line of sight. A later session reopens a cached
//! sub-store against the digest recorded for it, so a cache changed on disk is refused as
//! a pinned store would be, and nothing is fetched again.

use std::path::{Path, PathBuf};

use super::{stage, Manifest, ManifestSource, StageOptions, TerrainStore};

/// The four placeholders a fill source's template names a one-degree tile with.
pub const PLACEHOLDERS: [&str; 4] = ["{ns}", "{lat}", "{ew}", "{lon}"];

/// One one-degree source tile, by its north-west corner in whole degrees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OneDegree {
    /// The latitude of its north edge.
    pub north: i32,
    /// The longitude of its west edge.
    pub west: i32,
}

impl OneDegree {
    /// Its name as USGS 3DEP writes it: `n44w124` for the tile from 43 to 44 N and from
    /// 124 to 123 W; `s05e010` south of the equator and east of Greenwich.
    #[must_use]
    pub fn name(self) -> String {
        let (ns, lat) = self.parts();
        format!("{}{}{}{}", ns.0, lat.0, ns.1, lat.1)
    }

    /// `(ns, ew)` and `(lat, lon)` as the template's four placeholders read them.
    fn parts(self) -> ((char, char), (String, String)) {
        let ns = if self.north >= 0 { 'n' } else { 's' };
        let ew = if self.west >= 0 { 'e' } else { 'w' };
        (
            (ns, ew),
            (
                format!("{:02}", self.north.unsigned_abs()),
                format!("{:03}", self.west.unsigned_abs()),
            ),
        )
    }

    /// The URL `template` names this tile at.
    #[must_use]
    pub fn url(self, template: &str) -> String {
        let ((ns, ew), (lat, lon)) = self.parts();
        template
            .replace("{ns}", &ns.to_string())
            .replace("{lat}", &lat)
            .replace("{ew}", &ew.to_string())
            .replace("{lon}", &lon)
    }

    /// The tiles that touch the box `[lon0, lat0, lon1, lat1]` in degrees, west to east and
    /// north to south.
    // A box of degrees on the earth fits an `i32` many times over.
    #[allow(clippy::cast_possible_truncation)]
    #[must_use]
    pub fn covering(lon0: f64, lat0: f64, lon1: f64, lat1: f64) -> Vec<OneDegree> {
        if ![lon0, lat0, lon1, lat1].iter().all(|v| v.is_finite()) || lon1 <= lon0 || lat1 <= lat0 {
            return Vec::new();
        }
        let (south, north) = (lat0.max(-90.0).floor() as i32, lat1.min(90.0).ceil() as i32);
        let (west, east) = (
            lon0.max(-180.0).floor() as i32,
            lon1.min(180.0).ceil() as i32,
        );
        let mut tiles = Vec::new();
        for n in (south + 1..=north).rev() {
            for w in west..east {
                tiles.push(OneDegree { north: n, west: w });
            }
        }
        tiles
    }
}

/// Check a fill source's template: all four placeholders, and a scheme this fetches.
///
/// # Errors
///
/// What is missing or refused, in words.
pub fn check_template(template: &str) -> Result<(), String> {
    if let Some(missing) = PLACEHOLDERS.iter().find(|p| !template.contains(**p)) {
        return Err(format!(
            "{template:?} does not name a one-degree tile: it lacks {missing}, one of \
             {{ns}}{{lat}}{{ew}}{{lon}} (as n44w124)"
        ));
    }
    super::cli::check_url(
        &OneDegree {
            north: 44,
            west: -124,
        }
        .url(template),
    )
    .map(|_| ())
}

/// What a fill tile's fetch recorded: `<name>.fetched.json`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fetched {
    pub url: String,
    /// The fetched file's own SHA-256.
    pub source_sha256: String,
    /// The sub-store's manifest digest, which a later session reopens it against.
    pub manifest_sha256: String,
    /// Seconds since 1970 when it was fetched.
    pub fetched_at_unix: u64,
}

/// The cache's files for `tile` under `cache_dir`: the sub-store and its record.
#[must_use]
pub fn paths(cache_dir: &Path, tile: OneDegree) -> (PathBuf, PathBuf) {
    let name = tile.name();
    (
        cache_dir.join(&name),
        cache_dir.join(format!("{name}.fetched.json")),
    )
}

/// A sub-store fetched in an earlier session, reopened against the digest recorded when
/// it was fetched. `Ok(None)` when it was never fetched.
///
/// # Errors
///
/// A record that does not read, or a sub-store that no longer matches it.
pub fn open_cached(cache_dir: &Path, tile: OneDegree) -> Result<Option<TerrainStore>, String> {
    let (dir, record) = paths(cache_dir, tile);
    let text = match std::fs::read_to_string(&record) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", record.display())),
    };
    let fetched: Fetched =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", record.display()))?;
    TerrainStore::open(&dir, &fetched.manifest_sha256)
        .map(Some)
        .map_err(|e| e.to_string())
}

/// Fetch `tile` from `template`, stage it into its own sub-store under `cache_dir` in the
/// pinned store's frame and tile size (`like`), record what was fetched, and return the
/// sub-store. Never touches the pinned store: `cache_dir` is the data directory's.
///
/// # Errors
///
/// The fetch, the source, or the staging refused, in words. A build without
/// `terrain-fetch` has no client and says so.
pub fn fetch_and_stage(
    template: &str,
    tile: OneDegree,
    cache_dir: &Path,
    like: &Manifest,
) -> Result<TerrainStore, String> {
    let url = tile.url(template);
    let extension = super::cli::check_url(&url)?;
    let (dir, record) = paths(cache_dir, tile);
    std::fs::create_dir_all(cache_dir).map_err(|e| format!("{}: {e}", cache_dir.display()))?;
    let download = cache_dir.join(format!("{}.source.{extension}", tile.name()));
    let _ = std::fs::remove_file(&download);
    fetch(&url, &download)?;
    let staged = (|| {
        let file =
            std::fs::File::open(&download).map_err(|e| format!("{}: {e}", download.display()))?;
        let source_sha256 =
            crate::geoid::sha256_hex(std::io::BufReader::new(file)).map_err(|e| e.to_string())?;
        let grid = crate::geospatial::load_height_grid(&download).map_err(|e| e.to_string())?;
        let fetched_at_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let (_, manifest_sha256) = stage(
            &grid,
            &dir,
            &StageOptions {
                source: ManifestSource {
                    url: url.clone(),
                    retrieved: format!("unix:{fetched_at_unix}"),
                    licence: "as the fill source publishes it; fetched by the fill cache, \
                              not staged by anybody"
                        .to_string(),
                },
                staged_by: format!("gungnir fill cache {}", env!("CARGO_PKG_VERSION")),
                frame: like.frame.clone(),
                vertical: like.vertical.clone(),
                nad83_realisation: like.nad83_realisation.clone(),
                tile_cells: like.tile_cells,
                apron_cells: like.apron_cells,
                levels: None,
            },
        )
        .map_err(|e| e.to_string())?;
        let fetched = Fetched {
            url: url.clone(),
            source_sha256,
            manifest_sha256: manifest_sha256.clone(),
            fetched_at_unix,
        };
        let text = serde_json::to_vec_pretty(&fetched).map_err(|e| e.to_string())?;
        std::fs::write(&record, text).map_err(|e| format!("{}: {e}", record.display()))?;
        TerrainStore::open(&dir, &manifest_sha256).map_err(|e| e.to_string())
    })();
    let _ = std::fs::remove_file(&download);
    staged
}

/// The most a fetched source may be: past this it is not a DEM tile.
#[cfg(feature = "terrain-fetch")]
const MAX_FETCH_BYTES: u64 = 4 << 30;

/// Fetch `url` into a new file `dest`, verifying TLS against the operating system's trust
/// store (D-173).
///
/// # Errors
///
/// The request, the status or the copy failed, or the body was larger than any DEM tile.
#[cfg(feature = "terrain-fetch")]
pub fn fetch(url: &str, dest: &Path) -> Result<(), String> {
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
///
/// # Errors
///
/// Always, naming the feature.
#[cfg(not(feature = "terrain-fetch"))]
pub fn fetch(url: &str, _dest: &Path) -> Result<(), String> {
    Err(format!(
        "{url:?} is a URL, and this binary was built without the terrain-fetch feature, \
         which holds the only HTTP client that fetches one (D-173)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tile_is_named_by_its_north_west_corner_as_usgs_names_it() {
        assert_eq!(
            OneDegree {
                north: 44,
                west: -124
            }
            .name(),
            "n44w124"
        );
        assert_eq!(
            OneDegree {
                north: -5,
                west: 10
            }
            .name(),
            "s05e010"
        );
        assert_eq!(
            OneDegree {
                north: 44,
                west: -124
            }
            .url("https://x.example/{ns}{lat}{ew}{lon}/USGS_1_{ns}{lat}{ew}{lon}.tif"),
            "https://x.example/n44w124/USGS_1_n44w124.tif"
        );
        // 123.5 to 122.5 W, 43.2 to 44.1 N: two columns and two rows of tiles.
        let names: Vec<String> = OneDegree::covering(-123.5, 43.2, -122.5, 44.1)
            .into_iter()
            .map(OneDegree::name)
            .collect();
        assert_eq!(names, ["n45w124", "n45w123", "n44w124", "n44w123"]);
        assert!(OneDegree::covering(1.0, 1.0, 0.0, 2.0).is_empty());
    }

    #[test]
    fn a_template_names_all_four_parts_and_a_scheme_this_fetches() {
        assert!(check_template("https://x/{ns}{lat}{ew}{lon}.tif").is_ok());
        assert!(check_template("http://127.0.0.1:9/{ns}{lat}{ew}{lon}.tif").is_ok());
        assert!(check_template("https://x/{ns}{lat}{lon}.tif")
            .expect_err("no {ew}")
            .contains("{ew}"));
        assert!(check_template("http://example.com/{ns}{lat}{ew}{lon}.tif").is_err());
        assert!(check_template("https://x/{ns}{lat}{ew}{lon}.zip").is_err());
    }
}

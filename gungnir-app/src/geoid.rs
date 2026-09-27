// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The EGM2008 geoid grid on the desktop (GAP-108, D-121): found, verified against its
//! pinned SHA-256 off the render thread at start, and reported on PN-09.
//!
//! **Where the grid is looked for**, in order: the baseline's `geoid_grid_dir`; then
//! `PROJ_DATA`, PROJ's own convention for where its resource files live, and its older
//! `PROJ_LIB`, each of which may list several directories. The first directory holding
//! `us_nga_egm08_25.tif` is the one checked; a deployment naming none of these has no
//! grid, and says so. **Wherever it is found, it is used only if its bytes are the pinned
//! ones** (`gungnir_data::geoid::GeoidGrid`), and a file that is present but different is
//! reported as such -- not quietly used, and not quietly treated as absent.
//!
//! **What waits on it.** The terrain and the point-cloud pair do not start loading until
//! the check has settled, so a file stating EGM2008 heights never reaches placement while
//! the grid it needs is still being hashed. That costs the hash's time once at start --
//! about a fifth of a second for the 80 MB grid in a release build -- and nothing when no
//! grid is present.
//!
//! **What else it corrects: a UAS's height above mean sea level (GAP-196, D-123).**
//! ASTERIX Category 129 states a UAS's height only above mean sea level (I129/090).
//! Once the check settles, [`lend_to_feeds`] hands every bound radar feed the verified
//! grid as a [`gungnir_ingest::geoid::GeoidSeparation`] -- one PROJ lookup service,
//! `gungnir_data::geoid::UndulationService`, shared by them all -- and the feed's
//! adapter adds the EGM2008 separation before it places the report. Until then, and in
//! any deployment without a verified grid or a build without `crs`, every feed holds the
//! reason instead, and the height reaches the picture flagged as mean sea level, never as
//! an ellipsoidal one (D-124). `gungnir-ingest` has no edge to `gungnir-data`
//! (`ARCHITECTURE.md` §7.1), which is why the grid crosses as an interface lent here
//! rather than a call made there.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gungnir_data::geoid::{GeoidGrid, UndulationService, EGM2008_GRID_FILE};
use gungnir_ingest::geoid::GeoidSeparation;

use crate::state::AppState;

/// Where a grid directory was named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridSource {
    /// `geoid_grid_dir` in the baseline.
    Baseline,
    /// The `PROJ_DATA` (or older `PROJ_LIB`) environment variable.
    ProjData,
}

impl GridSource {
    fn words(self) -> &'static str {
        match self {
            GridSource::Baseline => "the baseline's geoid_grid_dir",
            GridSource::ProjData => "PROJ_DATA",
        }
    }
}

/// Where the geoid grid stands, for PN-09 and for every conversion that needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeoidStatus {
    /// Before the first tick: the check has not run.
    NotChecked,
    /// No directory to look in: the baseline names none and `PROJ_DATA` is unset or
    /// holds no grid. EGM2008 heights are refused.
    NotConfigured,
    /// The file is being hashed, off the render thread.
    Verifying { path: PathBuf, source: GridSource },
    /// The pinned grid, verified.
    Verified { grid: GeoidGrid, source: GridSource },
    /// A file was named and is not usable: missing, different from the pinned one, or
    /// unreadable. EGM2008 heights are refused, with this reason.
    Refused {
        path: PathBuf,
        source: GridSource,
        reason: String,
    },
}

impl GeoidStatus {
    /// Whether the check has finished, one way or the other; loads needing the grid wait
    /// until it has.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        !matches!(
            self,
            GeoidStatus::NotChecked | GeoidStatus::Verifying { .. }
        )
    }

    /// The verified grid for a conversion, or the reason there is none -- which a
    /// refused EGM2008 conversion carries into its own refusal.
    ///
    /// # Errors
    ///
    /// Every state but [`GeoidStatus::Verified`], in words.
    pub fn grid(&self) -> Result<&GeoidGrid, String> {
        match self {
            GeoidStatus::Verified { grid, .. } => Ok(grid),
            GeoidStatus::NotChecked => Err("the grid has not been checked yet".to_string()),
            GeoidStatus::NotConfigured => Err(format!(
                "no directory holding {EGM2008_GRID_FILE} is named (set geoid_grid_dir in \
                 the baseline, or PROJ_DATA)"
            )),
            GeoidStatus::Verifying { path, .. } => {
                Err(format!("{} is still being verified", path.display()))
            }
            GeoidStatus::Refused { reason, .. } => Err(reason.clone()),
        }
    }

    /// One line for PN-09.
    #[must_use]
    pub fn line(&self) -> String {
        let converts = if cfg!(feature = "crs") {
            ""
        } else {
            "; this build has no crs feature, so it converts no real-world CRS in any case, \
             and a UAS's height above mean sea level is placed uncorrected and flagged"
        };
        match self {
            GeoidStatus::NotChecked => "EGM2008 geoid grid: not checked yet".to_string(),
            GeoidStatus::NotConfigured => format!(
                "EGM2008 geoid grid: none installed (no geoid_grid_dir, and PROJ_DATA \
                 names no {EGM2008_GRID_FILE}); EGM2008 heights are refused, and a UAS's \
                 height above mean sea level is placed uncorrected and flagged{converts}"
            ),
            GeoidStatus::Verifying { path, source } => format!(
                "EGM2008 geoid grid: verifying {} (from {})",
                path.display(),
                source.words()
            ),
            GeoidStatus::Verified { grid, source } => format!(
                "EGM2008 geoid grid: verified, {} (from {}; SHA-256 {}...){converts}",
                grid.path().display(),
                source.words(),
                &grid.sha256()[..12]
            ),
            GeoidStatus::Refused {
                path,
                source,
                reason,
            } => format!(
                "EGM2008 geoid grid: refused, {} (from {}): {reason}; EGM2008 heights are \
                 refused, and a UAS's height above mean sea level is placed uncorrected \
                 and flagged{converts}",
                path.display(),
                source.words()
            ),
        }
    }

    /// Whether PN-09 shows the grid as present and good.
    #[must_use]
    pub fn is_verified(&self) -> bool {
        matches!(self, GeoidStatus::Verified { .. })
    }
}

/// The vertical datum a baseline's `vertical` string declares, `None` when it declares
/// none (`gungnir_config::VerticalFrame`, read into `gungnir-data`'s own type).
///
/// # Errors
///
/// The string does not parse, named by `field`. Validation refuses such a baseline first,
/// so this is reached only by one that skipped it.
pub fn declared_datum(
    field: &str,
    vertical: Option<&str>,
) -> Result<Option<gungnir_data::geoid::VerticalDatum>, String> {
    use gungnir_config::VerticalFrame;
    use gungnir_data::geoid::VerticalDatum;
    vertical
        .map(|v| {
            v.parse::<VerticalFrame>()
                .map(|frame| match frame {
                    VerticalFrame::Ellipsoidal => VerticalDatum::Ellipsoidal,
                    VerticalFrame::Epsg(code) => VerticalDatum::from_epsg(code),
                })
                .map_err(|reason| format!("{field} {reason}"))
        })
        .transpose()
}

/// The directory to look in and where it was named, or `None` for no candidate at all.
///
/// The baseline's field, when present, is final: a deployment that names a directory
/// means that one, and a grid elsewhere on `PROJ_DATA` is not substituted for a missing
/// one there. `PROJ_DATA` is searched entry by entry for the first that holds the file,
/// so a list whose first entry is PROJ's own `share/proj` still finds a grid installed
/// beside it.
fn candidate(baseline_dir: Option<&str>) -> Option<(PathBuf, GridSource)> {
    if let Some(dir) = baseline_dir {
        return Some((Path::new(dir).join(EGM2008_GRID_FILE), GridSource::Baseline));
    }
    let listed = std::env::var_os("PROJ_DATA").or_else(|| std::env::var_os("PROJ_LIB"))?;
    std::env::split_paths(&listed)
        .map(|dir| dir.join(EGM2008_GRID_FILE))
        .find(|path| path.is_file())
        .map(|path| (path, GridSource::ProjData))
}

/// Start the check on the first tick. Idempotent, and a status already settled -- a test
/// that installs a grid it verified itself, say -- is left alone.
pub fn start(state: &mut AppState) {
    if !matches!(state.geoid, GeoidStatus::NotChecked) {
        return;
    }
    let Some((path, source)) = candidate(state.config.geoid_grid_dir.as_deref()) else {
        state.geoid = GeoidStatus::NotConfigured;
        return;
    };
    // Nothing to hash: settle now, so a deployment whose named grid is missing starts
    // its terrain on the first frame like any other, and says why EGM2008 is refused.
    if !path.is_file() {
        settle(
            state,
            path.clone(),
            source,
            GeoidGrid::verify(&path, gungnir_data::geoid::EGM2008_GRID_SHA256)
                .map_err(|e| e.to_string()),
        );
        return;
    }
    let (tx, rx) = crossbeam_channel::bounded(1);
    let checked = path.clone();
    let spawned = std::thread::Builder::new()
        .name("geoid-grid-check".to_string())
        .spawn(move || {
            let result = GeoidGrid::verify(&checked, gungnir_data::geoid::EGM2008_GRID_SHA256)
                .map_err(|e| e.to_string());
            // The receiver is gone only if the state was dropped; nothing to tell then.
            let _ = tx.send(result);
        });
    match spawned {
        Ok(_) => {
            state.geoid = GeoidStatus::Verifying { path, source };
            state.geoid_check = Some(rx);
        }
        Err(e) => {
            state.geoid = GeoidStatus::Refused {
                path,
                source,
                reason: format!("the check could not be started: {e}"),
            };
        }
    }
}

/// Poll the check; on the tick, so hashing never stalls a frame. Starts it on the first
/// call, and says on the alert list what it found when it finishes, unless it found the
/// grid good.
pub fn poll(state: &mut AppState) {
    start(state);
    let Some(rx) = state.geoid_check.as_ref() else {
        return;
    };
    let result = match rx.try_recv() {
        Ok(result) => result,
        Err(crossbeam_channel::TryRecvError::Empty) => return,
        Err(crossbeam_channel::TryRecvError::Disconnected) => {
            Err("the check stopped without an answer".to_string())
        }
    };
    state.geoid_check = None;
    let GeoidStatus::Verifying { path, source } = state.geoid.clone() else {
        return;
    };
    settle(state, path, source, result);
}

/// The reason every bound feed holds before the grid check has settled.
pub const NOT_YET_CHECKED: &str = "the EGM2008 geoid grid has not been checked yet";

/// The verified EGM2008 grid as the ingest path sees it (GAP-196): one PROJ lookup
/// service, lent to every bound feed through its [`gungnir_ingest::geoid::GeoidHandle`].
#[derive(Debug, Clone)]
pub struct Egm2008Separation(pub UndulationService);

impl GeoidSeparation for Egm2008Separation {
    fn model(&self) -> &'static str {
        "EGM2008"
    }

    fn separation_m(&self, lat_deg: f64, lon_deg: f64) -> Result<f64, String> {
        self.0
            .undulation(lon_deg, lat_deg)
            .map_err(|e| e.to_string())
    }
}

/// Lend the grid, or the reason there is none, to every bound feed, whenever the check's
/// answer changes (GAP-196, D-123 and D-124). Runs on the tick after [`poll`], so a feed
/// bound before the answer learns it from the next report on; a status a test installs
/// directly is lent the same way.
///
/// A grid that verified and that PROJ still cannot open for lookups is an alert: the
/// deployment installed it and is not getting what it installed it for. A build without
/// `crs` says so in the reason and raises nothing, since PN-09's geoid line already
/// states the build cannot read any grid.
pub fn lend_to_feeds(state: &mut AppState) {
    if state.geoid_lent.as_ref() == Some(&state.geoid) {
        return;
    }
    state.geoid_lent = Some(state.geoid.clone());
    if state.feed_stats.is_empty() {
        // No feed holds the handle, so no lookup service is started for nobody.
        return;
    }
    let reason = match state.geoid.grid() {
        Ok(grid) => match UndulationService::start(grid) {
            Ok(service) => {
                state
                    .geoid_feeds
                    .set_available(Arc::new(Egm2008Separation(service)));
                return;
            }
            Err(e) => {
                let reason = format!("the verified EGM2008 grid cannot be read for lookups: {e}");
                if cfg!(feature = "crs") {
                    state.alerts.push(format!(
                        "a UAS's height above mean sea level stays uncorrected: {reason}"
                    ));
                }
                reason
            }
        },
        Err(reason) => format!("no verified EGM2008 geoid grid: {reason}"),
    };
    state.geoid_feeds.set_unavailable(reason);
}

/// Record the check's answer, and put a refusal on the alert list: a grid a deployment
/// named or installed and cannot use is something an operator is told about.
fn settle(
    state: &mut AppState,
    path: PathBuf,
    source: GridSource,
    result: Result<GeoidGrid, String>,
) {
    state.geoid = match result {
        Ok(grid) => GeoidStatus::Verified { grid, source },
        Err(reason) => {
            state.alerts.push(format!(
                "EGM2008 geoid grid {} refused: {reason}",
                path.display()
            ));
            GeoidStatus::Refused {
                path,
                source,
                reason,
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_named_directory_is_final_and_says_where_it_came_from() {
        let (path, source) = candidate(Some("/opt/gungnir/geoid")).expect("named");
        assert_eq!(source, GridSource::Baseline);
        assert!(path.ends_with(EGM2008_GRID_FILE), "{}", path.display());
    }

    #[test]
    fn every_unsettled_state_refuses_a_conversion_with_a_reason() {
        for status in [
            GeoidStatus::NotChecked,
            GeoidStatus::NotConfigured,
            GeoidStatus::Verifying {
                path: PathBuf::from("g.tif"),
                source: GridSource::Baseline,
            },
            GeoidStatus::Refused {
                path: PathBuf::from("g.tif"),
                source: GridSource::ProjData,
                reason: "its SHA-256 is not the pinned one".to_string(),
            },
        ] {
            let reason = status.grid().expect_err("no verified grid");
            assert!(!reason.is_empty(), "{status:?}");
            assert!(!status.is_verified());
            assert!(
                status.line().starts_with("EGM2008 geoid grid:"),
                "{status:?}"
            );
        }
        assert!(!GeoidStatus::NotChecked.is_settled());
        assert!(GeoidStatus::NotConfigured.is_settled());
    }
}

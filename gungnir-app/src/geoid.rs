// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The pinned geoid grids on the desktop (GAP-108 and D-121; GAP-197 and D-125): each
//! found, verified against its pinned SHA-256 off the render thread at start, and
//! reported on PN-09, one line per grid.
//!
//! **Three grids, each optional** (`gungnir_data::geoid::GeoidModel`): EGM2008 for
//! EGM2008 heights, EGM96 for EGM96 heights, GEOID18 for NAVD88 heights over the
//! conterminous United States. A deployment installs the ones its data needs; a grid it
//! did not install refuses only the heights that need it.
//!
//! **Where each grid is looked for**, in order: the baseline's `geoid_grid_dir`; then
//! `PROJ_DATA`, PROJ's own convention for where its resource files live, and its older
//! `PROJ_LIB`, each of which may list several directories, the first holding that grid's
//! file winning. **A named directory is final**, and one that holds none of the pinned
//! grids is a deployment that named the wrong place: every grid is then refused as
//! missing, with one alert. One that holds some of them has simply not installed the
//! rest, which PN-09 says quietly. **Wherever a grid is found, it is used only if its
//! bytes are the pinned ones** (`gungnir_data::geoid::GeoidGrid`), and a file that is
//! present but different is reported as such -- not quietly used, and not quietly
//! treated as absent.
//!
//! **What waits on it.** The terrain and the point-cloud pair do not start loading until
//! every check has settled, so a file stating geoid heights never reaches placement while
//! the grid it needs is still being hashed. That costs the hash's time once at start --
//! about a fifth of a second for the 80 MB EGM2008 grid in a release build, the three
//! hashed in parallel -- and nothing when no grid is present.
//!
//! **What else the EGM2008 grid corrects: a UAS's height above mean sea level (GAP-196,
//! D-123).** ASTERIX Category 129 states a UAS's height only above mean sea level
//! (I129/090). Once the EGM2008 check settles, [`lend_to_feeds`] hands every bound radar
//! feed the verified grid as a [`gungnir_ingest::geoid::GeoidSeparation`] -- one PROJ
//! lookup service, `gungnir_data::geoid::UndulationService`, shared by them all -- and
//! the feed's adapter adds the EGM2008 separation before it places the report. Until
//! then, and in any deployment without a verified EGM2008 grid or a build without `crs`,
//! every feed holds the reason instead, and the height reaches the picture flagged as
//! mean sea level, never as an ellipsoidal one (D-124). EGM96 and GEOID18 are not lent:
//! D-123 reads an unqualified "mean sea level" as EGM2008. `gungnir-ingest` has no edge to
//! `gungnir-data` (`ARCHITECTURE.md` §7.1), which is why the grid crosses as an interface
//! lent here rather than a call made there.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gungnir_data::geoid::{GeoidGrid, GeoidModel, UndulationService};
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

/// Where one geoid grid stands, for PN-09 and for every conversion that needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeoidStatus {
    /// Before the first tick: the check has not run.
    NotChecked,
    /// Not installed: no named directory holds it and `PROJ_DATA` is unset or holds none.
    /// Heights that need it are refused.
    NotConfigured,
    /// The file is being hashed, off the render thread.
    Verifying { path: PathBuf, source: GridSource },
    /// The pinned grid, verified.
    Verified { grid: GeoidGrid, source: GridSource },
    /// A file was named and is not usable: missing, different from the pinned one, or
    /// unreadable. Heights that need it are refused, with this reason.
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
    /// refused conversion carries into its own refusal.
    ///
    /// # Errors
    ///
    /// Every state but [`GeoidStatus::Verified`], in words.
    pub fn grid(&self, model: GeoidModel) -> Result<&GeoidGrid, String> {
        match self {
            GeoidStatus::Verified { grid, .. } => Ok(grid),
            GeoidStatus::NotChecked => Err("the grid has not been checked yet".to_string()),
            GeoidStatus::NotConfigured => Err(format!(
                "no directory holding {} is named (set geoid_grid_dir in the baseline, or \
                 PROJ_DATA)",
                model.file()
            )),
            GeoidStatus::Verifying { path, .. } => {
                Err(format!("{} is still being verified", path.display()))
            }
            GeoidStatus::Refused { reason, .. } => Err(reason.clone()),
        }
    }

    /// One line for PN-09.
    #[must_use]
    pub fn line(&self, model: GeoidModel) -> String {
        let converts = if cfg!(feature = "crs") {
            ""
        } else {
            "; this build has no crs feature, so it converts no real-world CRS in any case"
        };
        let heights = model.heights();
        // GAP-196 (D-124): without EGM2008, a UAS's height above mean sea level is
        // carried uncorrected and flagged, which the grid's own line says.
        let uas = if model == GeoidModel::Egm2008 {
            ", and a UAS's height above mean sea level is placed uncorrected and flagged"
        } else {
            ""
        };
        match self {
            GeoidStatus::NotChecked => format!("{model} geoid grid: not checked yet"),
            GeoidStatus::NotConfigured => format!(
                "{model} geoid grid: none installed ({} is in no geoid_grid_dir or \
                 PROJ_DATA directory); {heights} are refused{uas}{converts}",
                model.file()
            ),
            GeoidStatus::Verifying { path, source } => format!(
                "{model} geoid grid: verifying {} (from {})",
                path.display(),
                source.words()
            ),
            GeoidStatus::Verified { grid, source } => format!(
                "{model} geoid grid: verified, {} (from {}; SHA-256 {}...){converts}",
                grid.path().display(),
                source.words(),
                &grid.sha256()[..12]
            ),
            GeoidStatus::Refused {
                path,
                source,
                reason,
            } => format!(
                "{model} geoid grid: refused, {} (from {}): {reason}; {heights} are \
                 refused{uas}{converts}",
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

/// Every pinned grid's status, in [`GeoidModel::ALL`]'s order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeoidGrids {
    statuses: [(GeoidModel, GeoidStatus); 3],
}

impl Default for GeoidGrids {
    fn default() -> Self {
        GeoidGrids {
            statuses: GeoidModel::ALL.map(|model| (model, GeoidStatus::NotChecked)),
        }
    }
}

impl GeoidGrids {
    /// One grid's status.
    #[must_use]
    pub fn status(&self, model: GeoidModel) -> &GeoidStatus {
        // `statuses` holds every model of `GeoidModel::ALL`, so the search always finds
        // one; the fallback exists only so no `expect` is needed to say so.
        self.statuses
            .iter()
            .find(|(m, _)| *m == model)
            .map_or(&GeoidStatus::NotChecked, |(_, status)| status)
    }

    /// Set one grid's status: the check's own answer, or a test installing a grid it
    /// verified itself.
    pub fn set(&mut self, model: GeoidModel, status: GeoidStatus) {
        if let Some((_, slot)) = self.statuses.iter_mut().find(|(m, _)| *m == model) {
            *slot = status;
        }
    }

    /// Whether every check has finished; the terrain and the point-cloud pair wait until
    /// they all have.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        self.statuses.iter().all(|(_, status)| status.is_settled())
    }

    /// The verified grid for `model`, cloned for a conversion, or why there is none.
    ///
    /// # Errors
    ///
    /// As [`GeoidStatus::grid`].
    pub fn grid(&self, model: GeoidModel) -> Result<GeoidGrid, String> {
        self.status(model).grid(model).cloned()
    }

    /// Whether `model`'s grid is present and good.
    #[must_use]
    pub fn is_verified(&self, model: GeoidModel) -> bool {
        self.status(model).is_verified()
    }

    /// PN-09's lines: each model, whether its grid is verified, and the words.
    #[must_use]
    pub fn lines(&self) -> Vec<(GeoidModel, bool, String)> {
        self.statuses
            .iter()
            .map(|(model, status)| (*model, status.is_verified(), status.line(*model)))
            .collect()
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

/// Where to look for each grid: the file and where it was named, or `None` for a grid
/// with no candidate at all.
///
/// The baseline's field, when present, is final: a deployment that names a directory
/// means that one, and a grid elsewhere on `PROJ_DATA` is not substituted for a missing
/// one there. `PROJ_DATA` is searched entry by entry for the first that holds each file,
/// so a list whose first entry is PROJ's own `share/proj` still finds a grid installed
/// beside it.
fn candidate(baseline_dir: Option<&str>, model: GeoidModel) -> Option<(PathBuf, GridSource)> {
    if let Some(dir) = baseline_dir {
        return Some((Path::new(dir).join(model.file()), GridSource::Baseline));
    }
    let listed = std::env::var_os("PROJ_DATA").or_else(|| std::env::var_os("PROJ_LIB"))?;
    std::env::split_paths(&listed)
        .map(|dir| dir.join(model.file()))
        .find(|path| path.is_file())
        .map(|path| (path, GridSource::ProjData))
}

/// Start the checks on the first tick. Idempotent, and a status already set -- a test
/// that installs a grid it verified itself, say -- is left alone, while the grids it did
/// not set are checked as usual.
pub fn start(state: &mut AppState) {
    if !state.geoid_checks.is_empty() || state.geoid.is_settled() {
        return;
    }
    let named = state.config.geoid_grid_dir.clone();
    let candidates: Vec<(GeoidModel, Option<(PathBuf, GridSource)>)> = GeoidModel::ALL
        .into_iter()
        .filter(|model| matches!(state.geoid.status(*model), GeoidStatus::NotChecked))
        .map(|model| (model, candidate(named.as_deref(), model)))
        .collect();
    // A named directory holding none of the pinned grids is a deployment that named the
    // wrong place: every grid is refused as missing, and one alert says where it looked.
    // One holding some of them has just not installed the rest.
    let named_holds_none = named.is_some()
        && candidates
            .iter()
            .all(|(_, c)| c.as_ref().is_none_or(|(path, _)| !path.is_file()));
    if named_holds_none {
        if let Some(dir) = &named {
            state.alerts.push(format!(
                "geoid grids: the baseline's geoid_grid_dir {dir} holds none of the pinned \
                 grids ({}), so every geoid height is refused",
                GeoidModel::ALL.map(GeoidModel::file).join(", ")
            ));
        }
    }
    for (model, found) in candidates {
        let Some((path, source)) = found else {
            state.geoid.set(model, GeoidStatus::NotConfigured);
            continue;
        };
        if !path.is_file() {
            // Nothing to hash: settle now, so a deployment whose named grid is missing
            // starts its terrain on the first frame like any other.
            let status = if named_holds_none {
                GeoidStatus::Refused {
                    reason: GeoidGrid::verify(&path, model.sha256())
                        .err()
                        .map_or_else(|| "it is not there".to_string(), |e| e.to_string()),
                    path,
                    source,
                }
            } else {
                GeoidStatus::NotConfigured
            };
            state.geoid.set(model, status);
            continue;
        }
        let (tx, rx) = crossbeam_channel::bounded(1);
        let checked = path.clone();
        let spawned = std::thread::Builder::new()
            .name("geoid-grid-check".to_string())
            .spawn(move || {
                let result = GeoidGrid::verify(&checked, model.sha256()).map_err(|e| e.to_string());
                // The receiver is gone only if the state was dropped; nothing to tell then.
                let _ = tx.send(result);
            });
        match spawned {
            Ok(_) => {
                state
                    .geoid
                    .set(model, GeoidStatus::Verifying { path, source });
                state.geoid_checks.push((model, rx));
            }
            Err(e) => {
                let reason = format!("the check could not be started: {e}");
                state.alerts.push(format!(
                    "{model} geoid grid {} refused: {reason}",
                    path.display()
                ));
                state.geoid.set(
                    model,
                    GeoidStatus::Refused {
                        path,
                        source,
                        reason,
                    },
                );
            }
        }
    }
}

/// Poll the checks; on the tick, so hashing never stalls a frame. Starts them on the
/// first call, and says on the alert list what each found when it finishes, unless it
/// found the grid good.
pub fn poll(state: &mut AppState) {
    start(state);
    let mut pending = std::mem::take(&mut state.geoid_checks);
    pending.retain(|(model, rx)| {
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(crossbeam_channel::TryRecvError::Empty) => return true,
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                Err("the check stopped without an answer".to_string())
            }
        };
        if let GeoidStatus::Verifying { path, source } = state.geoid.status(*model).clone() {
            settle(state, *model, path, source, result);
        }
        false
    });
    state.geoid_checks = pending;
}

/// The reason every bound feed holds before the grid check has settled.
pub const NOT_YET_CHECKED: &str = "the EGM2008 geoid grid has not been checked yet";

/// The verified EGM2008 grid as the ingest path sees it (GAP-196): one PROJ lookup
/// service, lent to every bound feed through its [`gungnir_ingest::geoid::GeoidHandle`].
#[derive(Debug, Clone)]
pub struct Egm2008Separation(pub UndulationService);

impl GeoidSeparation for Egm2008Separation {
    fn model(&self) -> &'static str {
        GeoidModel::Egm2008.name()
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
    let status = state.geoid.status(GeoidModel::Egm2008).clone();
    if state.geoid_lent.as_ref() == Some(&status) {
        return;
    }
    state.geoid_lent = Some(status);
    if state.feed_stats.is_empty() {
        // No feed holds the handle, so no lookup service is started for nobody.
        return;
    }
    let reason = match state.geoid.grid(GeoidModel::Egm2008) {
        Ok(grid) => match UndulationService::start(&grid) {
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

/// Record a check's answer, and put a refusal on the alert list: a grid a deployment
/// installed and cannot use is something an operator is told about.
fn settle(
    state: &mut AppState,
    model: GeoidModel,
    path: PathBuf,
    source: GridSource,
    result: Result<GeoidGrid, String>,
) {
    let status = match result {
        Ok(grid) => GeoidStatus::Verified { grid, source },
        Err(reason) => {
            state.alerts.push(format!(
                "{model} geoid grid {} refused: {reason}",
                path.display()
            ));
            GeoidStatus::Refused {
                path,
                source,
                reason,
            }
        }
    };
    state.geoid.set(model, status);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_named_directory_is_final_and_says_where_it_came_from() {
        for model in GeoidModel::ALL {
            let (path, source) = candidate(Some("/opt/gungnir/geoid"), model).expect("named");
            assert_eq!(source, GridSource::Baseline);
            assert!(path.ends_with(model.file()), "{}", path.display());
        }
    }

    #[test]
    fn every_unsettled_state_refuses_a_conversion_with_a_reason() {
        for model in GeoidModel::ALL {
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
                let reason = status.grid(model).expect_err("no verified grid");
                assert!(!reason.is_empty(), "{status:?}");
                assert!(!status.is_verified());
                assert!(
                    status
                        .line(model)
                        .starts_with(&format!("{} geoid grid:", model.name())),
                    "{status:?}"
                );
            }
        }
        assert!(!GeoidStatus::NotChecked.is_settled());
        assert!(GeoidStatus::NotConfigured.is_settled());
    }

    #[test]
    fn the_grids_are_settled_only_when_every_one_is() {
        let mut grids = GeoidGrids::default();
        assert!(!grids.is_settled());
        grids.set(GeoidModel::Egm2008, GeoidStatus::NotConfigured);
        grids.set(GeoidModel::Egm96, GeoidStatus::NotConfigured);
        assert!(!grids.is_settled(), "GEOID18 is still unchecked");
        grids.set(GeoidModel::Geoid18Conus, GeoidStatus::NotConfigured);
        assert!(grids.is_settled());
        let lines = grids.lines();
        assert_eq!(lines.len(), 3);
        assert!(lines[2].2.contains("NAVD88"), "{:?}", lines[2]);
    }
}

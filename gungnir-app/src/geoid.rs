// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The pinned geoid grids on the desktop (GAP-108 and D-121; GAP-197 and D-125): each
//! found, verified against its pinned SHA-256 off the render thread at start, and
//! reported on PN-09, one line per grid.
//!
//! **Seven grids, each optional** (`gungnir_data::geoid::PinnedGrid`): EGM2008 for
//! EGM2008 heights, EGM96 for EGM96 heights, GEOID18 for NAVD88 heights over the
//! conterminous United States, and since GAP-200 (D-128) the four NADCON5 steps that carry
//! an older NAD83 realisation to NAD83(2011). A deployment installs the ones its data
//! needs; a grid it did not install refuses only the heights, or the positions, that need
//! it.
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

use std::path::PathBuf;
use std::sync::Arc;

use gungnir_data::geoid::{GeoidGrid, GeoidModel, PinnedGrid, UndulationService};
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
    pub fn grid(&self, pinned: impl Into<PinnedGrid>) -> Result<&GeoidGrid, String> {
        let pinned = pinned.into();
        match self {
            GeoidStatus::Verified { grid, .. } => Ok(grid),
            GeoidStatus::NotChecked => Err("the grid has not been checked yet".to_string()),
            GeoidStatus::NotConfigured => Err(format!(
                "no directory holding {} is named (set geoid_grid_dir in the baseline, or \
                 PROJ_DATA)",
                pinned.file()
            )),
            GeoidStatus::Verifying { path, .. } => {
                Err(format!("{} is still being verified", path.display()))
            }
            GeoidStatus::Refused { reason, .. } => Err(reason.clone()),
        }
    }

    /// One line for PN-09.
    #[must_use]
    pub fn line(&self, pinned: impl Into<PinnedGrid>) -> String {
        let pinned = pinned.into();
        let converts = if cfg!(feature = "crs") {
            ""
        } else {
            "; this build has no crs feature, so it converts no real-world CRS in any case"
        };
        let heights = pinned.converts();
        // A geoid model's line says so; a NADCON5 step's names the grid alone (GAP-200).
        let label = match pinned {
            PinnedGrid::Geoid(_) => format!("{pinned} geoid grid"),
            PinnedGrid::Nadcon5(_) => format!("{pinned} grid"),
        };
        // GAP-196 (D-124): without EGM2008, a UAS's height above mean sea level is
        // carried uncorrected and flagged, which the grid's own line says.
        let uas = if pinned == PinnedGrid::Geoid(GeoidModel::Egm2008) {
            ", and a UAS's height above mean sea level is placed uncorrected and flagged"
        } else {
            ""
        };
        match self {
            GeoidStatus::NotChecked => format!("{label}: not checked yet"),
            GeoidStatus::NotConfigured => format!(
                "{label}: none installed ({} is in no geoid_grid_dir or PROJ_DATA \
                 directory); {heights} are refused{uas}{converts}",
                pinned.file()
            ),
            GeoidStatus::Verifying { path, source } => format!(
                "{label}: verifying {} (from {})",
                path.display(),
                source.words()
            ),
            GeoidStatus::Verified { grid, source } => format!(
                "{label}: verified, {} (from {}; SHA-256 {}...){converts}",
                grid.path().display(),
                source.words(),
                &grid.sha256()[..12]
            ),
            GeoidStatus::Refused {
                path,
                source,
                reason,
            } => format!(
                "{label}: refused, {} (from {}): {reason}; {heights} are \
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

/// Every pinned grid's status, in [`PinnedGrid::ALL`]'s order: the three geoid models and
/// the four NADCON5 steps (GAP-200).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeoidGrids {
    statuses: [(PinnedGrid, GeoidStatus); 7],
}

impl Default for GeoidGrids {
    fn default() -> Self {
        GeoidGrids {
            statuses: PinnedGrid::ALL.map(|pinned| (pinned, GeoidStatus::NotChecked)),
        }
    }
}

impl GeoidGrids {
    /// One grid's status.
    #[must_use]
    pub fn status(&self, pinned: impl Into<PinnedGrid>) -> &GeoidStatus {
        let pinned = pinned.into();
        // `statuses` holds every grid of `PinnedGrid::ALL`, so the search always finds
        // one; the fallback exists only so no `expect` is needed to say so.
        self.statuses
            .iter()
            .find(|(m, _)| *m == pinned)
            .map_or(&GeoidStatus::NotChecked, |(_, status)| status)
    }

    /// Set one grid's status: the check's own answer, or a test installing a grid it
    /// verified itself.
    pub fn set(&mut self, pinned: impl Into<PinnedGrid>, status: GeoidStatus) {
        let pinned = pinned.into();
        if let Some((_, slot)) = self.statuses.iter_mut().find(|(m, _)| *m == pinned) {
            *slot = status;
        }
    }

    /// Whether every check has finished; the terrain and the point-cloud pair wait until
    /// they all have.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        self.statuses.iter().all(|(_, status)| status.is_settled())
    }

    /// The verified grid for `pinned`, cloned for a conversion, or why there is none.
    ///
    /// # Errors
    ///
    /// As [`GeoidStatus::grid`].
    pub fn grid(&self, pinned: impl Into<PinnedGrid>) -> Result<GeoidGrid, String> {
        let pinned = pinned.into();
        self.status(pinned).grid(pinned).cloned()
    }

    /// Whether `pinned`'s grid is present and good.
    #[must_use]
    pub fn is_verified(&self, pinned: impl Into<PinnedGrid>) -> bool {
        self.status(pinned).is_verified()
    }

    /// PN-09's lines: each grid, whether it is verified, and the words.
    #[must_use]
    pub fn lines(&self) -> Vec<(PinnedGrid, bool, String)> {
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

/// The NAD83 realisation a baseline's `nad83_realisation` declares, `None` when it
/// declares none (GAP-200, D-128; `gungnir_config::Nad83Realisation`, read into
/// `gungnir-data`'s own type).
///
/// # Errors
///
/// The string does not parse, named by `field`. Validation refuses such a baseline first.
pub fn declared_realisation(
    field: &str,
    declared: Option<&str>,
) -> Result<Option<gungnir_data::nad83::Realisation>, String> {
    use gungnir_config::Nad83Realisation;
    declared
        .map(|d| {
            let Nad83Realisation(code) = d
                .parse::<Nad83Realisation>()
                .map_err(|reason| format!("{field} {reason}"))?;
            gungnir_data::nad83::Realisation::from_geographic_epsg(code).ok_or_else(|| {
                format!("{field} names EPSG:{code}, which gungnir-data has no realisation for")
            })
        })
        .transpose()
}

/// The epoch a converted file is carried to WGS 84 at (GAP-200, D-127): the baseline's
/// `frame_epoch` when it pins one, and otherwise the current date.
#[must_use]
pub fn frame_epoch(config: &gungnir_config::ConfigBaseline) -> f64 {
    config
        .frame_epoch
        .unwrap_or_else(gungnir_data::nad83::current_epoch)
}

/// What a file's horizontal system says its datum is, read through PROJ, or refused by name
/// in a build without `crs`: one function for the desktop's placement and the node's
/// (DN-36 §9).
pub use gungnir_analytics::placement::file_datum;

/// Where to look for each grid: the file and where it was named, or `None` for a grid
/// with no candidate at all.
///
/// The baseline's field, when present, is final: a deployment that names a directory
/// means that one, and a grid elsewhere on `PROJ_DATA` is not substituted for a missing
/// one there. `PROJ_DATA` is searched entry by entry for the first that holds each file,
/// so a list whose first entry is PROJ's own `share/proj` still finds a grid installed
/// beside it.
fn candidate(baseline_dir: Option<&str>, model: PinnedGrid) -> Option<(PathBuf, GridSource)> {
    // The rule is `gungnir-data`'s, shared with the node's own check (GAP-198).
    gungnir_data::geoid::locate(baseline_dir, model.file()).map(|(path, named)| {
        let source = if named {
            GridSource::Baseline
        } else {
            GridSource::ProjData
        };
        (path, source)
    })
}

/// Start the checks on the first tick. Idempotent, and a status already set -- a test
/// that installs a grid it verified itself, say -- is left alone, while the grids it did
/// not set are checked as usual.
pub fn start(state: &mut AppState) {
    if !state.geoid_checks.is_empty() || state.geoid.is_settled() {
        return;
    }
    let named = state.config.geoid_grid_dir.clone();
    let candidates: Vec<(PinnedGrid, Option<(PathBuf, GridSource)>)> = PinnedGrid::ALL
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
                "pinned grids: the baseline's geoid_grid_dir {dir} holds none of the pinned \
                 grids ({}), so every geoid height and every position in an older NAD83 \
                 realisation is refused",
                PinnedGrid::ALL.map(PinnedGrid::file).join(", ")
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
                state
                    .alerts
                    .push(format!("{model} grid {} refused: {reason}", path.display()));
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
    model: PinnedGrid,
    path: PathBuf,
    source: GridSource,
    result: Result<GeoidGrid, String>,
) {
    let status = match result {
        Ok(grid) => GeoidStatus::Verified { grid, source },
        Err(reason) => {
            state
                .alerts
                .push(format!("{model} grid {} refused: {reason}", path.display()));
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
        for model in PinnedGrid::ALL {
            let (path, source) = candidate(Some("/opt/gungnir/geoid"), model).expect("named");
            assert_eq!(source, GridSource::Baseline);
            assert!(path.ends_with(model.file()), "{}", path.display());
        }
    }

    #[test]
    fn every_unsettled_state_refuses_a_conversion_with_a_reason() {
        for model in PinnedGrid::ALL {
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
                let label = match model {
                    PinnedGrid::Geoid(_) => format!("{} geoid grid:", model.name()),
                    PinnedGrid::Nadcon5(_) => format!("{} grid:", model.name()),
                };
                assert!(status.line(model).starts_with(&label), "{status:?}");
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
        assert!(!grids.is_settled(), "the NADCON5 grids are still unchecked");
        for step in gungnir_data::nad83::Nadcon5Step::ALL {
            grids.set(step, GeoidStatus::NotConfigured);
        }
        assert!(grids.is_settled());
        let lines = grids.lines();
        assert_eq!(lines.len(), 7);
        assert!(lines[2].2.contains("NAVD88"), "{:?}", lines[2]);
        assert!(
            lines[6]
                .2
                .contains("NADCON5 NAD83(NSRS2007) to NAD83(2011) grid"),
            "{:?}",
            lines[6]
        );
    }

    /// The baseline's realisation codes are exactly the realisations gungnir-data
    /// converts (GAP-200): `gungnir-config` may not depend on `gungnir-data`, so the two
    /// lists are held equal here.
    #[test]
    fn the_baselines_realisation_codes_are_gungnir_datas() {
        use gungnir_data::nad83::Realisation;
        assert_eq!(
            gungnir_config::Nad83Realisation::CODES.len(),
            Realisation::ALL.len()
        );
        for (code, name) in gungnir_config::Nad83Realisation::CODES {
            let r = Realisation::from_geographic_epsg(code).expect("a realisation");
            assert_eq!(r.name(), name);
            let declared =
                declared_realisation("terrain.nad83_realisation", Some(&format!("epsg:{code}")));
            assert_eq!(declared, Ok(Some(r)));
        }
        assert_eq!(
            declared_realisation("terrain.nad83_realisation", None),
            Ok(None)
        );
        let refused = declared_realisation("terrain.nad83_realisation", Some("epsg:4326"))
            .expect_err("WGS 84 is not a NAD83 realisation");
        assert!(
            refused.starts_with("terrain.nad83_realisation"),
            "{refused}"
        );
    }
}

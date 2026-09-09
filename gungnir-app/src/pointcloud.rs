// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Point clouds on the desktop (GAP-098): the baseline names a source and a target
//! file, the loader thread reads both off the render thread, and the viewport draws
//! whatever `DataStore.point_clouds` holds once the pair is complete.
//!
//! **A pair, not a file.** `gungnir-data-fusion::CpuIcp` (GAP-024, independent of this
//! and gated on synthetic clouds of its own) registers a source cloud onto a target, so
//! a single configured file would be reachable but useless: there would be nothing to
//! align it to. `gungnir_config::PointCloudConfig` names both, and this module treats
//! the pair as one unit that is loading, loaded, or failed together -- a cloud loaded
//! alone while its partner failed is not a pair anything downstream can use, so it is
//! not kept.
//!
//! **[`register`] is GAP-024's missing caller.** Once a pair is [`PointCloudStatus::
//! Loaded`], `crate::fusion::FusionBackend::engine_for` finally has a real target to
//! build against; [`register`] is what calls it, from `update::tick` right after
//! [`poll`], and [`registration_line`] is what PN-09 reads to say which backend did.

use gungnir_config::{PointCloudConfig, PointCloudFileConfig};
use gungnir_data::pointcloud::crs::PointCloudCrs;
use gungnir_data::pointcloud::PointBuffer;
use gungnir_data::{DataError, LoadRequest, LoadResult};

use crate::fusion::FusionBackend;
use crate::state::AppState;

/// Where the configured point-cloud pair stands, for PN-09-style reporting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PointCloudStatus {
    /// The baseline names no point-cloud pair.
    NotConfigured,
    /// Both files are requested; neither, one, or (transiently, between a `try_recv`
    /// and the next) both may have arrived off the channel.
    Loading { source: String, target: String },
    Loaded {
        source: String,
        target: String,
        source_points: usize,
        target_points: usize,
    },
    /// Either file failed to load; the reason is on screen and `DataStore.point_clouds`
    /// is left empty, since a lone cloud is not the pair registration needs and a
    /// partially loaded pair claiming readiness would be the failure this gate exists
    /// to avoid.
    Failed { path: String, reason: String },
}

impl PointCloudStatus {
    #[must_use]
    pub fn is_loaded(&self) -> bool {
        matches!(self, PointCloudStatus::Loaded { .. })
    }
}

/// The placed point-cloud pair for the viewport, once both files have loaded. Empty
/// until then -- the same convention `LayerInputs::predictions` and `::gaps` use for
/// "nothing to show yet" -- which draws nothing and claims nothing.
///
/// Takes the two fields rather than the state, so the viewport can borrow its own state
/// mutably in the same call (mirrors [`crate::terrain::layer`]).
#[must_use]
pub fn layers<'a>(
    status: &PointCloudStatus,
    data: &'a gungnir_data::DataStore,
) -> Vec<gungnir_viewport3d::layers::PointCloudLayer<'a>> {
    if !status.is_loaded() {
        return Vec::new();
    }
    data.point_clouds
        .iter()
        .map(|cloud| gungnir_viewport3d::layers::PointCloudLayer {
            positions: &cloud.positions,
        })
        .collect()
}

/// The `LoadRequest` a configured file calls for: a bounded COPC read when it names
/// bounds, a plain LAS/LAZ read otherwise. `gungnir_config::validate` is what enforces
/// that a COPC file always carries bounds and a non-COPC file never does, so this
/// reads as a plain dispatch rather than a second copy of that rule.
fn request_for(file: &PointCloudFileConfig) -> LoadRequest {
    match file.copc_bounds {
        Some(bounds) => LoadRequest::CopcBounded(std::path::PathBuf::from(&file.path), bounds),
        None => LoadRequest::PointCloud(std::path::PathBuf::from(&file.path)),
    }
}

/// What a loaded cloud needs before it can be drawn (GAP-102, D-41): the reconciliation
/// of what the baseline's `frame` claims with what the file's own CRS VLRs declare.
///
/// A pure function of the three things that decide it, so every branch is checkable
/// without a loader, a file, or a `libproj` build -- which matters here more than usual,
/// because the conversion arm is the only one a default build cannot execute.
#[derive(Debug, Clone, PartialEq)]
pub enum Placement {
    /// The cloud is already in the deployment's local ENU metres and is drawn as it
    /// stands. This is what every point cloud did before GAP-102, and it is still the
    /// default; what changed is that the claim is now checked against the file.
    AsLoaded,
    /// The cloud is in a real-world system and must be converted. `source` is the
    /// definition to hand PROJ -- the file's own WKT where it has one, since a compound
    /// WKT states the vertical system that a bare horizontal code does not.
    Convert {
        source: String,
        vertical_metres: f64,
    },
    /// Refused, with the reason an operator reads. Mirrors `terrain::placement_refusal`:
    /// a cloud that cannot be placed is named rather than drawn in the wrong place.
    Refused(String),
}

/// Reconcile the baseline's declared frame with the file's own.
///
/// `declared_epsg` is `PointCloudConfig::declared_epsg` -- `None` for `"local-enu"`.
/// `file` is what the loader read out of the file's CRS VLRs. `has_origin` is whether
/// the baseline declared `origin`, which is the anchor any conversion lands on.
///
/// **The file wins where the two disagree, and the disagreement is refused rather than
/// resolved.** A baseline claiming `"local-enu"` for a file whose own tags name a
/// projected system is the case this exists for: before GAP-102 nothing could contradict
/// that claim, and the cloud was drawn at whatever coordinates the file happened to
/// hold. It is the same rule `terrain.rs`'s `placement_refusal` has always applied to a
/// DEM, arriving here now that a point cloud finally carries a CRS to check.
#[must_use]
pub fn placement(
    declared_epsg: Option<u32>,
    file: Option<&PointCloudCrs>,
    has_origin: bool,
) -> Placement {
    let Some(code) = declared_epsg else {
        // The baseline says the file is already in local metres.
        return match file.and_then(PointCloudCrs::proj_definition) {
            Some(_) => Placement::Refused(format!(
                "the file declares its own coordinate reference system ({}), not the \
                 local frame point_cloud.frame claims; set point_cloud.frame to \
                 \"epsg:<code>\" to have it converted, or reproject the file to the \
                 deployment's local metres",
                describe(file)
            )),
            // Nothing declared contradicts nothing: the baseline's word stands, exactly
            // as it did before this gap.
            None => Placement::AsLoaded,
        };
    };
    if !has_origin {
        return Placement::Refused(format!(
            "point_cloud.frame is \"epsg:{code}\", which needs converting into the \
             deployment's local frame, and the baseline declares no origin to convert \
             onto; declare origin, or prepare the files in local metres and set \
             point_cloud.frame to \"local-enu\""
        ));
    }
    if let Some(declared) = file {
        if !declared.agrees_with_epsg(code) {
            return Placement::Refused(format!(
                "point_cloud.frame claims EPSG:{code} and the file declares {}; the \
                 file's own tags are what its coordinates actually are, so this is a \
                 baseline to correct rather than a file to override",
                describe(file)
            ));
        }
    }
    // The file's own definition where it has one: it is richer than the baseline's bare
    // code, because a compound WKT states the vertical system too.
    let source = file
        .and_then(PointCloudCrs::proj_definition)
        .unwrap_or_else(|| format!("EPSG:{code}"));
    let vertical_metres = match file {
        // A file that declares a system but no readable unit for its heights is refused,
        // not assumed: there the file had something to say and it could not be read, and
        // a wrong vertical unit is a silent factor-of-three error in every height.
        Some(declared) => match declared.vertical_unit_metres() {
            Some(metres) => metres,
            None => {
                return Placement::Refused(format!(
                    "the file declares {} but no unit this build can read for its \
                     heights, so they cannot be scaled to metres",
                    describe(file)
                ))
            }
        },
        // A file that declares nothing at all is taken to store metres. This is the one
        // assumption in the path and it is named rather than buried: a LAS file with no
        // CRS VLR carries no unit either, the baseline's code names only the horizontal
        // system, and metres is what such a file almost always holds. A deployment for
        // which that is wrong should reproject the file, which gives it a declaration
        // and removes the guess.
        None => 1.0,
    };
    Placement::Convert {
        source,
        vertical_metres,
    }
}

/// A file's declaration in the words an operator reads in a refusal.
fn describe(file: Option<&PointCloudCrs>) -> String {
    match file {
        Some(PointCloudCrs::Wkt(wkt)) => {
            // The name is the first quoted string of a WKT node, which is what a reader
            // recognises; the whole WKT runs to hundreds of characters and would bury
            // the rest of the message.
            let name = wkt
                .split_once('"')
                .and_then(|(_, rest)| rest.split_once('"'))
                .map_or("an unnamed system", |(name, _)| name);
            format!("{name:?}")
        }
        Some(PointCloudCrs::Geokeys(crs)) => format!("{crs:?}"),
        None => "nothing".to_string(),
    }
}

/// Apply this deployment's [`placement`] to one freshly loaded cloud.
///
/// `Err` carries the reason for `PointCloudStatus::Failed` and the operator alert, the
/// same shape a load error already takes.
fn place(state: &AppState, buffer: PointBuffer) -> Result<PointBuffer, String> {
    let declared_epsg = state
        .config
        .point_cloud
        .as_ref()
        .and_then(PointCloudConfig::declared_epsg);
    match placement(
        declared_epsg,
        buffer.crs.as_ref(),
        state.config.origin.is_some(),
    ) {
        Placement::AsLoaded => Ok(buffer),
        Placement::Refused(reason) => Err(reason),
        Placement::Convert {
            source,
            vertical_metres,
        } => {
            // `placement` already refused the no-origin case, so this is defensive
            // rather than a state a running deployment reaches; a `let-else` keeps the
            // no-`expect` rule without pretending the combination cannot occur.
            let Some(frame) = crate::sustainment::local_frame(state) else {
                return Err("the baseline declares no origin to convert onto".to_string());
            };
            convert(&buffer, &source, vertical_metres, &frame).map_err(|e| e.to_string())
        }
    }
}

/// The conversion, when this binary was built with the `crs` feature.
#[cfg(feature = "crs")]
fn convert(
    buffer: &PointBuffer,
    source: &str,
    vertical_metres: f64,
    frame: &gungnir_model::LocalFrame,
) -> Result<PointBuffer, DataError> {
    gungnir_data::pointcloud::crs::to_local_enu(
        buffer,
        source,
        vertical_metres,
        &|[lat_rad, lon_rad, alt_m]| {
            frame.to_enu(gungnir_model::Geodetic {
                lat_rad,
                lon_rad,
                alt_m,
            })
        },
    )
}

/// The same call in a binary built without the `crs` feature: refused by name, with the
/// reason being this build rather than anything about the file.
///
/// **Not a silent pass-through, deliberately.** Returning the cloud unconverted would
/// draw a projected coordinate as though it were metres east of the origin, which is the
/// confidently-wrong answer this workspace's health flags exist to refuse.
#[cfg(not(feature = "crs"))]
fn convert(
    _buffer: &PointBuffer,
    _source: &str,
    _vertical_metres: f64,
    _frame: &gungnir_model::LocalFrame,
) -> Result<PointBuffer, DataError> {
    Err(DataError::NotImplemented {
        what: "converting a point cloud out of its own coordinate reference system",
        waiting_on: "a build with gungnir-data's `crs` feature, which links libproj",
    })
}

/// Start loading the configured point-cloud pair off the render thread. Idempotent: the
/// first tick calls it, and a load already started or finished is left alone.
pub fn start(state: &mut AppState) {
    if state.pointcloud_loader.is_some()
        || !matches!(state.point_cloud, PointCloudStatus::NotConfigured)
    {
        return;
    }
    let Some(pair) = state.config.point_cloud.clone() else {
        return;
    };
    let (requests, results) = gungnir_data::spawn_loader();
    // Both are sent before either is read, so the worker moves straight to the second
    // file once the first is done rather than waiting for a tick to hand it the next
    // request.
    let sent = requests
        .send(request_for(&pair.source))
        .and_then(|()| requests.send(request_for(&pair.target)));
    match sent {
        Ok(()) => {
            state.point_cloud = PointCloudStatus::Loading {
                source: pair.source.path.clone(),
                target: pair.target.path.clone(),
            };
            state.pointcloud_loader = Some((requests, results));
        }
        Err(err) => {
            state.point_cloud = PointCloudStatus::Failed {
                path: pair.source.path.clone(),
                reason: format!("the loader thread is gone: {err}"),
            };
        }
    }
}

/// Poll the loader; on the tick, so a slow file never stalls a frame. Starts the load
/// on the first call, and drains every result already waiting -- never more than two,
/// since that is all `start` ever sends for one pair.
pub fn poll(state: &mut AppState) {
    start(state);
    loop {
        let Some((_, results)) = state.pointcloud_loader.as_ref() else {
            return;
        };
        let Ok(result) = results.try_recv() else {
            return;
        };
        apply_result(state, result);
    }
}

/// One `LoadResult` off the channel, matched against the source/target still named by
/// [`PointCloudStatus::Loading`]. Both other cases (`NotConfigured`, `Loaded`, `Failed`)
/// mean this arrived after the pair was already settled -- which `poll`'s loop exit
/// prevents -- so they are left alone rather than asserted against.
fn apply_result(state: &mut AppState, result: LoadResult) {
    let (LoadResult::PointCloud(loaded) | LoadResult::CopcBounded(loaded)) = result else {
        return;
    };
    let PointCloudStatus::Loading { source, target } = state.point_cloud.clone() else {
        return;
    };
    // `start` sends the source request before the target one, and `spawn_loader`'s
    // single worker thread drains its one request channel strictly in order, so the
    // result channel delivers them in the same order they were sent. `data.point_clouds`
    // holds only this pair while a load is in flight -- nothing else has ever written to
    // it (GAP-098 is the first thing that does) -- so its length is which of the two this
    // result is for: empty means it is the source's, one entry means it is the target's.
    let awaiting_source = state.data.point_clouds.is_empty();
    let path = if awaiting_source { &source } else { &target };
    // GAP-102: a cloud is reconciled against the frame the baseline claims before it is
    // kept, so a file in a real-world system is converted or refused rather than drawn
    // at whatever coordinates it happens to hold.
    let loaded = loaded
        .map_err(|e| e.to_string())
        .and_then(|b| place(state, b));
    match loaded {
        Ok(buffer) => {
            state.data.point_clouds.push(buffer);
            if !awaiting_source {
                let source_points = state.data.point_clouds[0].positions.len();
                let target_points = state.data.point_clouds[1].positions.len();
                state.point_cloud = PointCloudStatus::Loaded {
                    source,
                    target,
                    source_points,
                    target_points,
                };
                state.pointcloud_loader = None;
            }
            // Otherwise still waiting on the other half; status stays `Loading`.
        }
        Err(reason) => {
            // All-or-nothing: a cloud that arrived before its partner failed is not a
            // pair anything downstream can use.
            state.data.point_clouds.clear();
            state.point_cloud = PointCloudStatus::Failed {
                path: path.clone(),
                reason: reason.clone(),
            };
            state
                .alerts
                .push(format!("point cloud {path} not loaded: {reason}"));
            state.pointcloud_loader = None;
        }
    }
}

/// The largest number of ICP iterations one registration attempt is allowed before
/// `PointCloudFusion::step` refuses further progress as `FusionError::Divergence`
/// (`rust-3d-data-ecosystem-build-vs-adopt.md` §3.4: bounded so a poorly converging
/// registration never stalls the render loop). [`register`] spends one `step` call
/// per tick, so this is also, in effect, how many frames a pair is given to converge
/// before this module reports it gave up rather than retrying forever.
const MAX_ICP_ITERATIONS: u32 = 50;

/// What this tick's registration attempt did with the loaded pair (GAP-024): distinct
/// from [`PointCloudStatus`], which only says the *pair* finished loading.
/// `NoPair` covers "not configured", "still loading" and "failed" alike -- the same
/// all-or-nothing reasoning [`PointCloudStatus`] itself uses, since none of those three
/// is a pair complete enough to register.
#[derive(Debug, Clone, PartialEq)]
pub enum RegistrationOutcome {
    /// `state.point_cloud` is not [`PointCloudStatus::Loaded`]: nothing complete to
    /// register against.
    NoPair,
    /// The registration engine (GPU or CPU, per `AppState::fusion`) stepped this tick.
    Registered {
        transform: nalgebra::Isometry3<f32>,
        converged: bool,
        inlier_ratio: f32,
    },
    /// The engine could not be built, or could not step, and why -- never silently
    /// dropped (CLAUDE.md's rule against a health flag or a test claiming more than
    /// what actually ran).
    Failed { reason: String },
}

/// Build or continue this tick's registration of the loaded pair (GAP-024): the
/// caller `crate::fusion::FusionBackend::engine_for` was missing, now that GAP-098
/// gives it a real pair to build against. Called from `update::tick`, right after
/// [`poll`], so a pair that completes loading this tick is registered the same tick
/// rather than one frame late.
///
/// **All-or-nothing, matching GAP-098.** A no-op -- `state.registration_engine` is
/// dropped and [`RegistrationOutcome::NoPair`] recorded -- unless `state.point_cloud`
/// is [`PointCloudStatus::Loaded`]; an incomplete pair (only a source loaded, or
/// neither) has nothing to register against, the same discipline [`apply_result`]
/// already applies to what it keeps in `DataStore.point_clouds`.
///
/// **Built once per pair, stepped every tick after.** `FusionBackend::engine_for`
/// builds a fresh engine on every call it gets (uploading the target and its spatial
/// hash on the GPU path), so this calls it only on the first tick a pair is complete
/// (`state.registration_engine` still `None`) and keeps that same engine for every
/// tick after. `PointCloudFusion::step`'s own doc comment is explicit that one call is
/// one ICP iteration, meant to be spread over many frames rather than run to
/// convergence inside one -- the same non-blocking shape [`crate::terrain::poll`] and
/// [`poll`] already give a slow load.
pub fn register(state: &mut AppState) {
    if !state.point_cloud.is_loaded() {
        // Not a pair (not yet, or not any more): nothing to hold an engine open for.
        // Cleared rather than left stale, so a pair that somehow un-loads never leaves
        // behind a result implying registration is still running.
        state.registration_engine = None;
        state.registration = RegistrationOutcome::NoPair;
        return;
    }

    if state.registration_engine.is_none() {
        let handle = state.runtime.handle().clone();
        // `state.point_cloud.is_loaded()` above and GAP-098's own invariant (source
        // first, target second, and never one without the other) are what make these
        // two indices safe without a length check here.
        let target = &state.data.point_clouds[1];
        match state.fusion.engine_for(&handle, target, MAX_ICP_ITERATIONS) {
            Ok(engine) => state.registration_engine = Some(engine),
            Err(err) => {
                state.registration = RegistrationOutcome::Failed {
                    reason: err.to_string(),
                };
                return;
            }
        }
    }

    // The branch above either found an engine already in place or just built one on
    // success; a build failure already returned. Never `unwrap`/`expect`: a `let-else`
    // that leaves this tick's registration unchanged is the honest way to handle a
    // combination this module's own control flow does not actually produce.
    let Some(engine) = state.registration_engine.as_mut() else {
        return;
    };
    let source = &state.data.point_clouds[0];
    state.registration = match engine.step(source) {
        Ok(step) => RegistrationOutcome::Registered {
            transform: step.transform,
            converged: step.converged,
            inlier_ratio: step.inlier_ratio,
        },
        Err(err) => RegistrationOutcome::Failed {
            reason: err.to_string(),
        },
    };
}

/// The PN-09 line for which backend is registering the configured pair, and why
/// (GAP-024). A pure function of the two pieces of state that decide it, so it is
/// checkable against synthetic [`PointCloudStatus`]/[`FusionBackend`] values rather
/// than a real load or a real `wgpu` device.
#[must_use]
pub fn registration_line<'a>(
    point_cloud: &PointCloudStatus,
    fusion: &'a FusionBackend,
) -> gungnir_ui::panels::sensor_health::PointCloudRegistrationLine<'a> {
    use gungnir_ui::panels::sensor_health::PointCloudRegistrationLine;
    if !point_cloud.is_loaded() {
        return PointCloudRegistrationLine::NotConfigured;
    }
    match fusion {
        // Reachable only for the one tick, at most, between a pair finishing loading
        // and `register` resolving a backend for it -- both happen inside the same
        // `update::tick` call in a running deployment, so this is defensive rather
        // than a state an operator should ever actually see.
        FusionBackend::Uninitialized => PointCloudRegistrationLine::Pending,
        FusionBackend::Gpu { .. } => PointCloudRegistrationLine::Gpu,
        FusionBackend::Cpu { reason } => PointCloudRegistrationLine::CpuFallback { reason },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_data::pointcloud::PointBuffer;
    use gungnir_data::{DataError, DataStore};

    fn buffer(points: usize) -> PointBuffer {
        PointBuffer {
            positions: vec![[0.0, 0.0, 0.0]; points],
            ..PointBuffer::default()
        }
    }

    /// Nothing is drawn before both clouds are in, whatever `DataStore` happens to hold
    /// transiently while the pair is still loading.
    #[test]
    fn layers_are_empty_until_the_pair_is_loaded() {
        let mut data = DataStore::default();
        let status = PointCloudStatus::Loading {
            source: "a.las".into(),
            target: "b.las".into(),
        };
        assert!(layers(&status, &data).is_empty());

        data.point_clouds.push(buffer(3));
        assert!(
            layers(&status, &data).is_empty(),
            "one arrived cloud is still not a pair"
        );

        let status = PointCloudStatus::Failed {
            path: "a.las".into(),
            reason: "gone".into(),
        };
        assert!(layers(&status, &data).is_empty());
    }

    /// Once loaded, the viewport layer borrows both buffers' positions, source first.
    #[test]
    fn a_loaded_pair_becomes_two_layers_in_source_then_target_order() {
        let mut data = DataStore::default();
        data.point_clouds.push(buffer(5));
        data.point_clouds.push(buffer(7));
        let status = PointCloudStatus::Loaded {
            source: "a.las".into(),
            target: "b.copc.laz".into(),
            source_points: 5,
            target_points: 7,
        };
        let built = layers(&status, &data);
        assert_eq!(built.len(), 2);
        assert_eq!(built[0].positions.len(), 5);
        assert_eq!(built[1].positions.len(), 7);
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn a_copc_file_requests_a_bounded_read_and_a_plain_file_does_not() {
        let bounds = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let copc = PointCloudFileConfig {
            path: "clouds/a.copc.laz".into(),
            copc_bounds: Some(bounds),
        };
        assert!(matches!(
            request_for(&copc),
            LoadRequest::CopcBounded(_, b) if b == bounds
        ));
        let plain = PointCloudFileConfig {
            path: "clouds/a.las".into(),
            copc_bounds: None,
        };
        assert!(matches!(request_for(&plain), LoadRequest::PointCloud(_)));
    }

    /// `DataError`'s `Display` is what `apply_result` folds into the alert and the
    /// `Failed` reason; this just pins that a message survives the round trip.
    #[test]
    fn a_load_error_is_never_swallowed() {
        let err = DataError::Io("no such file".into());
        assert!(err.to_string().contains("no such file"));
    }

    // -- GAP-024: `register`/`registration_line`, the caller `FusionBackend` was
    //    missing (`crate::fusion`'s own doc comment) -------------------------------

    /// Four non-coplanar points, mirroring `crate::fusion`'s own `tiny_cloud` fixture:
    /// enough for Kabsch to recover a rotation, and identical to itself converges in
    /// one CPU `step` (`crate::fusion::tests::cpu_backend_builds_a_working_engine`
    /// checks the same fact one layer down).
    fn tiny_cloud() -> PointBuffer {
        PointBuffer {
            positions: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
            ],
            ..PointBuffer::default()
        }
    }

    /// A throwaway desktop, the same shape `gungnir-app/tests/pointcloud.rs::desktop`
    /// builds: a unique scratch `data_dir` so parallel tests never share a journal,
    /// and no point-cloud file configured, since these tests inject the pair directly
    /// rather than exercise the loader (GAP-098's own path, already covered there).
    fn desktop_state(name: &str) -> (AppState, std::path::PathBuf) {
        use gungnir_config::ConfigBaseline;
        let dir = std::env::temp_dir().join(format!(
            "gungnir-pointcloud-registration-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let config = ConfigBaseline {
            data_dir: dir.to_string_lossy().into_owned(),
            ..ConfigBaseline::default()
        };
        (AppState::with_config(config).expect("starts"), dir)
    }

    /// Neither cloud, then only the source: a real tick must not open a registration
    /// engine or claim a result for either, matching GAP-098's own all-or-nothing
    /// rule for what it keeps in `DataStore.point_clouds`.
    #[test]
    fn an_incomplete_pair_is_never_registered_by_a_real_tick() {
        let (mut state, dir) = desktop_state("incomplete");

        assert!(matches!(state.point_cloud, PointCloudStatus::NotConfigured));
        crate::update::tick(&mut state);
        assert_eq!(state.registration, RegistrationOutcome::NoPair);
        assert!(state.registration_engine.is_none());

        // The transient shape `apply_result` itself produces mid-load: the source has
        // arrived and the target has not, so `DataStore.point_clouds` holds exactly
        // one buffer while `PointCloudStatus` still says `Loading`.
        state.point_cloud = PointCloudStatus::Loading {
            source: "a.las".into(),
            target: "b.las".into(),
        };
        state.data.point_clouds.push(buffer(5));
        crate::update::tick(&mut state);
        assert_eq!(state.registration, RegistrationOutcome::NoPair);
        assert!(
            state.registration_engine.is_none(),
            "a lone cloud must not open a registration engine"
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    /// A loaded pair reaches `engine_for` through a real `update::tick`, on a backend
    /// forced to the CPU path so this test -- which runs under plain `cargo test` --
    /// never asks a real `wgpu::Instance` for an adapter (this workspace's hard rule
    /// against constructing a real GPU device anywhere plain `cargo test` reaches;
    /// `crate::fusion`'s own tests use the identical bypass).
    #[test]
    fn a_loaded_pair_registers_through_a_real_tick_on_a_forced_cpu_backend() {
        let (mut state, dir) = desktop_state("loaded-cpu");
        state.data.point_clouds.push(tiny_cloud());
        state.data.point_clouds.push(tiny_cloud());
        state.point_cloud = PointCloudStatus::Loaded {
            source: "source.las".into(),
            target: "target.las".into(),
            source_points: 4,
            target_points: 4,
        };
        state.fusion = FusionBackend::Cpu {
            reason: "test: forced CPU path".into(),
        };

        crate::update::tick(&mut state);

        match &state.registration {
            RegistrationOutcome::Registered { converged, .. } => {
                assert!(
                    *converged,
                    "an identical source and target converge at once"
                );
            }
            other => panic!("{other:?}"),
        }
        assert!(
            state.registration_engine.is_some(),
            "a successful build must be kept for the next tick to step"
        );
        assert!(
            !state.fusion.is_gpu(),
            "the forced CPU backend must not have been overwritten"
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    /// The PN-09 mapping itself, against synthetic `PointCloudStatus`/`FusionBackend`
    /// values -- no `AppState`, no loader, no device, for the same reason
    /// `crate::fusion`'s own tests construct `FusionBackend::Cpu` directly rather than
    /// resolve it.
    #[test]
    fn registration_line_reports_not_configured_pending_and_cpu_fallback() {
        use gungnir_ui::panels::sensor_health::PointCloudRegistrationLine;

        assert_eq!(
            registration_line(&PointCloudStatus::NotConfigured, &FusionBackend::new()),
            PointCloudRegistrationLine::NotConfigured
        );

        let loaded = PointCloudStatus::Loaded {
            source: "s".into(),
            target: "t".into(),
            source_points: 1,
            target_points: 1,
        };
        assert_eq!(
            registration_line(&loaded, &FusionBackend::new()),
            PointCloudRegistrationLine::Pending,
            "a pair loaded before any backend resolved must not be claimed as either one"
        );
        assert_eq!(
            registration_line(
                &loaded,
                &FusionBackend::Cpu {
                    reason: "no suitable GPU adapter".into(),
                },
            ),
            PointCloudRegistrationLine::CpuFallback {
                reason: "no suitable GPU adapter"
            }
        );
    }
}

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

use gungnir_config::PointCloudFileConfig;
use gungnir_data::{LoadRequest, LoadResult};

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
        Err(err) => {
            let reason = err.to_string();
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
}

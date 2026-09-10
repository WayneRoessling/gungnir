// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! fusion-async: out-of-sequence handling / multi-rate fusion + concurrency
//! correctness -- verification-capability-table.md `fusion-async` rows. The *only*
//! crate that uses the tokio runtime (agentic-coding-standards.md §2.2); the runtime
//! itself is owned by the host binary and handed in as a `Handle`. Channels, not
//! Arc<Mutex<..>>, are the default for cross-task state per the same section.

pub mod dense_group;
pub mod pipeline;
pub mod sync;

// Gate 4's model checks (`.github/workflows/loom.yml`, GAP-061). Compiled only by
// `cargo test --lib` under `--cfg loom`, which is the only configuration in which the
// `loom` dependency exists at all: it is a dev-dependency, so it is linked into test
// targets and nothing else. See `sync.rs` for why the cfg is `all(test, loom)`.
#[cfg(all(test, loom))]
mod loom_model;

pub use dense_group::{DenseGroupEstimate, DenseGroupFilter, DenseGroupSettings, GroupComponent};
pub use pipeline::{
    run_batch, BaselineError, BearingOutcome, BearingRefusal, FilterSelection, FusionPipeline,
    ImmBaselineFields, PipelineSettings, PipelineStats, PushError, RetainedBearing, TimedTrack,
    UnsupportedFilter, IMPLEMENTED_FILTERS,
};

// The channel types come from `crate::sync` rather than straight from
// `crossbeam-channel`: under `not(loom)` that module re-exports exactly these types, so
// the signatures below are unchanged for every ordinary build and for every caller, and
// under `--cfg loom` the same loop runs over a loom-instrumented channel (GAP-061).
use crate::sync::{Receiver, Sender, TryRecvError};

/// A raw detection as the tracking core consumes it. The canonical, provenance-bearing
/// form is `gungnir_model::DetectionView`; `gungnir-tracking-service` converts.
///
/// **This type is a position and only a position, and that is the enforcement of
/// DN-27 §5 rule 1.** A bearing arrives through [`BearingDetection`] and
/// [`FusionPipeline::offer_bearing`] instead, and there is no path from that type to
/// [`FusionPipeline::initiate`] -- not a flag to set, not a branch to take wrongly, but
/// no function that accepts one and creates a track. See
/// `docs/design/DN-27-bearing-only-detections.md` §5.
#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    pub sensor_id: u32,
    /// Source (sensor) time, seconds of mission time.
    pub timestamp_s: f64,
    /// Measurement in the local ENU frame, meters.
    pub measurement: nalgebra::Vector3<f64>,
}

/// A direction with no range: what an acoustic array, a passive radio-frequency
/// direction finder and a person with a compass report
/// (docs/design/DN-27-bearing-only-detections.md §4; GAP-001, GAP-004).
///
/// **Separate from [`Detection`] on purpose.** DN-27 §5 rule 1 forbids a bearing from
/// initiating a track: a single fixed sensor cannot localise from bearings at all,
/// the problem is unobservable, and a filter given a sequence of them converges to a
/// confident answer at the wrong range -- a failure that is silent and looks exactly
/// like success. The prohibition is a type boundary rather than a check because a check
/// can be forgotten and a missing function cannot be called.
///
/// `sensor_enu` is required and is not on `Detection`, because a bearing means nothing
/// without knowing where it was taken from. The caller resolves it from the deployment's
/// sensor list; `gungnir_model::DetectionView` carries a `SensorId` and not a position.
///
/// Azimuth is `atan2(east, north)`: a compass bearing, zero at north and increasing to
/// the east, the convention `gungnir_model::Measurement` and
/// `gungnir_filters::RangeAzimuthElevation` both state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BearingDetection {
    pub sensor_id: u32,
    /// Source (sensor) time, seconds of mission time.
    pub timestamp_s: f64,
    /// Where the bearing was measured from, local ENU metres.
    pub sensor_enu: [f64; 3],
    pub azimuth_rad: f64,
    /// `None` when the sensor reported none. **A missing elevation is not a zero one**:
    /// zero means the horizon (DN-27 §4).
    pub elevation_rad: Option<f64>,
    /// Angular variance of the azimuth, radians squared. Required: for a bearing the
    /// error *is* the information, and the cross-range error it implies grows with
    /// range (DN-27 §6).
    pub azimuth_variance_rad2: f64,
    /// Present exactly when `elevation_rad` is.
    pub elevation_variance_rad2: Option<f64>,
}

/// What a producer hands the ingest task: a position, or a direction with no range.
///
/// **Two variants rather than one, for the reason `BearingDetection` is a separate type
/// at all** (DN-27 §5 rule 1). A bearing may refine an existing track and may not start
/// one, and the pipeline enforces that through two different entry points --
/// [`FusionPipeline::push`] and [`FusionPipeline::offer_bearing`]. If the channel carried
/// one type, the loop below would have to decide which entry point to use by inspecting a
/// field, and a wrong branch there would put a bearing into the reorder buffer where it
/// would initiate a track at a range nobody measured.
///
/// The sensor's position is resolved by the *producer*, not here: `DetectionView` carries
/// a `SensorId` and the deployment's sensor list is the thing that knows where that sensor
/// is, which is `gungnir-tracking-service`'s business and not this task's.
#[derive(Debug, Clone, PartialEq)]
pub enum Submission {
    /// A measurement that determines a position. Enters the reorder buffer and may
    /// initiate a track.
    Position(Detection),
    /// A direction with no range. Offered to the tracker under DN-27 §5's three rules,
    /// and never able to initiate.
    Bearing(BearingDetection),
}

impl From<Detection> for Submission {
    fn from(d: Detection) -> Self {
        Submission::Position(d)
    }
}

impl From<BearingDetection> for Submission {
    fn from(b: BearingDetection) -> Self {
        Submission::Bearing(b)
    }
}

/// Everything one pass through [`ingest_with`]'s loop produced, bundled into one
/// channel message rather than sent as three (GAP-096).
///
/// **Signed by the owner 2026-09-09.** This crate is human-owned
/// (`docs/agentic-workflow.md`); this struct and the channel type change below were the
/// mechanical part of wiring [`FusionPipeline::retained_bearings`] and
/// [`FusionPipeline::stats`] out to a caller, written and gated 2026-09-08 and reviewed
/// before signing. The bundling claim below is not only gated but model-checked:
/// `loom_model::snapshot_fields_come_from_one_epoch` holds it over every interleaving of
/// the outbound channel, and `loom_model::unbundled_publication_is_caught` proves the
/// two-channel shape this replaced *does* skew under preemption. What the review found
/// was elsewhere -- the retained bearings' lifetime was not honoured on screen; see
/// [`FusionPipeline::expire_bearings`] -- and it was closed the same day.
///
/// Bundled on purpose rather than sent over a second channel: `tracks`, the retained
/// bearings and the stats are all read from the same pipeline at the same instant inside
/// this loop, with no `.await` between them. A poller draining two independent channels
/// could see a track snapshot from one epoch next to a bearing snapshot from another,
/// which is the same kind of skew [`TimedTrack`] exists to keep out of a single track.
///
/// **What the channel does not bound, stated rather than implied.** The outbound channel
/// is unbounded, one of these is sent per submission the loop processes, and each is a
/// full copy of the picture -- every live track, every retained bearing. A consumer that
/// keeps polling sees at most one poll's worth of them queued and applies only the
/// newest (`LiveTrackingService::poll`'s drain-to-latest); a consumer that stops polling
/// accumulates them until it polls again, and GAP-096 made each one larger. That is the
/// same shape the channel had when it carried `Vec<TimedTrack>` alone, and coalescing at
/// the producer would be a design change (§2.2's channels-by-default rule), not this
/// entry's; it is named here so the cost is known rather than discovered.
#[derive(Debug, Clone, Default)]
pub struct PipelineSnapshot {
    pub tracks: Vec<TimedTrack>,
    /// [`FusionPipeline::retained_bearings`] at the same instant as `tracks`
    /// (DN-27 §5 rule 3).
    pub retained_bearings: Vec<RetainedBearing>,
    /// [`FusionPipeline::stats`] at the same instant as `tracks`.
    pub stats: PipelineStats,
    /// What the dense-group mode reported for the most recent epoch (GAP-015), at the
    /// same instant as `tracks`. `None` is the ordinary answer and means the scene was
    /// resolvable.
    ///
    /// **This is not a second track list and must never be drawn as one.** It carries no
    /// identifier of any kind, because the PHD or CPHD intensity behind it carries no
    /// identity across scans; see [`crate::dense_group`] for the whole of that reasoning
    /// and for what a labelled filter would have to add before any of it could be
    /// presented as tracks. **Nothing reads this yet**: `gungnir-tracking-service` does
    /// not project it, which is the same state `retained_bearings` was left in above.
    pub dense_group: Option<DenseGroupEstimate>,
    /// [`FusionPipeline::dense_group_refusals`] at the same instant as `tracks`: epochs
    /// the dense-group filter refused, so a caller can tell a resolvable scene from a
    /// filter that would not run.
    ///
    /// Not folded into [`PipelineStats`], whose fields are GAP-096's wire contract with
    /// `gungnir_model::PipelineStatsView`; widening that contract is not this change's
    /// business.
    pub dense_group_refusals: u64,
}

/// Read every part of `pipeline`'s current output into one [`PipelineSnapshot`].
fn snapshot_output(pipeline: &FusionPipeline) -> PipelineSnapshot {
    PipelineSnapshot {
        tracks: pipeline.timed_snapshot(),
        retained_bearings: pipeline.retained_bearings().to_vec(),
        stats: pipeline.stats(),
        dense_group: pipeline.dense_group_estimate().cloned(),
        dense_group_refusals: pipeline.dense_group_refusals(),
    }
}

/// Whether this build has an out-of-sequence, multi-rate pipeline behind
/// [`ingest`]. `gungnir-tracking-service` reports `is_healthy() == false` while this
/// is false, so no dashboard can claim a working tracker before one exists.
///
/// **True since 2026-09-06** (GAP-011): [`pipeline::FusionPipeline`] composes the
/// signed linear Kalman filter, the Jonker-Volgenant associator, the chi-square gate
/// and the `gungnir-track` lifecycle over a reorder buffer, and
/// `tests/oos_convergence.rs` gates the `fusion-async` row -- the async path against
/// the offline batch over the same multi-sensor timeline.
///
/// **What it does not claim.** The pipeline runs a constant-velocity Kalman filter per
/// track, or, where a baseline names `imm-cv-ct` (DN-28), the CV/CT IMM over it -- one
/// selection per pipeline instance (`PipelineSettings::filter_selection`), not per
/// track. The remaining nonlinear estimators and track-to-track fusion are separate rows
/// and separate gaps (GAP-011's remainder, GAP-013, DN-28 §6); this constant says a
/// pipeline exists and produces tracks, not that every estimator in the capability table
/// is in it.
///
/// **Random-finite-set filtering is in the pipeline since GAP-015, and this constant is
/// still not a claim about it.** The dense-group mode ([`crate::dense_group`]) runs a
/// PHD or CPHD for a scan past the association limit and reports a count and a shape.
/// It produces **no tracks and no identities**, so nothing it reports is part of what
/// this flag says the pipeline produces; the labelled filters that would carry identity
/// are GAP-015's remaining row and are not built.
pub const PIPELINE_IMPLEMENTED: bool = true;

/// The ingest task: buffers out-of-order/late detections from multiple sensors,
/// reconciles them onto a common fused timeline, and emits a [`PipelineSnapshot`] on
/// `out` after every epoch it processes.
///
/// The default settings are used; [`ingest_with`] takes a deployment's.
///
/// Cancellation-safety note (required on every async fn per §2.2): the pipeline lives
/// on the stack of this task and is mutated only between awaits. A detection is taken
/// from the channel and pushed into the reorder buffer with no await in between, so a
/// dropped future loses at most one in-flight detection and never leaves the buffer
/// half-updated.
///
/// The inbound channel is a `crossbeam` channel because it is the boundary with the
/// synchronous render/UI thread (§2.2); it is polled with `try_recv` plus a yield
/// rather than a blocking `recv`, which would stall the executor thread.
pub async fn ingest(rx: Receiver<Submission>, out: Sender<PipelineSnapshot>) {
    ingest_with(rx, out, PipelineSettings::default()).await;
}

/// [`ingest`] under a deployment's settings.
///
/// **The stream's end is a flush, not a truncation.** When the inbound channel closes,
/// whatever is still inside the reorder horizon is processed and a final snapshot is
/// emitted before the task stops. Dropping it would lose the last horizon of every
/// session, and a replay would then end short of the recording it replayed.
pub async fn ingest_with(
    rx: Receiver<Submission>,
    out: Sender<PipelineSnapshot>,
    settings: PipelineSettings,
) {
    let mut pipeline = FusionPipeline::new(settings);
    tracing::info!(
        ?pipeline,
        "fusion-async pipeline started: out-of-sequence buffering with global association"
    );
    loop {
        match rx.try_recv() {
            Ok(Submission::Position(det)) => {
                // A position carries the same mission clock a bearing does, so the
                // retained set ages on it too (2026-09-09): a busy radar beside a quiet
                // acoustic feed is the ordinary case, and before this the last unmatched
                // bearing stayed in every snapshot until the next bearing arrived,
                // however long past its own `until_s` that was.
                pipeline.expire_bearings(det.timestamp_s);
                if let Err(err) = pipeline.push(det) {
                    tracing::warn!(%err, "detection refused by the reorder buffer");
                }
                if pipeline.run_ready() > 0 && out.send(snapshot_output(&pipeline)).is_err() {
                    tracing::warn!("track consumer is gone; stopping the pipeline");
                    return;
                }
            }
            Ok(Submission::Bearing(bearing)) => {
                // DN-27 §5: a bearing may update a track, may not initiate one, and is
                // retained and shown when it updates nothing. All three are the
                // pipeline's rules; this loop only reports which happened, because an
                // operator asking why a direction did not become a track needs the
                // answer to have been recorded somewhere.
                match pipeline.offer_bearing(&bearing) {
                    BearingOutcome::Updated(track) => {
                        tracing::debug!(
                            sensor = bearing.sensor_id,
                            ?track,
                            "a bearing refined a track"
                        );
                    }
                    BearingOutcome::Retained { until_s } => {
                        tracing::debug!(
                            sensor = bearing.sensor_id,
                            until_s,
                            "a bearing matched nothing and is retained"
                        );
                    }
                    BearingOutcome::Refused(why) => {
                        tracing::warn!(sensor = bearing.sensor_id, ?why, "a bearing was refused");
                    }
                }
                // A retained bearing has a stated lifetime and something has to end it.
                // Doing it here rather than on a timer keeps it on the same clock the
                // bearings themselves carry; the position arm above does the same, and a
                // pipeline that receives nothing at all cannot age its set, which is why
                // the consumer ages the *view* by its own clock as well.
                pipeline.expire_bearings(bearing.timestamp_s);
                if out.send(snapshot_output(&pipeline)).is_err() {
                    tracing::warn!("track consumer is gone; stopping the pipeline");
                    return;
                }
            }
            Err(TryRecvError::Empty) => crate::sync::idle_backoff().await,
            Err(TryRecvError::Disconnected) => break,
        }
    }
    if pipeline.flush() > 0 {
        let _ = out.send(snapshot_output(&pipeline));
    }
    tracing::info!(
        stats = ?pipeline.stats(),
        "fusion-async ingest task stopped: inbound channel closed and the buffer flushed"
    );
}

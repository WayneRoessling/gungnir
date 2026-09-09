// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! `OnnxModel`: the [`Model`] trait's real backend, now that GAP-077's runtime sign-off
//! (D-40) is built rather than deferred (`docs/ml/architecture.md` §3). Runs an ONNX
//! graph through `ort::session::Session` on the CPU execution provider -- the two rows
//! of architecture.md §3's table that need no GPU; the GPU-ordered-provider row is not
//! built here, since nothing in this change needs one to be true.
//!
//! `ort` is pinned with `download-binaries` refused and `load-dynamic` enabled instead
//! (`agentic-coding-standards.md` §2.9, "ONNX inference runtime"): the ONNX Runtime
//! shared library is `dlopen`ed from `ORT_DYLIB_PATH` the first time a session is
//! actually built, rather than downloaded from a CDN or linked in at compile time. That
//! keeps building this crate free of any C++ toolchain or network fetch. It does not,
//! however, keep [`OnnxModel::load`] from **panicking** on any machine that has not
//! placed a compatible `libonnxruntime`/`onnxruntime.dll` there -- checked 2026-09-08,
//! reproduced rather than assumed: `ort` 2.0.0-rc.13's global `ort::api()` accessor is
//! documented to panic ("May panic if ... Loading the ONNX Runtime dynamic library
//! fails") rather than return a `Result`, and on this build's own development machine it
//! did exactly that against a wrong-version system `onnxruntime.dll` it found on the
//! default search path with `ORT_DYLIB_PATH` unset. Worse, that first panic poisons an
//! internal global mutex `ort` locks again from an atexit handler, which then panics a
//! second time in a context that cannot unwind and hard-aborts the whole process --
//! observed directly, not theorised. [`OnnxModel::load`] wraps its own call into `ort`
//! in [`std::panic::catch_unwind`] so *this* function still returns a clean
//! [`MlError::RuntimeUnavailable`] rather than propagating the panic to its own caller,
//! but that cannot undo the poisoning or guarantee the process survives to exit cleanly
//! afterwards -- which is exactly why this module sits behind the `onnx-runtime`
//! feature, off by default (`lib.rs`'s own module documentation, and
//! `agentic-coding-standards.md` §2.9): nothing in the default `cargo test --workspace`
//! run may call into this module until a real, version-matched ONNX Runtime is known to
//! be reachable.

use std::sync::Mutex;

use ort::session::Session;
use ort::value::Tensor;
use sha2::{Digest, Sha256};

use crate::{ClassScores, FeatureBatch, InputSignature, MlError, Model, Outputs};

/// A [`Model`] backed by a real ONNX graph.
///
/// [`Session::run`] needs `&mut self`; [`Model::infer`] is `&self` because every other
/// implementor (`FakeModel`) is stateless and the trait is `Send + Sync` so a
/// [`crate::ModelSet`] can share one across callers without reaching into the runtime's
/// own concurrency story. A [`Mutex`] reconciles the two without changing the trait
/// every future backend has to implement against.
#[derive(Debug)]
pub struct OnnxModel {
    name: String,
    version: String,
    signature: InputSignature,
    /// The class each output column names, in column order. Stated by the manifest
    /// rather than read from the graph: an ONNX graph's own metadata does not reliably
    /// carry semantic class labels, and guessing them from the file would be exactly the
    /// invented-field failure `docs/ml/mlops.md`'s own closing action for GAP-078 warns
    /// against.
    classes: Vec<String>,
    input_name: String,
    output_name: String,
    session: Mutex<Session>,
}

impl OnnxModel {
    /// Verifies `model_bytes` against the manifest's stated SHA-256 before it may reach
    /// [`Self::load`] (`docs/ml/architecture.md` §5: "Loading verifies the hash before
    /// the file reaches the runtime").
    ///
    /// # Errors
    ///
    /// [`MlError::ArtefactHashMismatch`] when the digest does not match.
    pub fn verify_hash(
        name: &str,
        expected_sha256: &str,
        model_bytes: &[u8],
    ) -> Result<(), MlError> {
        let found = format!("{:x}", Sha256::digest(model_bytes));
        if !found.eq_ignore_ascii_case(expected_sha256) {
            return Err(MlError::ArtefactHashMismatch {
                model: name.to_owned(),
            });
        }
        Ok(())
    }

    /// Loads an already hash-verified model from memory. The caller
    /// ([`crate::ModelSet::load`]) is expected to have called [`Self::verify_hash`]
    /// first; this constructor does not repeat that check, so a caller that skips it
    /// hands the runtime bytes nothing has vouched for.
    ///
    /// # Errors
    ///
    /// [`MlError::RuntimeUnavailable`] when the ONNX Runtime shared library cannot be
    /// `dlopen`ed (`ORT_DYLIB_PATH` unset or wrong), the graph fails to parse, **or `ort`
    /// panics while trying either** -- caught here (module documentation above) and
    /// turned into this same variant, since a caller cannot act differently on "the
    /// library is not there" versus "it panicked trying to find out". [`MlError::ManifestInvalid`]
    /// when the graph does not declare exactly one input and one output tensor, the one
    /// shape this crate's feature-batch and class-score types assume.
    pub fn load(
        name: &str,
        version: &str,
        signature: InputSignature,
        classes: Vec<String>,
        model_bytes: &[u8],
    ) -> Result<Self, MlError> {
        let (session, input_name, output_name) =
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                Self::build_session(name, model_bytes)
            })) {
                Ok(result) => result?,
                Err(payload) => {
                    return Err(MlError::RuntimeUnavailable(format!(
                        "ONNX Runtime panicked while loading model {name}: {}",
                        panic_message(&payload)
                    )));
                }
            };

        Ok(Self {
            name: name.to_owned(),
            version: version.to_owned(),
            signature,
            classes,
            input_name,
            output_name,
            session: Mutex::new(session),
        })
    }

    /// The part of [`Self::load`] that actually touches `ort`, split out so
    /// [`std::panic::catch_unwind`] can wrap exactly this and nothing else -- see this
    /// module's own documentation for the confirmed panic this guards against.
    fn build_session(name: &str, model_bytes: &[u8]) -> Result<(Session, String, String), MlError> {
        let mut builder = Session::builder().map_err(|e| {
            MlError::RuntimeUnavailable(format!("ONNX Runtime session builder: {e}"))
        })?;
        let session = builder
            .commit_from_memory(model_bytes)
            .map_err(|e| MlError::RuntimeUnavailable(format!("ONNX Runtime session: {e}")))?;

        let inputs = session.inputs();
        if inputs.len() != 1 {
            return Err(MlError::ManifestInvalid(format!(
                "model {name}: expected exactly one input tensor, the graph declares {}",
                inputs.len()
            )));
        }
        let input_name = inputs[0].name().to_owned();

        let outputs = session.outputs();
        if outputs.len() != 1 {
            return Err(MlError::ManifestInvalid(format!(
                "model {name}: expected exactly one output tensor, the graph declares {}",
                outputs.len()
            )));
        }
        let output_name = outputs[0].name().to_owned();

        Ok((session, input_name, output_name))
    }
}

/// `ort`'s tensor shapes are `i64`; a track, feature, or class count is a `usize` from a
/// `Vec::len()`. The cast cannot realistically overflow, but "cannot realistically" is
/// not this codebase's bar for a signedness-changing `as` cast, so it is checked and
/// named rather than silently wrapped.
fn checked_i64(n: usize, model: &str, what: &str) -> Result<i64, MlError> {
    i64::try_from(n)
        .map_err(|_| MlError::Inference(format!("model {model}: {what} {n} overflows i64")))
}

/// Best-effort text out of a [`std::panic::catch_unwind`] payload: `panic!("{}", ...)`
/// and `panic!("literal")` cover almost everything seen in practice, `ort`'s own
/// `panic!("Failed to load ONNX Runtime dylib: {e}")` included.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}

impl Model for OnnxModel {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn input_signature(&self) -> &InputSignature {
        &self.signature
    }

    fn infer(&self, batch: &FeatureBatch) -> Result<Outputs, MlError> {
        let offered = batch.signature();
        if offered != self.signature {
            return Err(MlError::SignatureMismatch {
                model: self.name.clone(),
                expected: self.signature.describe(),
                found: offered.describe(),
            });
        }
        if batch.tracks.is_empty() {
            return Ok(Outputs::default());
        }

        let num_features = self.signature.features.len();
        let mut flat = Vec::with_capacity(batch.tracks.len() * num_features);
        for row in &batch.rows {
            flat.extend_from_slice(row);
        }
        let num_tracks = checked_i64(batch.tracks.len(), &self.name, "track count")?;
        let shape = vec![
            num_tracks,
            checked_i64(num_features, &self.name, "feature count")?,
        ];
        let tensor = Tensor::from_array((shape, flat)).map_err(|e| {
            MlError::Inference(format!("model {}: building input tensor: {e}", self.name))
        })?;

        let mut session = self.session.lock().map_err(|_| {
            MlError::Inference(format!(
                "model {}: session lock poisoned by an earlier panic",
                self.name
            ))
        })?;

        let outputs = session
            .run(ort::inputs![self.input_name.as_str() => tensor])
            .map_err(|e| {
                MlError::Inference(format!("model {}: ONNX Runtime run: {e}", self.name))
            })?;

        let (out_shape, out_data) = outputs[self.output_name.as_str()]
            .try_extract_tensor::<f32>()
            .map_err(|e| {
                MlError::Inference(format!("model {}: reading output tensor: {e}", self.name))
            })?;

        let num_classes = self.classes.len();
        let expected_shape = [
            num_tracks,
            checked_i64(num_classes, &self.name, "class count")?,
        ];
        if out_shape.as_ref() != expected_shape.as_slice() {
            return Err(MlError::Inference(format!(
                "model {}: output shape {out_shape:?} does not match {} tracks x {} classes",
                self.name,
                batch.tracks.len(),
                num_classes
            )));
        }

        let per_track = batch
            .tracks
            .iter()
            .enumerate()
            .map(|(i, &track)| {
                let row = &out_data[i * num_classes..(i + 1) * num_classes];
                ClassScores {
                    track,
                    scores: self
                        .classes
                        .iter()
                        .cloned()
                        .zip(row.iter().copied())
                        .collect(),
                }
            })
            .collect();

        Ok(Outputs { per_track })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_hash_catches_a_tampered_artefact() {
        let bytes = b"not really an onnx graph";
        let real = format!("{:x}", Sha256::digest(bytes));
        assert!(OnnxModel::verify_hash("ml-01", &real, bytes).is_ok());
        assert!(matches!(
            OnnxModel::verify_hash("ml-01", "0000", bytes),
            Err(MlError::ArtefactHashMismatch { model }) if model == "ml-01"
        ));
    }

    // **Deliberately no `OnnxModel::load` round-trip test here, even a failure-path one,
    // and not for lack of trying.** Confirmed 2026-09-08 on two independent runs: calling
    // `Session::builder()`/`commit_from_memory` while no compatible ONNX Runtime is
    // reachable does not just return `Err` for that call, and `catch_unwind` around it
    // does not fully contain the damage either. `ort` panics inside its own global
    // `api()` accessor (module documentation above), which poisons an internal mutex;
    // `catch_unwind` stops that panic from escaping `OnnxModel::load` itself, but the
    // poisoning is process-wide and permanent, and `ort`'s own atexit handler later locks
    // the same mutex and panics a second time in a context that cannot unwind --
    // `thread caused non-unwinding panic. aborting.`, `STATUS_STACK_BUFFER_OVERRUN`,
    // reproduced with and without the `catch_unwind` wrapper in place. A test that
    // exercises this call at all -- pass or fail -- takes the whole test binary down with
    // it at exit. This is the concrete reason `onnx-runtime` is a feature off by default
    // (`lib.rs`, `agentic-coding-standards.md` §2.9) rather than merely undocumented
    // caution: it is not safe to add this test even gated behind that feature, since
    // enabling it for a test run would reproduce the same abort. `verify_hash` above is
    // the whole of this module's runnable coverage until a real, version-matched ONNX
    // Runtime is available to test against.
}

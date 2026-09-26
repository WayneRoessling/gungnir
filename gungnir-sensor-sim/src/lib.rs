// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! **Simulation.** The observation half of the test-track generator: given a target
//! where a recording says it was, would this sensor, standing here, have detected it,
//! and where would it have reported it? (docs/design/DN-32-re-observation-for-a-laydown.md.)
//!
//! # Re-observes recorded truth; never generates it
//!
//! A generator of synthetic observations inside a command-and-control binary is a way
//! for invented data to be mistaken for sensor data, which is why `gungnir-scenario`
//! may never be a dependency of a production crate. This crate is the narrow half of
//! that generator a laydown rehearsal needs (DN-32 §2): the observation model and
//! nothing that makes a world -- no entities, no motion, no routes, no spawning. A
//! binary that links it can re-observe a recording and **cannot invent a target**.
//!
//! # Containment (DN-32 §6)
//!
//! 1. Every [`Observation`] carries a [`SimulationMark`], set here and nowhere else.
//! 2. `gungnir-app` puts the mark on the detection's provenance, and a live
//!    `gungnir_ingest::IngestGateway` refuses and counts any detection carrying it.
//! 3. A rehearsal runs on a throwaway desktop state with its own journal.
//! 4. `gungnir-app/tests/dependency_graph.rs`'s `sensor_sim_misuse`: this crate is a
//!    dependency of `gungnir-scenario`, `gungnir-app` and the verifier layer, and of
//!    nothing else -- not the node, ingest, the tracking service, the API or the link.
//! 5. `gungnir-app/tests/architecture_compliance.rs`: within `gungnir-app`, only
//!    `laydown_rehearsal.rs` may name this crate.
//!
//! # What is here
//!
//! - [`observe`] and [`false_alarms`]: the model, moved out of `gungnir-scenario`'s
//!   `tracks.rs` statement for statement, which the generator now calls
//!   (`gungnir-scenario/tests/reference_parity.rs` still reproduces every committed
//!   sample set byte for byte).
//! - [`pynum`] and [`python_random`]: the Python-parity arithmetic and random stream it
//!   needs, moved with it (DN-32 §4).
//! - [`reobserve`]: a recording's truth re-observed by a set of placed sensors, scan by
//!   scan, on the recording's own tick and never between its samples.

pub mod model;
pub mod observe;
pub mod pynum;
pub mod python_random;
pub mod recording;
pub mod vec3;

pub use model::{SensorCatalogue, SensorParams};
pub use observe::{
    false_alarms, observe, Observation, ScanConditions, ScanContext, Signature, SimulationMark,
    TargetState, MAX_LATENCY_S,
};
pub use python_random::PythonRandom;
pub use recording::{
    reobserve, EntitiesFile, EntityRecord, EnvironmentEvent, EnvironmentFile, PlacedSensor,
    ReObservation, ReObservationError, Recording, Sector, SensorEvents, SensorTally, TruthRecord,
};

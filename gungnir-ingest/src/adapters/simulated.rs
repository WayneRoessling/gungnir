//! An in-memory queue of detections, fed by tests or by a `gungnir-scenario`
//! generator in a test harness, so the gateway's validation and quarantine path can
//! run end to end without hardware.

use crate::{DetectionView, IngestError, ProtocolAdapter};
use gungnir_model::MissionTime;
use std::collections::VecDeque;

#[derive(Debug, Default)]
pub struct SimulatedAdapter {
    name: String,
    queue: VecDeque<DetectionView>,
}

impl SimulatedAdapter {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            queue: VecDeque::new(),
        }
    }

    pub fn push(&mut self, detection: DetectionView) {
        self.queue.push_back(detection);
    }

    pub fn pending(&self) -> usize {
        self.queue.len()
    }
}

impl ProtocolAdapter for SimulatedAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn poll(&mut self, _now: MissionTime) -> Result<Vec<DetectionView>, IngestError> {
        Ok(self.queue.drain(..).collect())
    }
}

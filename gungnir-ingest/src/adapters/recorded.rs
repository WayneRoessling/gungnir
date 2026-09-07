// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Replays a previously recorded feed (one JSON `DetectionView` per line) as if it
//! were live, releasing each detection when mission time reaches its source time.
//! Used for testing adapters against captured traffic shapes and for the
//! resilience store-and-forward tests.

use crate::gateway::decode_json_line;
use crate::{DetectionView, IngestError, ProtocolAdapter};
use gungnir_model::MissionTime;
use std::collections::VecDeque;
use std::path::Path;

#[derive(Debug)]
pub struct RecordedFeedAdapter {
    name: String,
    queue: VecDeque<DetectionView>,
}

impl RecordedFeedAdapter {
    /// Load the whole file up front; lines that fail to parse are counted as I/O
    /// errors and the file is rejected, since a torn recording is not a valid feed.
    pub fn open(path: &Path) -> Result<Self, IngestError> {
        let text = std::fs::read_to_string(path).map_err(|e| IngestError::Io(e.to_string()))?;
        let mut queue = VecDeque::new();
        for (n, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let d = decode_json_line(line.as_bytes())
                .map_err(|e| IngestError::Io(format!("{}:{}: {e}", path.display(), n + 1)))?;
            queue.push_back(d);
        }
        // Release order follows source time regardless of file order.
        let mut sorted: Vec<DetectionView> = queue.into_iter().collect();
        sorted.sort_by(|a, b| {
            a.source_time
                .partial_cmp(&b.source_time)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(Self {
            name: format!("recorded:{}", path.display()),
            queue: sorted.into(),
        })
    }

    pub fn remaining(&self) -> usize {
        self.queue.len()
    }
}

impl ProtocolAdapter for RecordedFeedAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn poll(&mut self, now: MissionTime) -> Result<Vec<DetectionView>, IngestError> {
        let mut due = Vec::new();
        while let Some(front) = self.queue.front() {
            if front.source_time <= now {
                if let Some(d) = self.queue.pop_front() {
                    due.push(d);
                }
            } else {
                break;
            }
        }
        Ok(due)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gungnir_model::{Provenance, SensorId};

    fn line(t: f64) -> String {
        serde_json::to_string(&DetectionView {
            sensor: SensorId(1),
            source_time: MissionTime(t),
            receipt_time: MissionTime(t + 0.1),
            measurement: gungnir_model::Measurement::Position {
                enu: nalgebra::Vector3::new(t, 0.0, 0.0),
                variance_m2: [400.0, 400.0, 900.0],
            },
            provenance: Provenance::default(),
        })
        .expect("encode")
    }

    #[test]
    fn releases_detections_in_source_time_order_as_time_advances() {
        let path =
            std::env::temp_dir().join(format!("gungnir-recorded-{}.jsonl", std::process::id()));
        std::fs::write(
            &path,
            format!("{}\n{}\n\n{}\n", line(3.0), line(1.0), line(2.0)),
        )
        .expect("write");
        let mut adapter = RecordedFeedAdapter::open(&path).expect("open");
        assert_eq!(adapter.remaining(), 3);
        let first = adapter.poll(MissionTime(1.5)).expect("poll");
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].source_time, MissionTime(1.0));
        let rest = adapter.poll(MissionTime(10.0)).expect("poll");
        assert_eq!(
            rest.iter().map(|d| d.source_time.0).collect::<Vec<_>>(),
            vec![2.0, 3.0]
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn torn_recording_is_rejected() {
        let path =
            std::env::temp_dir().join(format!("gungnir-recorded-bad-{}.jsonl", std::process::id()));
        std::fs::write(&path, "{\"sensor\":").expect("write");
        assert!(matches!(
            RecordedFeedAdapter::open(&path),
            Err(IngestError::Io(_))
        ));
        let _ = std::fs::remove_file(&path);
    }
}

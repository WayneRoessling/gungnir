// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! Fuzz target for CI gate #5: build an association cost matrix from arbitrary
//! bytes (dimensions from the first two bytes, entries from the rest as f64 bit
//! patterns, including NaN and infinities) and assert construction never panics.
//!
//! As of 2026-09-05 `solve_assignment` is implemented, so it is called here too: a
//! degenerate or non-finite matrix must never crash the associator. The solver is
//! fallible by design, and this target is what holds it to that -- an `Err` is a pass,
//! a panic is a failure.
//!
//! The postconditions below are checked on every `Ok`, because "did not panic" is a
//! weak thing to assert about a solver. A cost matrix reaches this code from sensor
//! data, and an assignment that used a column twice would silently associate one
//! detection with two tracks.
#![no_main]
use gungnir_association::solve_assignment;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }
    let rows = usize::from(data[0] % 32);
    let cols = usize::from(data[1] % 32);
    let needed = rows * cols;
    let entries: Vec<f64> = data[2..]
        .chunks_exact(8)
        .take(needed)
        .map(|c| f64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
        .collect();
    if entries.len() != needed {
        return;
    }
    let cost = nalgebra::DMatrix::<f64>::from_iterator(rows, cols, entries);
    let _ = cost.iter().filter(|v| v.is_finite()).count();

    // An error is the expected outcome for a non-finite matrix and is not a finding.
    if let Ok(assignment) = solve_assignment(&cost) {
        assert_eq!(
            assignment.row_to_col.len(),
            rows,
            "assignment has one entry per row"
        );
        assert_eq!(
            assignment.assigned_count(),
            rows.min(cols),
            "a solvable matrix assigns as many rows as there are columns to go round"
        );
        let mut seen = vec![false; cols];
        for col in assignment.row_to_col.iter().flatten() {
            assert!(*col < cols, "assigned column is in range");
            assert!(!seen[*col], "column assigned to two rows");
            seen[*col] = true;
        }
        assert!(
            assignment.total_cost.is_finite(),
            "a finite cost matrix yields a finite total"
        );
    }
});

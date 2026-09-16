// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

//! The LAS/LAZ and COPC loaders against their verification row
//! (`verification-capability-table.md` §2, `gungnir-data`): a fixture loads to exact counts
//! and bounds, and a corrupt file is a `DataError`, never a panic. Figures in
//! `testdata/pointcloud/SOURCE.md`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gungnir_data::pointcloud::{self, PointBuffer};
use gungnir_data::{DataError, LoadRequest, LoadResult};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../testdata/pointcloud")
        .join(name)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gungnir-las-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir.join(name)
}

/// `five-points.las` as `testdata/pointcloud/SOURCE.md` tabulates it, record by record:
/// position in the file's own frame, intensity, classification.
const FIVE_POINTS: [([f64; 3], f32, u8); 5] = [
    ([500_010.00, 6_000_020.00, 12.50], 100.0, 2),
    ([500_011.25, 6_000_020.00, 12.75], 120.0, 2),
    ([500_010.00, 6_000_021.50, 13.00], 140.0, 5),
    ([500_012.00, 6_000_022.00, 18.25], 160.0, 6),
    ([500_013.50, 6_000_019.00, 12.00], 180.0, 2),
];

/// The bounded query box `a_bounded_query_recovers_points_from_a_real_copc_hierarchy`
/// chose from the COPC fixture's own bounds (`SOURCE.md`): x, y, z minimum, then maximum.
const COPC_QUERY_BOX: [f32; 6] = [637_200.0, 851_100.0, 400.0, 637_300.0, 851_200.0, 620.0];

/// What `SOURCE.md` promises of the five-point cloud, whichever encoding it was read from.
///
/// Exact, not within a margin: every coordinate in the table is a multiple of 0.25 m, which
/// an `f64` holds exactly at these magnitudes, and so does an `f32` offset of a few metres
/// from the origin, so any difference at all is a wrong read.
#[allow(clippy::float_cmp)]
fn assert_is_the_five_point_cloud(cloud: &PointBuffer) {
    assert_eq!(cloud.positions.len(), 5);
    assert_eq!(
        cloud.origin,
        [500_010.0, 6_000_019.0, 12.0],
        "the header's minimum"
    );
    assert_eq!(
        cloud.bounds(),
        Some([
            [500_010.0, 6_000_019.0, 12.0],
            [500_013.5, 6_000_022.0, 18.25]
        ]),
        "SOURCE.md's bounds"
    );
    let intensity = cloud.intensity.as_deref().expect("intensity");
    let classification = cloud.classification.as_deref().expect("classification");
    for (i, (position, want_intensity, want_class)) in FIVE_POINTS.iter().enumerate() {
        // Positions are relative to the origin, so a UTM coordinate survives the f32.
        let got: [f64; 3] =
            std::array::from_fn(|axis| f64::from(cloud.positions[i][axis]) + cloud.origin[axis]);
        assert_eq!(got, *position, "record {}", i + 1);
        assert_eq!(intensity[i], *want_intensity, "record {}", i + 1);
        assert_eq!(classification[i], *want_class, "record {}", i + 1);
    }
}

/// `five-points.las`'s own records, LAZ-compressed by the `las` crate into a scratch file
/// named `name` (which must end `.laz`: `Writer::from_path` compresses by extension). The
/// fixture's header travels with them, so scale, offset, version and point format are the
/// fixture's own and only the encoding differs.
fn five_points_as_laz(name: &str) -> PathBuf {
    let mut reader = las::Reader::from_path(fixture("five-points.las")).expect("fixture");
    let header = reader.header().clone();
    let records = reader.read_all().expect("fixture records");
    let path = scratch(name);
    let mut writer = las::Writer::from_path(&path, header).expect("a LAZ writer");
    for point in records.points() {
        writer
            .write_point(point.expect("a fixture record"))
            .expect("written");
    }
    writer.close().expect("closed");
    path
}

#[test]
fn the_fixture_loads_to_its_recorded_figures() {
    let cloud = pointcloud::load_las(&fixture("five-points.las")).expect("loads");
    assert_is_the_five_point_cloud(&cloud);
}

/// The LAS/LAZ half of the `gungnir-data` Loader correctness per format row
/// (`docs/verification-capability-table.md` §2): a LAZ file goes through `load_las` to the
/// same exact count, bounds and records as the LAS file it was compressed from.
///
/// No LAZ file is committed, so one is written at test time from `five-points.las`; the
/// expected figures are still `SOURCE.md`'s, not whatever the LAS read returned.
#[test]
fn a_laz_file_loads_to_the_same_figures_as_the_las_file_it_compresses() {
    let path = five_points_as_laz("five-points.laz");
    let bytes = std::fs::read(&path).expect("written");
    // LASzip marks a compressed file by setting the top bit of the point data format byte
    // (offset 104 of the public header block), so this proves the file the loader reads
    // is LAZ rather than a LAS file that happens to carry a `.laz` name.
    assert_eq!(
        bytes[104], 0x80,
        "point format 0 with the LASzip compression bit"
    );
    let cloud = pointcloud::load_las(&path).expect("a LAZ file loads");
    assert_is_the_five_point_cloud(&cloud);
}

#[test]
fn corrupt_files_are_errors_never_panics() {
    let good = std::fs::read(fixture("five-points.las")).expect("fixture");
    let truncated_header = scratch("truncated-header.las");
    std::fs::write(&truncated_header, &good[..100]).expect("write");
    let truncated_points = scratch("truncated-points.las");
    std::fs::write(&truncated_points, &good[..good.len() - 7]).expect("write");
    let garbage = scratch("garbage.las");
    std::fs::write(
        &garbage,
        (0..400u32)
            .map(|i| u8::try_from((i * 37 + 11) & 0xff).unwrap_or(0))
            .collect::<Vec<_>>(),
    )
    .expect("write");
    let text = scratch("text.las");
    std::fs::write(&text, b"ncols 1\n").expect("write");
    for (path, what) in [
        (truncated_header, "a truncated header"),
        (truncated_points, "a truncated point record"),
        (garbage, "garbage"),
        (text, "text"),
    ] {
        match pointcloud::load_las(&path) {
            Err(DataError::Parse(message)) => {
                assert!(message.contains(".las"), "{what}: {message}");
            }
            other => panic!("{what}: expected a parse error, got {}", describe(&other)),
        }
    }
    assert!(matches!(
        pointcloud::load_las(&fixture("absent.las")),
        Err(DataError::Io(_))
    ));
}

/// The LAS/LAZ half of the `gungnir-data` Loader correctness per format row
/// (`docs/verification-capability-table.md` §2), on its corrupt-file side: a LAZ file cut
/// short at **every** length from empty to one byte short is a `DataError::Parse` naming
/// the file, never a panic and never a cloud.
///
/// Every length rather than a chosen few, because the file is a few hundred bytes and the
/// interesting places to cut it -- inside the header, the LAZ VLR, the chunk-table
/// offset, the compressed records, the chunk table at the end -- are exactly the ones a
/// hand-picked list is most likely to miss.
#[test]
fn a_laz_file_truncated_at_any_length_is_an_error_never_a_panic() {
    let whole = std::fs::read(five_points_as_laz("five-points-to-truncate.laz")).expect("written");
    let cut = scratch("truncated.laz");
    for length in 0..whole.len() {
        std::fs::write(&cut, &whole[..length]).expect("write");
        match pointcloud::load_las(&cut) {
            Err(DataError::Parse(message)) => {
                assert!(
                    message.contains("truncated.laz"),
                    "{length} of {} bytes: {message}",
                    whole.len()
                );
            }
            other => panic!(
                "{length} of {} bytes: expected a parse error, got {}",
                whole.len(),
                describe(&other)
            ),
        }
    }
}

/// A real COPC file, its own octree hierarchy queried through the happy path.
///
/// `testdata/pointcloud/autzen-classified.copc.laz` (`SOURCE.md`): a public,
/// CC-BY-4.0, real LIDAR capture -- not authored for this test, and not the `las`
/// crate's own doctest fixture either, so this is a second, independent confirmation
/// that `load_copc_bounded`'s query reaches a real hierarchy rather than one shaped to
/// match this function's own assumptions. The bounds are a round-number 100 x 100 m box
/// with a generous z range, chosen from the file's own reported bounds
/// (`testdata/pointcloud/SOURCE.md`) rather than from what the query happens to return,
/// so the count below is a real check and not a tautology.
#[test]
fn a_bounded_query_recovers_points_from_a_real_copc_hierarchy() {
    let cloud =
        pointcloud::load_copc_bounded(&fixture("autzen-classified.copc.laz"), COPC_QUERY_BOX)
            .expect("a real COPC file with points in this box");
    assert_eq!(cloud.positions.len(), 4767);
    assert!(
        (cloud.origin[0] - 635_577.79).abs() < 0.01
            && (cloud.origin[1] - 848_882.15).abs() < 0.01
            && (cloud.origin[2] - 406.14).abs() < 0.01,
        "origin is the file's own minimum bound, not the query box: {:?}",
        cloud.origin
    );
    let [min, max] = cloud.bounds().expect("4767 points have bounds");
    // The query box, not the file's: every returned point is inside what was asked
    // for, and the box was wide enough on x and y that the returned points do not
    // fill it exactly on z (the ground and the low vegetation above it stop well
    // short of 620 m).
    let query_box = [
        [637_200.0, 637_300.0],
        [851_100.0, 851_200.0],
        [400.0, 620.0],
    ];
    for axis in 0..3 {
        let (got_min, got_max) = (min[axis], max[axis]);
        let [box_min, box_max]: [f64; 2] = query_box[axis];
        assert!(
            got_min >= box_min - 0.01 && got_max <= box_max + 0.01,
            "axis {axis}: [{got_min}, {got_max}] outside the query box [{box_min}, {box_max}]"
        );
    }
    assert!(cloud.intensity.is_some());
    let classification = cloud.classification.as_deref().expect("classification");
    // Ground (2), high vegetation (5), overhead structure (19), and car (65): a real
    // classification, not four points that happen to be ground.
    let mut counts = std::collections::BTreeMap::new();
    for &c in classification {
        *counts.entry(c).or_insert(0u32) += 1;
    }
    assert_eq!(
        counts,
        std::collections::BTreeMap::from([(0, 2), (2, 4499), (5, 114), (19, 8), (65, 144)]),
        "classification counts changed: {counts:?}"
    );
}

/// **What this proves, and what it does not.** `load_copc_bounded` is implemented
/// against `las::copc::CopcReader`, whose own doctest (`las-0.11.1/src/copc.rs`) reads
/// real points from a real COPC file through the same `query` call this function makes.
/// The test above is a second, independent confirmation against a different file. What
/// remains here is the refusal side: a plain LAZ/LAS file (no COPC info VLR, no
/// hierarchy EVLR) is refused by name rather than silently read as if it were bounded,
/// and malformed bounds are refused before the file is even opened.
#[test]
fn a_plain_laz_file_has_no_copc_hierarchy_and_is_refused_by_name() {
    match pointcloud::load_copc_bounded(&fixture("five-points.las"), [0.0, 0.0, 0.0, 1.0, 1.0, 1.0])
    {
        Err(DataError::Parse(message)) => {
            assert!(
                message.to_lowercase().contains("copc"),
                "expected the refusal to name what was missing: {message}"
            );
        }
        other => panic!(
            "a file with no COPC hierarchy must be refused, not read: {}",
            describe(&other)
        ),
    }
}

#[test]
fn non_finite_or_inverted_bounds_are_refused_before_the_file_is_opened() {
    for bad in [
        [f32::NAN, 0.0, 0.0, 1.0, 1.0, 1.0],
        [0.0, 0.0, 0.0, f32::INFINITY, 1.0, 1.0],
        // min_x (5.0) past max_x (1.0): inverted on one axis is still inverted.
        [5.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    ] {
        match pointcloud::load_copc_bounded(&fixture("absent.copc.laz"), bad) {
            Err(DataError::Parse(message)) => {
                assert!(message.contains(&format!("{bad:?}")), "{message}");
            }
            other => panic!("{bad:?}: expected a parse error, got {}", describe(&other)),
        }
    }
}

#[test]
fn a_missing_copc_file_is_an_io_error() {
    assert!(matches!(
        pointcloud::load_copc_bounded(&fixture("absent.copc.laz"), [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),
        Err(DataError::Io(_))
    ));
}

/// The COPC half of the `gungnir-data` Loader correctness per format row
/// (`docs/verification-capability-table.md` §2): a bounded query's exact minimum and
/// maximum on every axis, with its count and classification counts, against figures that
/// did not come from the query.
///
/// **Where the expected figures come from.** Not from `load_copc_bounded`, and not from
/// the file's octree: the whole file is read front to back through `las::Reader`, which
/// walks the LAZ chunk table from the first chunk to the last and knows nothing of the
/// COPC hierarchy, and every point whose world coordinates lie inside the closed query box
/// is kept. Their extremes, count and classifications are what the query must return. The
/// two paths share the `las` header parse, the `las` scale-and-offset decode, and the
/// `laz` decompressor that turns a chunk into records; so this is independent of the
/// query -- which nodes it selects, how it tests a point against the box, and how it
/// stores what it keeps -- and not of LAZ decoding itself.
///
/// A [`PointBuffer`] holds each position as an `f32` offset from `origin` (its own
/// documented representation), so the bounds it reports are the extremes after that
/// storage. The comparison applies the same storage to the independent extremes and is
/// then exact.
#[test]
#[allow(clippy::float_cmp, clippy::cast_possible_truncation)]
fn a_bounded_copc_query_returns_the_extremes_a_front_to_back_read_finds() {
    let path = fixture("autzen-classified.copc.laz");
    let [lo_x, lo_y, lo_z, hi_x, hi_y, hi_z] = COPC_QUERY_BOX.map(f64::from);
    let (lo, hi) = ([lo_x, lo_y, lo_z], [hi_x, hi_y, hi_z]);

    let mut reader = las::Reader::from_path(&path).expect("the fixture reads front to back");
    let file_min = reader.header().bounds().min;
    let promised = reader.header().number_of_points();
    let mut batch = las::PointDataBuilder::new()
        .for_header(reader.header())
        .build();
    let (mut read, mut count) = (0u64, 0usize);
    let (mut min, mut max) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    let mut classes = BTreeMap::new();
    loop {
        let n = reader
            .fill_points(1_000_000, &mut batch)
            .expect("a batch of records");
        if n == 0 {
            break;
        }
        read += n;
        let points = batch
            .x()
            .zip(batch.y())
            .zip(batch.z())
            .map(|((x, y), z)| [x, y, z]);
        for (point, class) in points.zip(batch.classification()) {
            if (0..3).all(|axis| lo[axis] <= point[axis] && point[axis] <= hi[axis]) {
                count += 1;
                for axis in 0..3 {
                    min[axis] = min[axis].min(point[axis]);
                    max[axis] = max[axis].max(point[axis]);
                }
                *classes.entry(class).or_insert(0u32) += 1;
            }
        }
    }
    assert_eq!(
        read, promised,
        "the front-to-back read reached every record"
    );

    let cloud = pointcloud::load_copc_bounded(&path, COPC_QUERY_BOX).expect("the query");
    assert_eq!(
        cloud.origin,
        [file_min.x, file_min.y, file_min.z],
        "the origin is the header's minimum"
    );
    assert_eq!(cloud.positions.len(), count, "points in the box");
    let stored =
        |v: f64, axis: usize| f64::from((v - cloud.origin[axis]) as f32) + cloud.origin[axis];
    let [got_min, got_max] = cloud.bounds().expect("points in the box have bounds");
    for axis in 0..3 {
        assert_eq!(
            got_min[axis],
            stored(min[axis], axis),
            "axis {axis}: the lowest point in the box is at {}",
            min[axis]
        );
        assert_eq!(
            got_max[axis],
            stored(max[axis], axis),
            "axis {axis}: the highest point in the box is at {}",
            max[axis]
        );
    }
    let mut got_classes = BTreeMap::new();
    for &c in cloud.classification.as_deref().expect("classification") {
        *got_classes.entry(c).or_insert(0u32) += 1;
    }
    assert_eq!(got_classes, classes, "classification counts in the box");
}

/// The COPC half of the `gungnir-data` Loader correctness per format row's corrupt-file
/// side (`docs/verification-capability-table.md` §2): the real COPC fixture cut short at
/// six lengths, one inside each part of the file a bounded read depends on, is a
/// `DataError::Parse` naming the file, never a panic and never a cloud.
///
/// The cut points are read from the file's own LAS 1.4 public header block (header size at
/// byte 94, offset to point data at 96, start of the first EVLR at 235, EVLR count at 243)
/// and from that EVLR's own header, not from anything the loader reports. In this file the
/// one EVLR is the COPC hierarchy and it is the last thing in the file, which the test
/// checks first: so every cut removes at least part of the hierarchy.
#[test]
fn a_copc_file_truncated_inside_each_of_its_parts_is_an_error_never_a_panic() {
    let whole = std::fs::read(fixture("autzen-classified.copc.laz")).expect("fixture");
    // A little-endian unsigned field of `width` bytes at `at`.
    let field = |at: usize, width: usize| {
        whole[at..at + width]
            .iter()
            .rev()
            .fold(0u64, |v, &b| (v << 8) | u64::from(b))
    };
    let index = |v: u64| usize::try_from(v).expect("an offset in an 81 MB file");
    let header_size = index(field(94, 2));
    let point_data = index(field(96, 4));
    let first_evlr = index(field(235, 8));
    let evlr_count = field(243, 4);
    // An EVLR header is 60 bytes; its record length is the u64 at byte 20 of it.
    let hierarchy_length = index(field(first_evlr + 20, 8));
    assert!(
        header_size < 1_000 && 1_000 < point_data && point_data < first_evlr,
        "the layout this test cuts through: header {header_size}, data at {point_data}, \
         EVLRs at {first_evlr}"
    );
    assert_eq!(
        (evlr_count, first_evlr + 60 + hierarchy_length),
        (1, whole.len()),
        "one EVLR, ending the file"
    );

    let cut = scratch("truncated.copc.laz");
    for (length, what) in [
        (100, "inside the public header block"),
        (1_000, "inside the VLRs, the header whole"),
        (
            point_data + 8,
            "the chunk-table offset and not one compressed chunk",
        ),
        (whole.len() / 2, "halfway through the compressed chunks"),
        (
            first_evlr + 60,
            "the hierarchy EVLR's header without its page",
        ),
        (whole.len() - 1, "one byte short, inside the hierarchy page"),
    ] {
        std::fs::write(&cut, &whole[..length]).expect("write");
        match pointcloud::load_copc_bounded(&cut, COPC_QUERY_BOX) {
            Err(DataError::Parse(message)) => {
                assert!(message.contains("truncated.copc.laz"), "{what}: {message}");
            }
            other => panic!(
                "{what} ({length} bytes): expected a parse error, got {}",
                describe(&other)
            ),
        }
    }
    // Up to 81 MB a copy: not left in the temp directory for every run.
    let _ = std::fs::remove_file(&cut);
}

#[test]
fn the_loader_thread_dispatches_a_copc_bounded_request() {
    let (requests, results) = gungnir_data::spawn_loader();
    requests
        .send(LoadRequest::CopcBounded(
            fixture("five-points.las"),
            [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        ))
        .expect("loader alive");
    match results
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("a result")
    {
        // Dispatch is what this test proves; the file is deliberately not a COPC one,
        // so a named refusal (not a hang, not a panic) is the right outcome here.
        LoadResult::CopcBounded(Err(DataError::Parse(message))) => {
            assert!(message.to_lowercase().contains("copc"), "{message}");
        }
        LoadResult::CopcBounded(other) => panic!("expected a named refusal: {}", describe(&other)),
        _ => panic!("wrong result kind"),
    }
}

#[test]
fn the_loader_thread_dispatches_point_clouds() {
    let (requests, results) = gungnir_data::spawn_loader();
    requests
        .send(LoadRequest::PointCloud(fixture("five-points.las")))
        .expect("loader alive");
    match results
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("a result")
    {
        LoadResult::PointCloud(Ok(cloud)) => assert_eq!(cloud.positions.len(), 5),
        LoadResult::PointCloud(Err(e)) => panic!("{e}"),
        _ => panic!("wrong result kind"),
    }
}

fn describe(r: &Result<pointcloud::PointBuffer, DataError>) -> String {
    match r {
        Ok(c) => format!("ok with {} points", c.positions.len()),
        Err(e) => e.to_string(),
    }
}

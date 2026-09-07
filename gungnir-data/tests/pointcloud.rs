//! The LAS loader against its verification row (`verification-capability-table.md` §2,
//! `gungnir-data`): the fixture loads to exact counts and bounds, and a corrupt file is a
//! `DataError`, never a panic. Figures in `testdata/pointcloud/SOURCE.md`.

use std::path::{Path, PathBuf};

use gungnir_data::pointcloud;
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

#[test]
#[allow(clippy::float_cmp)]
fn the_fixture_loads_to_its_recorded_figures() {
    let cloud = pointcloud::load_las(&fixture("five-points.las")).expect("loads");
    assert_eq!(cloud.positions.len(), 5);
    assert_eq!(
        cloud.origin,
        [500_010.0, 6_000_019.0, 12.0],
        "the header's minimum"
    );
    let [min, max] = cloud.bounds().expect("five points have bounds");
    for (got, want) in min.iter().zip(&[500_010.0, 6_000_019.0, 12.0]) {
        assert!((got - want).abs() < 1e-6, "{min:?}");
    }
    for (got, want) in max.iter().zip(&[500_013.5, 6_000_022.0, 18.25]) {
        assert!((got - want).abs() < 1e-6, "{max:?}");
    }
    // Positions are relative to the origin, so a UTM coordinate survives the f32.
    assert!(
        (cloud.positions[1][0] - 1.25).abs() < 1e-6,
        "{:?}",
        cloud.positions[1]
    );
    assert_eq!(
        cloud.classification.as_deref(),
        Some(&[2u8, 2, 5, 6, 2][..])
    );
    let intensity = cloud.intensity.as_deref().expect("intensity");
    assert!((intensity[4] - 180.0).abs() < f32::EPSILON);
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

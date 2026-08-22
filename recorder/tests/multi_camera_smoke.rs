// SPDX-License-Identifier: AGPL-3.0-only

#[cfg(windows)]
#[allow(unsafe_code)]
fn monotonic_time_ns() -> i64 {
    use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

    let mut counter = 0_i64;
    let mut frequency = 0_i64;
    unsafe {
        QueryPerformanceFrequency(&raw mut frequency).unwrap();
        QueryPerformanceCounter(&raw mut counter).unwrap();
    }
    i64::try_from(i128::from(counter) * 1_000_000_000 / i128::from(frequency)).unwrap()
}

#[cfg(windows)]
#[test]
#[ignore = "requires two physical Windows cameras"]
fn records_two_physical_cameras_concurrently() {
    use std::{thread, time::Duration};

    use daw_video_recorder_service::{
        capture::CaptureHandle, encoding::EncodingProfile, video_source::enumerate_video_sources,
    };

    let sources = enumerate_video_sources().expect("camera enumeration failed");
    assert!(sources.len() >= 2, "two cameras are required");
    let first = CaptureHandle::prepare(sources[0].source_id.clone())
        .expect("first camera preparation failed");
    let second = CaptureHandle::prepare(sources[1].source_id.clone())
        .expect("second camera preparation failed");
    let profile = EncodingProfile::default();
    first
        .prepare_profile(profile)
        .expect("first camera encoder failed");
    second
        .prepare_profile(profile)
        .expect("second camera encoder failed");

    let directory = tempfile::tempdir().unwrap();
    let first_output = directory.path().join("camera-one.mp4");
    let second_output = directory.path().join("camera-two.mp4");
    first
        .begin_recording("camera-one".into(), first_output.clone(), 0, profile)
        .expect("first camera recording failed");
    second
        .begin_recording("camera-two".into(), second_output.clone(), 0, profile)
        .expect("second camera recording failed");
    thread::sleep(Duration::from_secs(2));

    let logical_stop = monotonic_time_ns();
    let first_stats = first
        .end_recording("camera-one".into(), logical_stop)
        .expect("first camera finalization failed");
    let second_stats = second
        .end_recording("camera-two".into(), logical_stop)
        .expect("second camera finalization failed");
    eprintln!(
        "recorded {} and {} packets from {} and {}",
        first_stats.frame_count,
        second_stats.frame_count,
        sources[0].display_name,
        sources[1].display_name
    );
    assert!(first_stats.frame_count > 0);
    assert!(second_stats.frame_count > 0);
    assert!(first_output.metadata().unwrap().len() > 0);
    assert!(second_output.metadata().unwrap().len() > 0);
}

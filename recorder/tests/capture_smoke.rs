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
#[ignore = "requires a physical Windows camera"]
fn records_with_the_shared_encoder_pipeline() {
    use std::{thread, time::Duration};

    use daw_video_recorder_service::{
        capture::CaptureHandle, encoding::EncodingProfile, video_source::enumerate_video_sources,
    };

    let source = enumerate_video_sources()
        .expect("camera enumeration failed")
        .into_iter()
        .next()
        .expect("no camera was detected");
    eprintln!("opening {}", source.display_name);
    let capture = CaptureHandle::prepare(source.source_id).expect("camera preparation failed");
    eprintln!("preparing shared H.264 profile");
    capture
        .prepare_profile(EncodingProfile::default())
        .expect("encoder preparation failed");
    let software_profile = EncodingProfile {
        bit_rate: 4_000_000,
        gop_duration_ms: 1_000,
        prefer_hardware: false,
    };
    capture
        .prepare_profile(software_profile)
        .expect("software encoder preparation failed");

    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("shared-encoder-smoke.mp4");
    let software_output = directory.path().join("software-encoder-smoke.mp4");
    capture
        .begin_recording(
            "capture-smoke".into(),
            output.clone(),
            0,
            EncodingProfile::default(),
        )
        .expect("recording start failed");
    capture
        .begin_recording(
            "capture-smoke-software".into(),
            software_output.clone(),
            0,
            software_profile,
        )
        .expect("software recording start failed");
    thread::sleep(Duration::from_secs(2));
    let logical_stop = monotonic_time_ns();
    let stats = capture
        .end_recording("capture-smoke".into(), logical_stop)
        .expect("recording finalization failed");
    let software_stats = capture
        .end_recording("capture-smoke-software".into(), logical_stop)
        .expect("software recording finalization failed");
    eprintln!(
        "recorded {} packets to {}",
        stats.frame_count,
        output.display()
    );
    assert!(stats.frame_count > 0);
    assert!(output.metadata().unwrap().len() > 0);
    assert!(software_stats.frame_count > 0);
    assert!(software_output.metadata().unwrap().len() > 0);
    assert_eq!(
        software_stats.encoding_backend,
        daw_video_recorder_service::encoding::EncodingBackend::Software
    );
}

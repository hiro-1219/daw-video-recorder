// SPDX-License-Identifier: AGPL-3.0-only

use std::{fs, io, path::Path};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TakeMetadata {
    pub schema_version: u32,
    pub take_id: String,
    pub session_id: String,
    pub persistent_instance_id: String,
    pub runtime_instance_id: String,
    pub instance_label: String,
    pub source_id: String,
    pub source_display_name: String,
    pub encoding_profile: String,
    #[serde(default)]
    pub encoding_backend: String,
    pub output_video_path: String,
    pub record_started_at: String,
    pub record_stopped_at: String,
    pub start_project_sample: Option<i64>,
    pub end_project_sample: Option<i64>,
    pub sample_rate: f64,
    pub start_host_time_ns: Option<i64>,
    pub start_monotonic_time_ns: i64,
    #[serde(default)]
    pub first_frame_monotonic_time_ns: Option<i64>,
    pub sync_offset_ns: i64,
    #[serde(default)]
    pub configured_sync_offset_ns: i64,
    #[serde(default)]
    pub last_frame_monotonic_time_ns: Option<i64>,
    #[serde(default)]
    pub end_sync_offset_ns: Option<i64>,
    pub frame_count: u64,
    pub dropped_frame_count: u64,
    #[serde(default)]
    pub buffered_frame_count: u64,
    #[serde(default)]
    pub discarded_tail_frame_count: u64,
    #[serde(default)]
    pub stop_holdback_exceeded: bool,
    #[serde(default)]
    pub clock_drift_duration_ns: Option<i64>,
    #[serde(default)]
    pub clock_drift_ns: Option<i64>,
    #[serde(default)]
    pub clock_drift_ppm: Option<f64>,
    #[serde(default)]
    pub clock_drift_start_frame_monotonic_time_ns: Option<i64>,
    #[serde(default)]
    pub clock_drift_end_frame_monotonic_time_ns: Option<i64>,
    pub warnings: Vec<String>,
    pub finalization_status: String,
}

pub fn write_json(path: &Path, metadata: &TakeMetadata) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "metadata path has no parent")
    })?;
    fs::create_dir_all(parent)?;

    let temporary_path = path.with_extension("json.tmp");
    let serialized = serde_json::to_vec_pretty(metadata)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    fs::write(&temporary_path, serialized)?;
    fs::rename(temporary_path, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_metadata_atomically_to_the_requested_path() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Recordings").join("take.sync.json");
        let metadata = TakeMetadata {
            schema_version: 2,
            take_id: "take-1".into(),
            session_id: "session-1".into(),
            persistent_instance_id: "persistent".into(),
            runtime_instance_id: "runtime".into(),
            instance_label: "Camera".into(),
            source_id: "camera-1".into(),
            source_display_name: "Camera 1".into(),
            encoding_profile: "native-to-h264".into(),
            encoding_backend: "software".into(),
            output_video_path: "take.mp4".into(),
            record_started_at: "2026-08-22T00:00:00Z".into(),
            record_stopped_at: "2026-08-22T00:01:00Z".into(),
            start_project_sample: Some(0),
            end_project_sample: Some(2_880_000),
            sample_rate: 48_000.0,
            start_host_time_ns: None,
            start_monotonic_time_ns: 10,
            first_frame_monotonic_time_ns: Some(12),
            sync_offset_ns: 0,
            configured_sync_offset_ns: 0,
            last_frame_monotonic_time_ns: Some(20),
            end_sync_offset_ns: Some(-2),
            frame_count: 1_800,
            dropped_frame_count: 0,
            buffered_frame_count: 2,
            discarded_tail_frame_count: 3,
            stop_holdback_exceeded: false,
            clock_drift_duration_ns: Some(60_000_000_000),
            clock_drift_ns: Some(1_000),
            clock_drift_ppm: Some(0.016_666_666_7),
            clock_drift_start_frame_monotonic_time_ns: Some(12),
            clock_drift_end_frame_monotonic_time_ns: Some(20),
            warnings: Vec::new(),
            finalization_status: "complete".into(),
        };

        write_json(&path, &metadata).unwrap();
        let restored: TakeMetadata = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(restored, metadata);
    }

    #[test]
    fn newly_added_timing_fields_default_when_reading_older_metadata() {
        let value = serde_json::json!({
            "schema_version": 1,
            "take_id": "take-1",
            "session_id": "session-1",
            "persistent_instance_id": "persistent",
            "runtime_instance_id": "runtime",
            "instance_label": "Camera",
            "source_id": "camera-1",
            "source_display_name": "Camera 1",
            "encoding_profile": "native-to-h264",
            "output_video_path": "take.mp4",
            "record_started_at": "2026-08-22T00:00:00Z",
            "record_stopped_at": "2026-08-22T00:01:00Z",
            "start_project_sample": 0,
            "end_project_sample": 2_880_000,
            "sample_rate": 48_000.0,
            "start_host_time_ns": null,
            "start_monotonic_time_ns": 10,
            "sync_offset_ns": 0,
            "frame_count": 1_800,
            "dropped_frame_count": 0,
            "warnings": [],
            "finalization_status": "complete"
        });
        let restored: TakeMetadata = serde_json::from_value(value).unwrap();
        assert_eq!(restored.first_frame_monotonic_time_ns, None);
        assert_eq!(restored.buffered_frame_count, 0);
        assert_eq!(restored.last_frame_monotonic_time_ns, None);
        assert_eq!(restored.end_sync_offset_ns, None);
        assert_eq!(restored.discarded_tail_frame_count, 0);
        assert!(!restored.stop_holdback_exceeded);
        assert_eq!(restored.encoding_backend, "");
        assert_eq!(restored.configured_sync_offset_ns, 0);
        assert_eq!(restored.clock_drift_ns, None);
        assert_eq!(restored.clock_drift_start_frame_monotonic_time_ns, None);
        assert_eq!(restored.clock_drift_end_frame_monotonic_time_ns, None);
    }
}

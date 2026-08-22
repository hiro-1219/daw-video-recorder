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
    pub output_video_path: String,
    pub record_started_at: String,
    pub record_stopped_at: String,
    pub start_project_sample: Option<i64>,
    pub end_project_sample: Option<i64>,
    pub sample_rate: f64,
    pub start_host_time_ns: Option<i64>,
    pub start_monotonic_time_ns: i64,
    pub sync_offset_ns: i64,
    pub frame_count: u64,
    pub dropped_frame_count: u64,
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
            schema_version: 1,
            take_id: "take-1".into(),
            session_id: "session-1".into(),
            persistent_instance_id: "persistent".into(),
            runtime_instance_id: "runtime".into(),
            instance_label: "Camera".into(),
            source_id: "camera-1".into(),
            source_display_name: "Camera 1".into(),
            encoding_profile: "native-to-h264".into(),
            output_video_path: "take.mp4".into(),
            record_started_at: "2026-08-22T00:00:00Z".into(),
            record_stopped_at: "2026-08-22T00:01:00Z".into(),
            start_project_sample: Some(0),
            end_project_sample: Some(2_880_000),
            sample_rate: 48_000.0,
            start_host_time_ns: None,
            start_monotonic_time_ns: 10,
            sync_offset_ns: 0,
            frame_count: 1_800,
            dropped_frame_count: 0,
            warnings: Vec::new(),
            finalization_status: "complete".into(),
        };

        write_json(&path, &metadata).unwrap();
        let restored: TakeMetadata = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(restored, metadata);
    }
}

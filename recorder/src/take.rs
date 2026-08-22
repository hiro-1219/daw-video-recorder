// SPDX-License-Identifier: AGPL-3.0-only

use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

use chrono::{SecondsFormat, Utc};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    capture::{CaptureError, CaptureHandle},
    metadata::{TakeMetadata, write_json},
    protocol::TransportSnapshot,
};

#[derive(Debug, Clone)]
pub struct TakeContext {
    pub persistent_instance_id: String,
    pub runtime_instance_id: String,
    pub instance_label: String,
    pub source_id: String,
    pub source_display_name: String,
    pub project_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalizedTake {
    pub runtime_instance_id: String,
    pub take_id: String,
    pub video_path: PathBuf,
    pub metadata_path: PathBuf,
}

#[derive(Debug, Error)]
pub enum TakeError {
    #[error("recording is already active for runtime instance: {0}")]
    AlreadyActive(String),
    #[error("no recording is active for runtime instance: {0}")]
    NotActive(String),
    #[error("camera capture is not prepared for runtime instance: {0}")]
    NotPrepared(String),
    #[error("prepared camera does not match the selected source")]
    PreparedSourceMismatch,
    #[error("project root does not exist or cannot be resolved: {0}")]
    InvalidProjectRoot(PathBuf),
    #[error("Recordings directory resolved outside the project root")]
    RecordingsOutsideProject,
    #[error(transparent)]
    Capture(#[from] CaptureError),
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Debug)]
struct ActiveTake {
    context: TakeContext,
    take_id: String,
    started_at: String,
    start_snapshot: TransportSnapshot,
    partial_video_path: PathBuf,
    video_path: PathBuf,
    metadata_path: PathBuf,
}

#[derive(Debug)]
struct PreparedCapture {
    source_id: String,
    capture: CaptureHandle,
}

#[derive(Debug, Default)]
pub struct TakeCoordinator {
    prepared: HashMap<String, PreparedCapture>,
    active: HashMap<String, ActiveTake>,
}

impl TakeCoordinator {
    pub fn prepare(&mut self, runtime_instance_id: &str, source_id: &str) -> Result<(), TakeError> {
        if self.active.contains_key(runtime_instance_id) {
            return Err(TakeError::AlreadyActive(runtime_instance_id.to_owned()));
        }
        if self
            .prepared
            .get(runtime_instance_id)
            .is_some_and(|prepared| prepared.source_id == source_id)
        {
            return Ok(());
        }

        self.prepared.remove(runtime_instance_id);
        let capture = CaptureHandle::prepare(source_id.to_owned())?;
        self.prepared.insert(
            runtime_instance_id.to_owned(),
            PreparedCapture {
                source_id: source_id.to_owned(),
                capture,
            },
        );
        tracing::info!(runtime_instance_id, source_id, "camera capture prepared");
        Ok(())
    }

    pub fn unprepare(&mut self, runtime_instance_id: &str) {
        if !self.active.contains_key(runtime_instance_id) {
            self.prepared.remove(runtime_instance_id);
        }
    }

    pub fn start(
        &mut self,
        context: TakeContext,
        snapshot: TransportSnapshot,
    ) -> Result<(), TakeError> {
        if self.active.contains_key(&context.runtime_instance_id) {
            return Err(TakeError::AlreadyActive(context.runtime_instance_id));
        }
        let prepared = self
            .prepared
            .get(&context.runtime_instance_id)
            .ok_or_else(|| TakeError::NotPrepared(context.runtime_instance_id.clone()))?;
        if prepared.source_id != context.source_id {
            return Err(TakeError::PreparedSourceMismatch);
        }

        let recordings_root = prepare_recordings_root(&context.project_root)?;
        let now = Utc::now();
        let timestamp = now.format("%Y%m%dT%H%M%S%.3fZ").to_string();
        let take_id = format!("{timestamp}-{}", Uuid::new_v4().simple());
        let label = safe_path_component(&context.instance_label);
        let take_directory = recordings_root.join(format!("{label}_{take_id}"));
        fs::create_dir(&take_directory)?;

        let base_name = format!("{label}_{take_id}");
        let partial_video_path = take_directory.join(format!("{base_name}.partial.mp4"));
        let video_path = take_directory.join(format!("{base_name}.mp4"));
        let metadata_path = take_directory.join(format!("{base_name}.sync.json"));
        if let Err(error) = prepared.capture.begin_recording(
            partial_video_path.clone(),
            snapshot.callback_monotonic_time_ns,
        ) {
            let _ = fs::remove_dir(&take_directory);
            return Err(error.into());
        }

        tracing::info!(
            runtime_instance_id = %context.runtime_instance_id,
            take_id,
            path = %partial_video_path.display(),
            "recording take started"
        );

        self.active.insert(
            context.runtime_instance_id.clone(),
            ActiveTake {
                context,
                take_id,
                started_at: now.to_rfc3339_opts(SecondsFormat::Millis, true),
                start_snapshot: snapshot,
                partial_video_path,
                video_path,
                metadata_path,
            },
        );
        Ok(())
    }

    pub fn stop(
        &mut self,
        runtime_instance_id: &str,
        stop_snapshot: &TransportSnapshot,
    ) -> Result<FinalizedTake, TakeError> {
        let active = self
            .active
            .remove(runtime_instance_id)
            .ok_or_else(|| TakeError::NotActive(runtime_instance_id.to_owned()))?;
        let stats = self
            .prepared
            .get(runtime_instance_id)
            .ok_or_else(|| TakeError::NotPrepared(runtime_instance_id.to_owned()))?
            .capture
            .end_recording()?;
        tracing::info!(
            runtime_instance_id,
            take_id = %active.take_id,
            frames = stats.frame_count,
            "recording take capture stopped"
        );
        if active.video_path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "refusing to overwrite an existing take",
            )
            .into());
        }
        fs::rename(&active.partial_video_path, &active.video_path)?;

        let sync_offset_ns = stats
            .first_frame_monotonic_time_ns
            .map_or(0, |first_frame| {
                first_frame.saturating_sub(active.start_snapshot.callback_monotonic_time_ns)
            });
        let warnings = if stats.first_frame_monotonic_time_ns.is_some() {
            vec![
                "camera timestamps are correlated at frame delivery; camera sensor latency is not calibrated"
                    .into(),
            ]
        } else {
            vec!["no captured frame was available for DAW clock correlation".into()]
        };
        let metadata = TakeMetadata {
            schema_version: 1,
            take_id: active.take_id.clone(),
            session_id: active.context.runtime_instance_id.clone(),
            persistent_instance_id: active.context.persistent_instance_id,
            runtime_instance_id: active.context.runtime_instance_id.clone(),
            instance_label: active.context.instance_label,
            source_id: active.context.source_id,
            source_display_name: active.context.source_display_name,
            encoding_profile: "native-uncompressed-to-h264-mp4".into(),
            output_video_path: active.video_path.to_string_lossy().into_owned(),
            record_started_at: active.started_at,
            record_stopped_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            start_project_sample: active.start_snapshot.project_sample_position,
            end_project_sample: stop_snapshot.project_sample_position,
            sample_rate: active.start_snapshot.sample_rate,
            start_host_time_ns: active.start_snapshot.host_time_ns,
            start_monotonic_time_ns: active.start_snapshot.callback_monotonic_time_ns,
            first_frame_monotonic_time_ns: stats.first_frame_monotonic_time_ns,
            sync_offset_ns,
            frame_count: stats.frame_count,
            dropped_frame_count: stats.dropped_frame_count,
            buffered_frame_count: stats.buffered_frame_count,
            warnings,
            finalization_status: "complete".into(),
        };
        write_json(&active.metadata_path, &metadata)?;

        Ok(FinalizedTake {
            runtime_instance_id: active.context.runtime_instance_id,
            take_id: active.take_id,
            video_path: active.video_path,
            metadata_path: active.metadata_path,
        })
    }

    pub fn abort(&mut self, runtime_instance_id: &str) {
        self.active.remove(runtime_instance_id);
        self.prepared.remove(runtime_instance_id);
    }

    #[must_use]
    pub fn is_active(&self, runtime_instance_id: &str) -> bool {
        self.active.contains_key(runtime_instance_id)
    }

    #[must_use]
    pub fn is_prepared(&self, runtime_instance_id: &str) -> bool {
        self.prepared.contains_key(runtime_instance_id)
    }
}

fn prepare_recordings_root(project_root: &Path) -> Result<PathBuf, TakeError> {
    let canonical_project = project_root
        .canonicalize()
        .map_err(|_| TakeError::InvalidProjectRoot(project_root.to_owned()))?;
    if !canonical_project.is_dir() {
        return Err(TakeError::InvalidProjectRoot(project_root.to_owned()));
    }
    let recordings = canonical_project.join("Recordings");
    fs::create_dir_all(&recordings)?;
    let canonical_recordings = recordings.canonicalize()?;
    if !canonical_recordings.starts_with(&canonical_project) {
        return Err(TakeError::RecordingsOutsideProject);
    }
    Ok(canonical_recordings)
}

fn safe_path_component(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut last_was_separator = false;
    for character in value.chars() {
        if character.is_alphanumeric() || matches!(character, '-' | '_') {
            result.push(character);
            last_was_separator = false;
        } else if !last_was_separator && !result.is_empty() {
            result.push('-');
            last_was_separator = true;
        }
    }
    while result.ends_with('-') {
        result.pop();
    }
    if result.is_empty() {
        "camera".into()
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recordings_directory_is_created_under_project_root() {
        let project = tempfile::tempdir().unwrap();
        let recordings = prepare_recordings_root(project.path()).unwrap();
        assert_eq!(
            recordings,
            project.path().canonicalize().unwrap().join("Recordings")
        );
        assert!(recordings.is_dir());
    }

    #[test]
    fn file_components_remove_path_syntax() {
        assert_eq!(safe_path_component("Face / Camera: 1"), "Face-Camera-1");
        assert_eq!(safe_path_component("<>:\"/\\|?*"), "camera");
        assert_eq!(safe_path_component("手元カメラ"), "手元カメラ");
    }
}

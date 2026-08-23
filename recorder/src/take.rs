// SPDX-License-Identifier: AGPL-3.0-only

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::mpsc::SyncSender,
};

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    capture::{CaptureError, CaptureHandle, RawPreviewFrame},
    encoding::{EncodingProfile, measure_clock_drift},
    protocol::TransportSnapshot,
};

#[derive(Debug, Error)]
pub enum TakeError {
    #[error(transparent)]
    Capture(#[from] CaptureError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("project root is not a valid directory: {0}")]
    InvalidProjectRoot(PathBuf),
    #[error("recordings directory escaped project root")]
    RecordingsOutsideProject,
    #[error("take is already active for runtime instance: {0}")]
    AlreadyActive(String),
    #[error("take is not active for runtime instance: {0}")]
    NotActive(String),
    #[error("runtime instance is not prepared: {0}")]
    NotPrepared(String),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TakeContext {
    pub persistent_instance_id: String,
    pub runtime_instance_id: String,
    pub instance_label: String,
    pub source_id: String,
    pub source_display_name: String,
    pub project_root: PathBuf,
    pub encoding_profile: EncodingProfile,
    pub sync_offset_ns: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinalizedTake {
    pub runtime_instance_id: String,
    pub take_id: String,
    pub video_path: PathBuf,
    pub metadata_path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
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
    pub encoding_backend: String,
    pub output_video_path: String,
    pub record_started_at: String,
    pub record_stopped_at: String,
    pub start_project_sample: Option<i64>,
    pub end_project_sample: Option<i64>,
    pub sample_rate: f64,
    pub start_host_time_ns: Option<i64>,
    pub start_monotonic_time_ns: i64,
    pub first_frame_monotonic_time_ns: Option<i64>,
    pub sync_offset_ns: i64,
    pub configured_sync_offset_ns: i64,
    pub last_frame_monotonic_time_ns: Option<i64>,
    pub end_sync_offset_ns: Option<i64>,
    pub frame_count: u64,
    pub dropped_frame_count: u64,
    pub buffered_frame_count: u64,
    pub discarded_tail_frame_count: u64,
    pub stop_holdback_exceeded: bool,
    pub clock_drift_duration_ns: Option<i64>,
    pub clock_drift_ns: Option<i64>,
    pub clock_drift_ppm: Option<f64>,
    pub clock_drift_start_frame_monotonic_time_ns: Option<i64>,
    pub clock_drift_end_frame_monotonic_time_ns: Option<i64>,
    pub warnings: Vec<String>,
    pub finalization_status: String,
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
    encoding_profile: EncodingProfile,
}

#[derive(Debug)]
struct SharedCapture {
    capture: CaptureHandle,
    profile_users: HashMap<EncodingProfile, usize>,
    preview_users: HashSet<String>,
}

#[derive(Debug, Default)]
pub struct TakeCoordinator {
    prepared: HashMap<String, PreparedCapture>,
    captures: HashMap<String, SharedCapture>,
    active: HashMap<String, ActiveTake>,
    preview_subscriptions: HashMap<String, String>,
}

impl TakeCoordinator {
    pub fn prepare(
        &mut self,
        runtime_instance_id: &str,
        source_id: &str,
        encoding_profile: EncodingProfile,
    ) -> Result<(), TakeError> {
        if self.active.contains_key(runtime_instance_id) {
            return Err(TakeError::AlreadyActive(runtime_instance_id.to_owned()));
        }
        if self
            .prepared
            .get(runtime_instance_id)
            .is_some_and(|prepared| {
                prepared.source_id == source_id && prepared.encoding_profile == encoding_profile
            })
        {
            return Ok(());
        }

        self.release_prepared(runtime_instance_id);
        if !self.captures.contains_key(source_id) {
            let capture = CaptureHandle::prepare(source_id.to_owned())?;
            self.captures.insert(
                source_id.to_owned(),
                SharedCapture {
                    capture,
                    profile_users: HashMap::new(),
                    preview_users: HashSet::new(),
                },
            );
        }
        let shared = self
            .captures
            .get_mut(source_id)
            .expect("the shared capture exists after preparation");
        if !shared.profile_users.contains_key(&encoding_profile) {
            shared.capture.prepare_profile(encoding_profile)?;
        }
        *shared.profile_users.entry(encoding_profile).or_default() += 1;
        self.prepared.insert(
            runtime_instance_id.to_owned(),
            PreparedCapture {
                source_id: source_id.to_owned(),
                encoding_profile,
            },
        );
        tracing::info!(
            runtime_instance_id,
            source_id,
            profiles = shared.profile_users.len(),
            "shared camera capture prepared"
        );
        Ok(())
    }

    pub fn subscribe_preview(
        &mut self,
        runtime_instance_id: &str,
        source_id: &str,
        target_width: u32,
        target_height: u32,
        max_fps: u32,
        sender: SyncSender<RawPreviewFrame>,
    ) -> Result<(), TakeError> {
        self.unsubscribe_preview(runtime_instance_id);
        if !self.captures.contains_key(source_id) {
            let capture = CaptureHandle::prepare(source_id.to_owned())?;
            self.captures.insert(
                source_id.to_owned(),
                SharedCapture {
                    capture,
                    profile_users: HashMap::new(),
                    preview_users: HashSet::new(),
                },
            );
        }
        let shared = self
            .captures
            .get_mut(source_id)
            .expect("the shared capture exists after preparation");
        shared.capture.subscribe_preview(
            runtime_instance_id.to_owned(),
            target_width,
            target_height,
            max_fps,
            sender,
        )?;
        shared.preview_users.insert(runtime_instance_id.to_owned());
        self.preview_subscriptions
            .insert(runtime_instance_id.to_owned(), source_id.to_owned());
        Ok(())
    }

    pub fn unsubscribe_preview(&mut self, runtime_instance_id: &str) {
        if let Some(source_id) = self.preview_subscriptions.remove(runtime_instance_id) {
            if let Some(shared) = self.captures.get_mut(&source_id) {
                let _ = shared.capture.unsubscribe_preview(runtime_instance_id);
                shared.preview_users.remove(runtime_instance_id);
                if shared.profile_users.is_empty() && shared.preview_users.is_empty() {
                    self.captures.remove(&source_id);
                }
            }
        }
    }

    pub fn unprepare(&mut self, runtime_instance_id: &str) {
        if !self.active.contains_key(runtime_instance_id) {
            self.release_prepared(runtime_instance_id);
        }
    }

    pub fn start(
        &mut self,
        context: TakeContext,
        start_snapshot: TransportSnapshot,
    ) -> Result<(), TakeError> {
        let runtime_instance_id = context.runtime_instance_id.clone();
        if self.active.contains_key(&runtime_instance_id) {
            return Err(TakeError::AlreadyActive(runtime_instance_id));
        }

        self.prepare(
            &runtime_instance_id,
            &context.source_id,
            context.encoding_profile,
        )?;
        let recordings_root = prepare_recordings_root(&context.project_root)?;
        let take_id = format!("take-{}", Uuid::new_v4());
        let video_name = format!(
            "{}-{}.mp4",
            safe_path_component(&context.instance_label),
            take_id
        );
        let partial_video_name = format!("{video_name}.part");
        let metadata_name = format!("{video_name}.json");
        let partial_video_path = recordings_root.join(partial_video_name);
        let video_path = recordings_root.join(video_name);
        let metadata_path = recordings_root.join(metadata_name);

        let shared = self
            .captures
            .get_mut(&context.source_id)
            .ok_or_else(|| TakeError::NotPrepared(runtime_instance_id.clone()))?;
        shared.capture.begin_recording(
            runtime_instance_id.clone(),
            partial_video_path.clone(),
            start_snapshot.callback_monotonic_time_ns,
            context.encoding_profile,
        )?;

        self.active.insert(
            runtime_instance_id,
            ActiveTake {
                context,
                take_id,
                started_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
                start_snapshot,
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
        let shared = self
            .captures
            .get_mut(&active.context.source_id)
            .ok_or_else(|| TakeError::NotPrepared(runtime_instance_id.to_owned()))?;
        let stats = shared.capture.end_recording(
            runtime_instance_id.to_owned(),
            stop_snapshot.callback_monotonic_time_ns,
        )?;

        if active.partial_video_path.exists() {
            fs::rename(&active.partial_video_path, &active.video_path)?;
        }
        let corrected_start_monotonic_time_ns = active
            .start_snapshot
            .callback_monotonic_time_ns
            .saturating_add(active.context.sync_offset_ns);
        let corrected_stop_monotonic_time_ns = stop_snapshot
            .callback_monotonic_time_ns
            .saturating_add(active.context.sync_offset_ns);
        let sync_offset_ns = stats
            .first_frame_monotonic_time_ns
            .map_or(0, |first_frame| {
                first_frame.saturating_sub(corrected_start_monotonic_time_ns)
            });
        let end_sync_offset_ns = stats
            .last_frame_monotonic_time_ns
            .map(|last_frame| last_frame.saturating_sub(corrected_stop_monotonic_time_ns));
        let drift = measure_clock_drift(
            active.start_snapshot.project_sample_position,
            stop_snapshot.project_sample_position,
            active.start_snapshot.sample_rate,
            stats.drift_start_frame_monotonic_time_ns,
            stats.drift_end_frame_monotonic_time_ns,
        );
        let mut warnings = if stats.first_frame_monotonic_time_ns.is_some() {
            vec![
                "camera timestamps are correlated at frame delivery; camera sensor latency is not calibrated"
                    .into(),
            ]
        } else {
            vec!["no captured frame was available for DAW clock correlation".into()]
        };
        if stats.stop_holdback_exceeded {
            warnings.push(
                "DAW stop notification exceeded the output holdback; the video contains frames after the logical stop"
                    .into(),
            );
        }
        let metadata = TakeMetadata {
            schema_version: 2,
            take_id: active.take_id.clone(),
            session_id: active.context.runtime_instance_id.clone(),
            persistent_instance_id: active.context.persistent_instance_id,
            runtime_instance_id: active.context.runtime_instance_id.clone(),
            instance_label: active.context.instance_label,
            source_id: active.context.source_id,
            source_display_name: active.context.source_display_name,
            encoding_profile: active.context.encoding_profile.stable_id(),
            encoding_backend: stats.encoding_backend.as_str().into(),
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
            configured_sync_offset_ns: active.context.sync_offset_ns,
            last_frame_monotonic_time_ns: stats.last_frame_monotonic_time_ns,
            end_sync_offset_ns,
            frame_count: stats.frame_count,
            dropped_frame_count: stats.dropped_frame_count,
            buffered_frame_count: stats.buffered_frame_count,
            discarded_tail_frame_count: stats.discarded_tail_frame_count,
            stop_holdback_exceeded: stats.stop_holdback_exceeded,
            clock_drift_duration_ns: drift.map(|value| value.duration_ns),
            clock_drift_ns: drift.map(|value| value.drift_ns),
            clock_drift_ppm: drift.map(|value| value.drift_ppm),
            clock_drift_start_frame_monotonic_time_ns: stats.drift_start_frame_monotonic_time_ns,
            clock_drift_end_frame_monotonic_time_ns: stats.drift_end_frame_monotonic_time_ns,
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
        let active_source = self
            .active
            .remove(runtime_instance_id)
            .map(|active| active.context.source_id);
        let source_id = active_source.or_else(|| {
            self.prepared
                .get(runtime_instance_id)
                .map(|prepared| prepared.source_id.clone())
        });
        if let Some(source_id) = source_id
            && let Some(shared) = self.captures.get(&source_id)
        {
            let _ = shared
                .capture
                .abort_recording(runtime_instance_id.to_owned());
        }
        self.release_prepared(runtime_instance_id);
    }

    #[must_use]
    pub fn is_active(&self, runtime_instance_id: &str) -> bool {
        self.active.contains_key(runtime_instance_id)
    }

    #[must_use]
    pub fn is_prepared(&self, runtime_instance_id: &str) -> bool {
        self.prepared.contains_key(runtime_instance_id)
    }

    fn release_prepared(&mut self, runtime_instance_id: &str) {
        let Some(prepared) = self.prepared.remove(runtime_instance_id) else {
            return;
        };
        let remove_capture = self
            .captures
            .get_mut(&prepared.source_id)
            .is_some_and(|shared| {
                let remove_profile = shared
                    .profile_users
                    .get_mut(&prepared.encoding_profile)
                    .is_some_and(|users| {
                        *users = users.saturating_sub(1);
                        *users == 0
                    });
                if remove_profile {
                    shared.profile_users.remove(&prepared.encoding_profile);
                    let _ = shared.capture.release_profile(prepared.encoding_profile);
                }
                shared.profile_users.is_empty()
                    && shared.preview_users.is_empty()
                    && !self.preview_subscriptions.values().any(|src| src == &prepared.source_id)
            });
        if remove_capture {
            self.captures.remove(&prepared.source_id);
            tracing::info!(
                source_id = prepared.source_id,
                "unused shared camera capture released"
            );
        }
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

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), TakeError> {
    let temp_path = path.with_extension("json.tmp");
    let file = fs::File::create(&temp_path)?;
    serde_json::to_writer_pretty(file, value)?;
    fs::rename(temp_path, path)?;
    Ok(())
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

// SPDX-License-Identifier: AGPL-3.0-only

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    encoding::{EncodingProfile, EncodingProfileError},
    protocol::TransportSnapshot,
};

const MAX_SYNC_OFFSET_NS: i64 = 5_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStatus {
    Idle,
    Preparing,
    Armed,
    Recording,
    Finalizing,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RecordingAction {
    Start(TransportSnapshot),
    Stop(TransportSnapshot),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SessionError {
    #[error("operation is not valid while the session is {0:?}")]
    InvalidState(SessionStatus),
    #[error("project root must be an absolute path")]
    ProjectRootNotAbsolute,
    #[error("the capture pipeline cannot become ready without a source and project root")]
    NotConfigured,
    #[error(transparent)]
    InvalidEncodingProfile(#[from] EncodingProfileError),
    #[error("sync offset must be between -5000 and 5000 ms: {0} ns")]
    SyncOffsetOutOfRange(i64),
}

#[derive(Debug, Clone)]
pub struct RecordingSession {
    persistent_instance_id: String,
    runtime_instance_id: String,
    instance_label: String,
    selected_source_id: Option<String>,
    project_root: Option<PathBuf>,
    encoding_profile: EncodingProfile,
    sync_offset_ns: i64,
    armed_requested: bool,
    status: SessionStatus,
    last_sequence_number: Option<u64>,
    last_transport_recording: bool,
}

impl RecordingSession {
    #[must_use]
    pub fn new(
        persistent_instance_id: String,
        runtime_instance_id: String,
        instance_label: String,
    ) -> Self {
        Self {
            persistent_instance_id,
            runtime_instance_id,
            instance_label,
            selected_source_id: None,
            project_root: None,
            encoding_profile: EncodingProfile::default(),
            sync_offset_ns: 0,
            armed_requested: false,
            status: SessionStatus::Idle,
            last_sequence_number: None,
            last_transport_recording: false,
        }
    }

    #[must_use]
    pub fn persistent_instance_id(&self) -> &str {
        &self.persistent_instance_id
    }

    #[must_use]
    pub fn runtime_instance_id(&self) -> &str {
        &self.runtime_instance_id
    }

    #[must_use]
    pub fn instance_label(&self) -> &str {
        &self.instance_label
    }

    #[must_use]
    pub const fn status(&self) -> SessionStatus {
        self.status
    }

    #[must_use]
    pub const fn armed_requested(&self) -> bool {
        self.armed_requested
    }

    #[must_use]
    pub fn selected_source_id(&self) -> Option<&str> {
        self.selected_source_id.as_deref()
    }

    #[must_use]
    pub fn project_root(&self) -> Option<&Path> {
        self.project_root.as_deref()
    }

    #[must_use]
    pub const fn encoding_profile(&self) -> EncodingProfile {
        self.encoding_profile
    }

    #[must_use]
    pub const fn sync_offset_ns(&self) -> i64 {
        self.sync_offset_ns
    }

    pub fn select_source(&mut self, source_id: String) -> Result<(), SessionError> {
        if matches!(
            self.status,
            SessionStatus::Recording | SessionStatus::Finalizing
        ) {
            return Err(SessionError::InvalidState(self.status));
        }
        self.selected_source_id = Some(source_id);
        self.refresh_preparation_state();
        Ok(())
    }

    pub fn set_project_root(&mut self, project_root: PathBuf) -> Result<(), SessionError> {
        if !project_root.is_absolute() {
            return Err(SessionError::ProjectRootNotAbsolute);
        }
        if matches!(
            self.status,
            SessionStatus::Recording | SessionStatus::Finalizing
        ) {
            return Err(SessionError::InvalidState(self.status));
        }
        self.project_root = Some(project_root);
        self.refresh_preparation_state();
        Ok(())
    }

    pub fn set_encoding_profile(
        &mut self,
        encoding_profile: EncodingProfile,
    ) -> Result<(), SessionError> {
        if matches!(
            self.status,
            SessionStatus::Recording | SessionStatus::Finalizing
        ) {
            return Err(SessionError::InvalidState(self.status));
        }
        self.encoding_profile = encoding_profile.validate()?;
        self.refresh_preparation_state();
        Ok(())
    }

    pub fn set_sync_offset_ns(&mut self, sync_offset_ns: i64) -> Result<(), SessionError> {
        if matches!(
            self.status,
            SessionStatus::Recording | SessionStatus::Finalizing
        ) {
            return Err(SessionError::InvalidState(self.status));
        }
        if sync_offset_ns.abs() > MAX_SYNC_OFFSET_NS {
            return Err(SessionError::SyncOffsetOutOfRange(sync_offset_ns));
        }
        self.sync_offset_ns = sync_offset_ns;
        Ok(())
    }

    pub fn set_armed(&mut self, armed: bool) {
        self.armed_requested = armed;
        match self.status {
            SessionStatus::Recording | SessionStatus::Finalizing => {
                // The active take is not truncated. The request is applied after finalization.
            }
            _ if !armed => self.status = SessionStatus::Idle,
            _ => self.refresh_preparation_state(),
        }
    }

    pub fn mark_prepared(&mut self) -> Result<(), SessionError> {
        if self.status != SessionStatus::Preparing {
            return Err(SessionError::InvalidState(self.status));
        }
        if self.selected_source_id.is_none() || self.project_root.is_none() {
            return Err(SessionError::NotConfigured);
        }
        self.status = SessionStatus::Armed;
        Ok(())
    }

    pub fn handle_transport(&mut self, snapshot: TransportSnapshot) -> Option<RecordingAction> {
        if snapshot.is_offline_rendering {
            return None;
        }

        if self
            .last_sequence_number
            .is_some_and(|sequence| snapshot.sequence_number <= sequence)
        {
            return None;
        }
        self.last_sequence_number = Some(snapshot.sequence_number);

        let recording_started = snapshot.is_recording && !self.last_transport_recording;
        let recording_stopped = !snapshot.is_recording && self.last_transport_recording;
        self.last_transport_recording = snapshot.is_recording;

        if self.status == SessionStatus::Armed && recording_started {
            self.status = SessionStatus::Recording;
            return Some(RecordingAction::Start(snapshot));
        }

        if self.status == SessionStatus::Recording && recording_stopped {
            self.status = SessionStatus::Finalizing;
            return Some(RecordingAction::Stop(snapshot));
        }

        None
    }

    pub fn mark_finalized(&mut self) -> Result<(), SessionError> {
        if self.status != SessionStatus::Finalizing {
            return Err(SessionError::InvalidState(self.status));
        }
        self.status = if self.armed_requested {
            SessionStatus::Preparing
        } else {
            SessionStatus::Idle
        };
        Ok(())
    }

    pub fn fail(&mut self) {
        self.status = SessionStatus::Error;
    }

    fn refresh_preparation_state(&mut self) {
        if self.armed_requested && self.selected_source_id.is_some() && self.project_root.is_some()
        {
            self.status = SessionStatus::Preparing;
        } else if !matches!(
            self.status,
            SessionStatus::Recording | SessionStatus::Finalizing
        ) {
            self.status = SessionStatus::Idle;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(sequence_number: u64, is_recording: bool) -> TransportSnapshot {
        TransportSnapshot {
            runtime_instance_id: "runtime".into(),
            sequence_number,
            callback_monotonic_time_ns: 100,
            host_time_ns: None,
            project_sample_position: Some(512),
            sample_rate: 48_000.0,
            is_playing: is_recording,
            is_recording,
            is_looping: false,
            is_offline_rendering: false,
            tempo: Some(120.0),
            time_signature_numerator: Some(4),
            time_signature_denominator: Some(4),
            ppq_position: Some(0.0),
        }
    }

    fn configured_session() -> RecordingSession {
        let mut session =
            RecordingSession::new("persistent".into(), "runtime".into(), "Camera".into());
        session.select_source("camera-1".into()).unwrap();
        session
            .set_project_root(PathBuf::from(r"C:\Projects\Song"))
            .unwrap();
        session.set_armed(true);
        session.mark_prepared().unwrap();
        session
    }

    #[test]
    fn requires_source_and_project_root_before_preparing() {
        let mut session =
            RecordingSession::new("persistent".into(), "runtime".into(), "Camera".into());
        session.set_armed(true);
        assert_eq!(session.status(), SessionStatus::Idle);

        session.select_source("camera-1".into()).unwrap();
        assert_eq!(session.status(), SessionStatus::Idle);

        session
            .set_project_root(PathBuf::from(r"C:\Projects\Song"))
            .unwrap();
        assert_eq!(session.status(), SessionStatus::Preparing);
    }

    #[test]
    fn recording_edges_drive_the_state_machine() {
        let mut session = configured_session();
        assert!(matches!(
            session.handle_transport(snapshot(1, true)),
            Some(RecordingAction::Start(_))
        ));
        assert_eq!(session.status(), SessionStatus::Recording);

        assert!(session.handle_transport(snapshot(2, true)).is_none());
        assert!(matches!(
            session.handle_transport(snapshot(3, false)),
            Some(RecordingAction::Stop(_))
        ));
        assert_eq!(session.status(), SessionStatus::Finalizing);
    }

    #[test]
    fn disarming_does_not_truncate_an_active_take() {
        let mut session = configured_session();
        session.handle_transport(snapshot(1, true));
        session.set_armed(false);
        assert_eq!(session.status(), SessionStatus::Recording);

        session.handle_transport(snapshot(2, false));
        session.mark_finalized().unwrap();
        assert_eq!(session.status(), SessionStatus::Idle);
    }

    #[test]
    fn armed_session_can_record_multiple_consecutive_takes() {
        let mut session = configured_session();
        assert!(matches!(
            session.handle_transport(snapshot(1, true)),
            Some(RecordingAction::Start(_))
        ));
        assert!(matches!(
            session.handle_transport(snapshot(2, false)),
            Some(RecordingAction::Stop(_))
        ));
        session.mark_finalized().unwrap();
        assert_eq!(session.status(), SessionStatus::Preparing);
        session.mark_prepared().unwrap();

        assert!(matches!(
            session.handle_transport(snapshot(3, true)),
            Some(RecordingAction::Start(_))
        ));
        assert_eq!(session.status(), SessionStatus::Recording);
    }

    #[test]
    fn ignores_out_of_order_and_offline_snapshots() {
        let mut session = configured_session();
        let mut offline = snapshot(1, true);
        offline.is_offline_rendering = true;
        assert!(session.handle_transport(offline).is_none());
        assert_eq!(session.status(), SessionStatus::Armed);

        assert!(session.handle_transport(snapshot(2, false)).is_none());
        assert!(session.handle_transport(snapshot(1, true)).is_none());
        assert_eq!(session.status(), SessionStatus::Armed);
    }

    #[test]
    fn validates_encoding_profile_and_sync_offset() {
        let mut session = configured_session();
        let profile = EncodingProfile {
            bit_rate: 16_000_000,
            gop_duration_ms: 1_000,
            prefer_hardware: false,
        };
        session.set_encoding_profile(profile).unwrap();
        session.set_sync_offset_ns(125_000_000).unwrap();
        assert_eq!(session.encoding_profile(), profile);
        assert_eq!(session.sync_offset_ns(), 125_000_000);
        assert!(matches!(
            session.set_sync_offset_ns(6_000_000_000),
            Err(SessionError::SyncOffsetOutOfRange(_))
        ));
    }
}

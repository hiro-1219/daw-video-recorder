// SPDX-License-Identifier: AGPL-3.0-only

use std::{collections::HashMap, path::PathBuf};

use thiserror::Error;
use uuid::Uuid;

use crate::{
    protocol::{TransportSnapshot, VideoSource},
    session::{RecordingAction, RecordingSession, SessionError, SessionStatus},
    take::{FinalizedTake, TakeContext, TakeCoordinator, TakeError},
    video_source::{SourceError, enumerate_video_sources},
};

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("unknown runtime instance: {0}")]
    UnknownInstance(String),
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error("video source is not available: {0}")]
    SourceUnavailable(String),
    #[error(transparent)]
    Take(#[from] TakeError),
}

#[derive(Debug, Default)]
pub struct RecorderService {
    sessions: HashMap<String, RecordingSession>,
    sources: HashMap<String, VideoSource>,
    takes: TakeCoordinator,
}

impl RecorderService {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_instance(
        &mut self,
        persistent_instance_id: String,
        instance_label: String,
    ) -> &RecordingSession {
        let runtime_instance_id = Uuid::new_v4().to_string();
        let session = RecordingSession::new(
            persistent_instance_id,
            runtime_instance_id.clone(),
            instance_label,
        );
        self.sessions.insert(runtime_instance_id.clone(), session);
        self.sessions
            .get(&runtime_instance_id)
            .expect("the session was inserted immediately before lookup")
    }

    pub fn unregister_instance(&mut self, runtime_instance_id: &str) -> bool {
        self.takes.abort(runtime_instance_id);
        self.sessions.remove(runtime_instance_id).is_some()
    }

    #[must_use]
    pub fn sources(&self) -> Vec<VideoSource> {
        let mut sources: Vec<_> = self.sources.values().cloned().collect();
        sources.sort_by(|left, right| left.display_name.cmp(&right.display_name));
        sources
    }

    pub fn refresh_sources(&mut self) -> Result<Vec<VideoSource>, ServiceError> {
        self.sources = enumerate_video_sources()?
            .into_iter()
            .map(|source| (source.source_id.clone(), source))
            .collect();
        Ok(self.sources())
    }

    #[must_use]
    pub fn session(&self, runtime_instance_id: &str) -> Option<&RecordingSession> {
        self.sessions.get(runtime_instance_id)
    }

    pub fn select_source(
        &mut self,
        runtime_instance_id: &str,
        source_id: String,
    ) -> Result<SessionStatus, ServiceError> {
        if !self.sources.contains_key(&source_id) {
            return Err(ServiceError::SourceUnavailable(source_id));
        }
        let session = self.session_mut(runtime_instance_id)?;
        session.select_source(source_id)?;
        Ok(session.status())
    }

    pub fn set_project_root(
        &mut self,
        runtime_instance_id: &str,
        project_root: PathBuf,
    ) -> Result<SessionStatus, ServiceError> {
        let session = self.session_mut(runtime_instance_id)?;
        session.set_project_root(project_root)?;
        Ok(session.status())
    }

    pub fn set_armed(
        &mut self,
        runtime_instance_id: &str,
        armed: bool,
    ) -> Result<SessionStatus, ServiceError> {
        let status = {
            let session = self.session_mut(runtime_instance_id)?;
            session.set_armed(armed);
            session.status()
        };
        if !armed && !self.takes.is_active(runtime_instance_id) {
            self.takes.unprepare(runtime_instance_id);
        }
        Ok(status)
    }

    pub fn prepare_capture(
        &mut self,
        runtime_instance_id: &str,
    ) -> Result<SessionStatus, ServiceError> {
        let context = self.take_context(runtime_instance_id)?;
        self.takes
            .prepare(runtime_instance_id, &context.source_id)?;
        let session = self.session_mut(runtime_instance_id)?;
        session.mark_prepared()?;
        Ok(session.status())
    }

    pub fn handle_transport(
        &mut self,
        snapshot: TransportSnapshot,
    ) -> Result<(SessionStatus, Option<RecordingAction>), ServiceError> {
        let runtime_instance_id = snapshot.runtime_instance_id.clone();
        let session = self.session_mut(&runtime_instance_id)?;
        let action = session.handle_transport(snapshot);
        Ok((session.status(), action))
    }

    pub fn execute_recording_action(
        &mut self,
        runtime_instance_id: &str,
        action: RecordingAction,
    ) -> Result<Option<FinalizedTake>, ServiceError> {
        match action {
            RecordingAction::Start(snapshot) => {
                let context = self.take_context(runtime_instance_id)?;
                if let Err(error) = self.takes.start(context, snapshot) {
                    self.takes.abort(runtime_instance_id);
                    self.session_mut(runtime_instance_id)?.fail();
                    return Err(error.into());
                }
                Ok(None)
            }
            RecordingAction::Stop(snapshot) => {
                let finalized = match self.takes.stop(runtime_instance_id, &snapshot) {
                    Ok(finalized) => finalized,
                    Err(error) => {
                        self.takes.abort(runtime_instance_id);
                        self.session_mut(runtime_instance_id)?.fail();
                        return Err(error.into());
                    }
                };
                let session = self.session_mut(runtime_instance_id)?;
                session.mark_finalized()?;
                if session.status() == SessionStatus::Preparing {
                    session.mark_prepared()?;
                } else {
                    self.takes.unprepare(runtime_instance_id);
                }
                Ok(Some(finalized))
            }
        }
    }

    fn take_context(&self, runtime_instance_id: &str) -> Result<TakeContext, ServiceError> {
        let session = self
            .sessions
            .get(runtime_instance_id)
            .ok_or_else(|| ServiceError::UnknownInstance(runtime_instance_id.to_owned()))?;
        let source_id = session
            .selected_source_id()
            .ok_or(SessionError::NotConfigured)?;
        let source = self
            .sources
            .get(source_id)
            .ok_or_else(|| ServiceError::SourceUnavailable(source_id.to_owned()))?;
        let project_root = session
            .project_root()
            .ok_or(SessionError::NotConfigured)?
            .to_owned();
        Ok(TakeContext {
            persistent_instance_id: session.persistent_instance_id().to_owned(),
            runtime_instance_id: session.runtime_instance_id().to_owned(),
            instance_label: session.instance_label().to_owned(),
            source_id: source_id.to_owned(),
            source_display_name: source.display_name.clone(),
            project_root,
        })
    }

    fn session_mut(
        &mut self,
        runtime_instance_id: &str,
    ) -> Result<&mut RecordingSession, ServiceError> {
        self.sessions
            .get_mut(runtime_instance_id)
            .ok_or_else(|| ServiceError::UnknownInstance(runtime_instance_id.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_source(source_id: &str) -> VideoSource {
        VideoSource {
            source_id: source_id.into(),
            display_name: "Test camera".into(),
            formats: Vec::new(),
            available: true,
        }
    }

    #[test]
    fn duplicate_persistent_ids_receive_distinct_runtime_ids() {
        let mut service = RecorderService::new();
        let first = service
            .register_instance("saved-id".into(), "One".into())
            .runtime_instance_id()
            .to_owned();
        let second = service
            .register_instance("saved-id".into(), "Two".into())
            .runtime_instance_id()
            .to_owned();

        assert_ne!(first, second);
        assert_eq!(
            service.session(&first).unwrap().persistent_instance_id(),
            "saved-id"
        );
        assert_eq!(
            service.session(&second).unwrap().persistent_instance_id(),
            "saved-id"
        );
    }

    #[test]
    fn only_currently_enumerated_sources_can_be_selected() {
        let mut service = RecorderService::new();
        let runtime_id = service
            .register_instance("saved-id".into(), "Camera".into())
            .runtime_instance_id()
            .to_owned();

        assert!(matches!(
            service.select_source(&runtime_id, "missing".into()),
            Err(ServiceError::SourceUnavailable(source_id)) if source_id == "missing"
        ));

        service
            .sources
            .insert("camera-1".into(), test_source("camera-1"));
        service
            .select_source(&runtime_id, "camera-1".into())
            .unwrap();
        assert_eq!(
            service.session(&runtime_id).unwrap().selected_source_id(),
            Some("camera-1")
        );
    }
}

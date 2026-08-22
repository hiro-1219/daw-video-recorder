// SPDX-License-Identifier: AGPL-3.0-only

use std::{collections::HashMap, path::PathBuf};

use thiserror::Error;
use uuid::Uuid;

use crate::{
    protocol::{TransportSnapshot, VideoSource},
    session::{RecordingAction, RecordingSession, SessionError, SessionStatus},
};

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("unknown runtime instance: {0}")]
    UnknownInstance(String),
    #[error(transparent)]
    Session(#[from] SessionError),
}

#[derive(Debug, Default)]
pub struct RecorderService {
    sessions: HashMap<String, RecordingSession>,
    sources: HashMap<String, VideoSource>,
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
        self.sessions.remove(runtime_instance_id).is_some()
    }

    #[must_use]
    pub fn sources(&self) -> Vec<VideoSource> {
        self.sources.values().cloned().collect()
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
        let session = self.session_mut(runtime_instance_id)?;
        session.set_armed(armed);
        Ok(session.status())
    }

    pub fn mark_prepared(
        &mut self,
        runtime_instance_id: &str,
    ) -> Result<SessionStatus, ServiceError> {
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
}

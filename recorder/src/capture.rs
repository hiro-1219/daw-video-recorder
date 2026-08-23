// SPDX-License-Identifier: AGPL-3.0-only

use std::{path::PathBuf, sync::mpsc, thread, time::Duration};

use thiserror::Error;

use crate::encoding::{EncodingBackend, EncodingProfile};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPreviewFrame {
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub frame_data: Vec<u8>,
    pub timestamp_ns: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CaptureStats {
    pub frame_count: u64,
    pub dropped_frame_count: u64,
    pub buffered_frame_count: u64,
    pub first_frame_monotonic_time_ns: Option<i64>,
    pub last_frame_monotonic_time_ns: Option<i64>,
    pub discarded_tail_frame_count: u64,
    pub stop_holdback_exceeded: bool,
    pub encoding_backend: EncodingBackend,
    pub drift_start_frame_monotonic_time_ns: Option<i64>,
    pub drift_end_frame_monotonic_time_ns: Option<i64>,
}

#[derive(Debug, Error)]
pub enum CaptureError {
    #[cfg(windows)]
    #[error("Media Foundation capture failed: {0}")]
    Windows(#[from] windows::core::Error),
    #[error("camera has no supported uncompressed video format")]
    NoSupportedFormat,
    #[error("capture initialization timed out")]
    InitializationTimeout,
    #[error("capture thread terminated before initialization completed")]
    InitializationDisconnected,
    #[error("capture thread panicked")]
    ThreadPanicked,
    #[error("capture command timed out while {0}")]
    CommandTimeout(&'static str),
    #[error("capture thread disconnected while {0}")]
    CommandDisconnected(&'static str),
    #[error("recording has already started on this prepared capture")]
    AlreadyRecording,
    #[error("recording has not started on this prepared capture")]
    NotRecording,
    #[error("captured frame is too large to store in a Media Foundation buffer")]
    FrameTooLarge,
    #[error("no H.264 encoder is available for the requested profile")]
    NoEncoder,
    #[error("encoding profile is not prepared")]
    ProfileNotPrepared,
    #[error("recording writer queue overflowed: {0}")]
    WriterQueueFull(String),
    #[error("recording writer failed: {0}")]
    WriterFailed(String),
    #[cfg(not(windows))]
    #[error("camera capture is currently supported on Windows only")]
    UnsupportedPlatform,
}

enum CaptureCommand {
    PrepareProfile {
        profile: EncodingProfile,
        response: mpsc::SyncSender<Result<(), CaptureError>>,
    },
    ReleaseProfile {
        profile: EncodingProfile,
        response: mpsc::SyncSender<Result<(), CaptureError>>,
    },
    SubscribePreview {
        subscriber_id: String,
        target_width: u32,
        target_height: u32,
        max_fps: u32,
        sender: mpsc::SyncSender<RawPreviewFrame>,
        response: mpsc::SyncSender<Result<(), CaptureError>>,
    },
    UnsubscribePreview {
        subscriber_id: String,
        response: mpsc::SyncSender<Result<(), CaptureError>>,
    },
    Begin {
        recording_id: String,
        output_path: PathBuf,
        logical_start_monotonic_time_ns: i64,
        profile: EncodingProfile,
        response: mpsc::SyncSender<Result<(), CaptureError>>,
    },
    End {
        recording_id: String,
        logical_stop_monotonic_time_ns: i64,
        response: mpsc::SyncSender<Result<CaptureStats, CaptureError>>,
    },
    Abort {
        recording_id: String,
        response: mpsc::SyncSender<Result<(), CaptureError>>,
    },
    Shutdown,
}

#[derive(Debug)]
pub struct CaptureHandle {
    commands: mpsc::Sender<CaptureCommand>,
    thread: Option<thread::JoinHandle<Result<(), CaptureError>>>,
}

impl CaptureHandle {
    pub fn prepare(source_id: String) -> Result<Self, CaptureError> {
        let (command_sender, command_receiver) = mpsc::channel();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let capture_thread = thread::Builder::new()
            .name("DAW Video Recorder Capture".into())
            .spawn(move || platform::run(&source_id, &command_receiver, &ready_sender))
            .map_err(|_| CaptureError::ThreadPanicked)?;

        match ready_receiver.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => Ok(Self {
                commands: command_sender,
                thread: Some(capture_thread),
            }),
            Ok(Err(error)) => {
                let _ = capture_thread.join();
                Err(error)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = command_sender.send(CaptureCommand::Shutdown);
                let _ = capture_thread.join();
                Err(CaptureError::InitializationTimeout)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = capture_thread.join();
                Err(CaptureError::InitializationDisconnected)
            }
        }
    }

    pub fn prepare_profile(&self, profile: EncodingProfile) -> Result<(), CaptureError> {
        let (response_sender, response_receiver) = mpsc::sync_channel(1);
        self.commands
            .send(CaptureCommand::PrepareProfile {
                profile,
                response: response_sender,
            })
            .map_err(|_| CaptureError::CommandDisconnected("preparing encoding profile"))?;
        response_receiver
            .recv_timeout(Duration::from_secs(10))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => {
                    CaptureError::CommandTimeout("preparing encoding profile")
                }
                mpsc::RecvTimeoutError::Disconnected => {
                    CaptureError::CommandDisconnected("preparing encoding profile")
                }
            })?
    }

    pub fn release_profile(&self, profile: EncodingProfile) -> Result<(), CaptureError> {
        let (response_sender, response_receiver) = mpsc::sync_channel(1);
        self.commands
            .send(CaptureCommand::ReleaseProfile {
                profile,
                response: response_sender,
            })
            .map_err(|_| CaptureError::CommandDisconnected("releasing encoding profile"))?;
        response_receiver
            .recv_timeout(Duration::from_secs(10))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => {
                    CaptureError::CommandTimeout("releasing encoding profile")
                }
                mpsc::RecvTimeoutError::Disconnected => {
                    CaptureError::CommandDisconnected("releasing encoding profile")
                }
            })?
    }

    pub fn subscribe_preview(
        &self,
        subscriber_id: String,
        target_width: u32,
        target_height: u32,
        max_fps: u32,
        sender: mpsc::SyncSender<RawPreviewFrame>,
    ) -> Result<(), CaptureError> {
        let (response_sender, response_receiver) = mpsc::sync_channel(1);
        self.commands
            .send(CaptureCommand::SubscribePreview {
                subscriber_id,
                target_width,
                target_height,
                max_fps,
                sender,
                response: response_sender,
            })
            .map_err(|_| CaptureError::CommandDisconnected("subscribing to preview"))?;
        response_receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => {
                    CaptureError::CommandTimeout("subscribing to preview")
                }
                mpsc::RecvTimeoutError::Disconnected => {
                    CaptureError::CommandDisconnected("subscribing to preview")
                }
            })?
    }

    pub fn unsubscribe_preview(&self, subscriber_id: &str) -> Result<(), CaptureError> {
        let (response_sender, response_receiver) = mpsc::sync_channel(1);
        self.commands
            .send(CaptureCommand::UnsubscribePreview {
                subscriber_id: subscriber_id.to_owned(),
                response: response_sender,
            })
            .map_err(|_| CaptureError::CommandDisconnected("unsubscribing from preview"))?;
        response_receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => {
                    CaptureError::CommandTimeout("unsubscribing from preview")
                }
                mpsc::RecvTimeoutError::Disconnected => {
                    CaptureError::CommandDisconnected("unsubscribing from preview")
                }
            })?
    }

    pub fn begin_recording(
        &self,
        recording_id: String,
        output_path: PathBuf,
        logical_start_monotonic_time_ns: i64,
        profile: EncodingProfile,
    ) -> Result<(), CaptureError> {
        let (response_sender, response_receiver) = mpsc::sync_channel(1);
        self.commands
            .send(CaptureCommand::Begin {
                recording_id,
                output_path,
                logical_start_monotonic_time_ns,
                profile,
                response: response_sender,
            })
            .map_err(|_| CaptureError::CommandDisconnected("starting recording"))?;
        response_receiver
            .recv_timeout(Duration::from_secs(10))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => {
                    CaptureError::CommandTimeout("starting recording")
                }
                mpsc::RecvTimeoutError::Disconnected => {
                    CaptureError::CommandDisconnected("starting recording")
                }
            })?
    }

    pub fn end_recording(
        &self,
        recording_id: String,
        logical_stop_monotonic_time_ns: i64,
    ) -> Result<CaptureStats, CaptureError> {
        let (response_sender, response_receiver) = mpsc::sync_channel(1);
        self.commands
            .send(CaptureCommand::End {
                recording_id,
                logical_stop_monotonic_time_ns,
                response: response_sender,
            })
            .map_err(|_| CaptureError::CommandDisconnected("stopping recording"))?;
        response_receiver
            .recv_timeout(Duration::from_secs(15))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => {
                    CaptureError::CommandTimeout("stopping recording")
                }
                mpsc::RecvTimeoutError::Disconnected => {
                    CaptureError::CommandDisconnected("stopping recording")
                }
            })?
    }

    pub fn abort_recording(&self, recording_id: String) -> Result<(), CaptureError> {
        let (response_sender, response_receiver) = mpsc::sync_channel(1);
        self.commands
            .send(CaptureCommand::Abort {
                recording_id,
                response: response_sender,
            })
            .map_err(|_| CaptureError::CommandDisconnected("aborting recording"))?;
        response_receiver
            .recv_timeout(Duration::from_secs(10))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => {
                    CaptureError::CommandTimeout("aborting recording")
                }
                mpsc::RecvTimeoutError::Disconnected => {
                    CaptureError::CommandDisconnected("aborting recording")
                }
            })?
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        let _ = self.commands.send(CaptureCommand::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(windows)]
#[path = "capture_windows.rs"]
pub(crate) mod platform;

#[cfg(not(windows))]
mod platform {
    use super::{CaptureCommand, CaptureError};
    use std::sync::mpsc::{Receiver, SyncSender};

    pub(crate) fn run(
        _source_id: &str,
        _commands: &Receiver<CaptureCommand>,
        ready: &SyncSender<Result<(), CaptureError>>,
    ) -> Result<(), CaptureError> {
        let _ = ready.send(Err(CaptureError::UnsupportedPlatform));
        Err(CaptureError::UnsupportedPlatform)
    }
}

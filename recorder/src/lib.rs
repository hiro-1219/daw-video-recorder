// SPDX-License-Identifier: AGPL-3.0-only

pub mod capture;
pub mod encoding;
pub mod ipc;
pub mod metadata;
pub mod preview;
pub mod protocol;
pub mod service;
pub mod session;
pub mod take;
pub mod video_source;

pub const PROTOCOL_VERSION: u32 = 2;
pub const DEFAULT_PIPE_NAME: &str = r"\\.\pipe\daw-video-recorder-v2";

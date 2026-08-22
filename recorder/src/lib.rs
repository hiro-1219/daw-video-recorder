// SPDX-License-Identifier: AGPL-3.0-only

pub mod ipc;
pub mod metadata;
pub mod protocol;
pub mod service;
pub mod session;

pub const PROTOCOL_VERSION: u32 = 1;
pub const DEFAULT_PIPE_NAME: &str = r"\\.\pipe\daw-video-recorder-v1";

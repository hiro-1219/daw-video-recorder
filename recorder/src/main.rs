// SPDX-License-Identifier: AGPL-3.0-only

use std::{fs::OpenOptions, path::PathBuf, sync::Arc};

use daw_video_recorder_service::{DEFAULT_PIPE_NAME, ipc, service::RecorderService};
use tokio::sync::Mutex;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt::writer::BoxMakeWriter};

fn log_file_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|path| path.join("DAW Video Recorder").join("service.log"))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug"));

    if let Some(log_path) = log_file_path() {
        if let Some(parent) = log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(file) = OpenOptions::new().create(true).append(true).open(&log_path) {
            let file_writer = BoxMakeWriter::new(file);
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(file_writer)
                .with_ansi(false)
                .init();
        } else {
            tracing_subscriber::fmt().with_env_filter(filter).init();
        }
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }

    let service = Arc::new(Mutex::new(RecorderService::new()));
    info!(pipe = DEFAULT_PIPE_NAME, "starting recording service");

    #[cfg(windows)]
    ipc::serve_named_pipe(DEFAULT_PIPE_NAME, service).await?;

    #[cfg(not(windows))]
    return Err("the initial recording service is supported on Windows only".into());

    #[allow(unreachable_code)]
    Ok(())
}

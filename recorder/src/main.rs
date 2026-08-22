// SPDX-License-Identifier: AGPL-3.0-only

use std::sync::Arc;

use daw_video_recorder_service::{DEFAULT_PIPE_NAME, ipc, service::RecorderService};
use tokio::sync::Mutex;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let service = Arc::new(Mutex::new(RecorderService::new()));
    info!(pipe = DEFAULT_PIPE_NAME, "starting recording service");

    #[cfg(windows)]
    ipc::serve_named_pipe(DEFAULT_PIPE_NAME, service).await?;

    #[cfg(not(windows))]
    return Err("the initial recording service is supported on Windows only".into());

    #[allow(unreachable_code)]
    Ok(())
}

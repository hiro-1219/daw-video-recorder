// SPDX-License-Identifier: AGPL-3.0-only

use std::{io, path::PathBuf, sync::Arc};

use prost::Message;
use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::Mutex,
};

use crate::{
    PROTOCOL_VERSION,
    protocol::{self, Envelope, envelope::Payload},
    service::{RecorderService, ServiceError},
    session::SessionStatus,
};

const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Error)]
pub enum IpcError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Decode(#[from] prost::DecodeError),
    #[error(transparent)]
    Encode(#[from] prost::EncodeError),
    #[error("IPC message is too large: {0} bytes")]
    MessageTooLarge(usize),
}

#[cfg(windows)]
pub async fn serve_named_pipe(
    pipe_name: &str,
    service: Arc<Mutex<RecorderService>>,
) -> Result<(), IpcError> {
    use tokio::net::windows::named_pipe::ServerOptions;

    let mut first_instance = true;
    loop {
        let server = ServerOptions::new()
            .first_pipe_instance(first_instance)
            .create(pipe_name)?;
        first_instance = false;
        server.connect().await?;

        let client_service = Arc::clone(&service);
        tokio::spawn(async move {
            if let Err(error) = serve_connection(server, client_service).await {
                tracing::warn!(%error, "named-pipe client disconnected with an error");
            }
        });
    }
}

pub async fn serve_connection<S>(
    mut stream: S,
    service: Arc<Mutex<RecorderService>>,
) -> Result<(), IpcError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut connection_instances = Vec::new();
    let connection_result = async {
        while let Some(request) = read_envelope(&mut stream).await? {
            let responses = handle_envelope(request, &service).await;
            for response in responses {
                if let Some(Payload::InstanceRegistered(registered)) = &response.payload {
                    connection_instances.push(registered.runtime_instance_id.clone());
                }
                write_envelope(&mut stream, &response).await?;
            }
        }
        Ok(())
    }
    .await;

    let mut service = service.lock().await;
    for runtime_instance_id in connection_instances {
        service.unregister_instance(&runtime_instance_id);
    }
    connection_result
}

pub async fn read_envelope<R>(reader: &mut R) -> Result<Option<Envelope>, IpcError>
where
    R: AsyncRead + Unpin,
{
    let mut length_bytes = [0_u8; 4];
    match reader.read_exact(&mut length_bytes).await {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error.into()),
    }

    let length = u32::from_le_bytes(length_bytes) as usize;
    if length > MAX_MESSAGE_BYTES {
        return Err(IpcError::MessageTooLarge(length));
    }

    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload).await?;
    Ok(Some(Envelope::decode(payload.as_slice())?))
}

pub async fn write_envelope<W>(writer: &mut W, envelope: &Envelope) -> Result<(), IpcError>
where
    W: AsyncWrite + Unpin,
{
    let encoded_length = envelope.encoded_len();
    if encoded_length > MAX_MESSAGE_BYTES {
        return Err(IpcError::MessageTooLarge(encoded_length));
    }

    let encoded_length =
        u32::try_from(encoded_length).map_err(|_| IpcError::MessageTooLarge(encoded_length))?;
    writer.write_all(&encoded_length.to_le_bytes()).await?;

    let mut payload = Vec::with_capacity(encoded_length as usize);
    envelope.encode(&mut payload)?;
    writer.write_all(&payload).await?;
    writer.flush().await?;
    Ok(())
}

async fn handle_envelope(
    request: Envelope,
    service: &Arc<Mutex<RecorderService>>,
) -> Vec<Envelope> {
    let request_id = request.request_id;
    let Some(payload) = request.payload else {
        return vec![error_envelope(
            request_id,
            "empty_payload",
            "request has no payload",
            "",
        )];
    };

    match payload {
        Payload::Hello(hello) => {
            if hello.minimum_version <= PROTOCOL_VERSION
                && hello.maximum_version >= PROTOCOL_VERSION
            {
                vec![response(
                    request_id,
                    Payload::HelloAck(protocol::HelloAck {
                        selected_version: PROTOCOL_VERSION,
                        service_version: env!("CARGO_PKG_VERSION").to_owned(),
                    }),
                )]
            } else {
                vec![error_envelope(
                    request_id,
                    "protocol_version_mismatch",
                    "client and service have no common protocol version",
                    "",
                )]
            }
        }
        Payload::RegisterInstance(register) => {
            let mut service = service.lock().await;
            let session = service.register_instance(
                register.persistent_instance_id.clone(),
                register.instance_label,
            );
            vec![response(
                request_id,
                Payload::InstanceRegistered(protocol::InstanceRegistered {
                    persistent_instance_id: register.persistent_instance_id,
                    runtime_instance_id: session.runtime_instance_id().to_owned(),
                }),
            )]
        }
        Payload::UnregisterInstance(unregister) => {
            let mut service = service.lock().await;
            if service.unregister_instance(&unregister.runtime_instance_id) {
                Vec::new()
            } else {
                vec![error_envelope(
                    request_id,
                    "unknown_instance",
                    "runtime instance is not registered",
                    &unregister.runtime_instance_id,
                )]
            }
        }
        Payload::ListSources(_) => {
            let service = service.lock().await;
            vec![response(
                request_id,
                Payload::SourceList(protocol::SourceList {
                    sources: service.sources(),
                }),
            )]
        }
        Payload::SelectSource(select) => {
            let runtime_id = select.runtime_instance_id;
            let result = {
                let mut service = service.lock().await;
                service.select_source(&runtime_id, select.source_id)
            };
            state_or_error(request_id, &runtime_id, result)
        }
        Payload::SetProjectRoot(set_root) => {
            let runtime_id = set_root.runtime_instance_id;
            let result = {
                let mut service = service.lock().await;
                service.set_project_root(&runtime_id, PathBuf::from(set_root.project_root))
            };
            state_or_error(request_id, &runtime_id, result)
        }
        Payload::SetArmed(set_armed) => {
            let runtime_id = set_armed.runtime_instance_id;
            let result = {
                let mut service = service.lock().await;
                match service.set_armed(&runtime_id, set_armed.armed) {
                    // Until the capture backend is connected, preparation has no asynchronous work.
                    Ok(SessionStatus::Preparing) => service.mark_prepared(&runtime_id),
                    other => other,
                }
            };
            state_or_error(request_id, &runtime_id, result)
        }
        Payload::TransportSnapshotBatch(batch) => {
            let mut responses = Vec::new();
            let mut service = service.lock().await;
            let mut accepted_runtime_id = String::new();
            let mut accepted_sequence = 0;
            for snapshot in batch.snapshots {
                let runtime_id = snapshot.runtime_instance_id.clone();
                accepted_runtime_id.clone_from(&runtime_id);
                accepted_sequence = snapshot.sequence_number;
                match service.handle_transport(snapshot) {
                    Ok((status, Some(_))) => {
                        responses.push(state_response(request_id, &runtime_id, status));
                    }
                    Ok((_, None)) => {}
                    Err(error) => responses.push(service_error(request_id, &runtime_id, &error)),
                }
            }
            responses.push(response(
                request_id,
                Payload::TransportAccepted(protocol::TransportAccepted {
                    runtime_instance_id: accepted_runtime_id,
                    last_sequence_number: accepted_sequence,
                }),
            ));
            responses
        }
        Payload::Heartbeat(heartbeat) => vec![response(
            request_id,
            Payload::HeartbeatAck(protocol::HeartbeatAck {
                sequence_number: heartbeat.sequence_number,
            }),
        )],
        _ => vec![error_envelope(
            request_id,
            "unexpected_response",
            "the service received a response payload from a client",
            "",
        )],
    }
}

fn state_or_error(
    request_id: u64,
    runtime_id: &str,
    result: Result<SessionStatus, ServiceError>,
) -> Vec<Envelope> {
    match result {
        Ok(status) => vec![state_response(request_id, runtime_id, status)],
        Err(error) => vec![service_error(request_id, runtime_id, &error)],
    }
}

fn state_response(request_id: u64, runtime_id: &str, status: SessionStatus) -> Envelope {
    response(
        request_id,
        Payload::SessionStateChanged(protocol::SessionStateChanged {
            runtime_instance_id: runtime_id.to_owned(),
            state: protocol_state(status) as i32,
            detail: String::new(),
        }),
    )
}

const fn protocol_state(status: SessionStatus) -> protocol::SessionState {
    match status {
        SessionStatus::Idle => protocol::SessionState::Idle,
        SessionStatus::Preparing => protocol::SessionState::Preparing,
        SessionStatus::Armed => protocol::SessionState::Armed,
        SessionStatus::Recording => protocol::SessionState::Recording,
        SessionStatus::Finalizing => protocol::SessionState::Finalizing,
        SessionStatus::Error => protocol::SessionState::Error,
    }
}

fn service_error(request_id: u64, runtime_id: &str, error: &ServiceError) -> Envelope {
    let code = match error {
        ServiceError::UnknownInstance(_) => "unknown_instance",
        ServiceError::Session(_) => "invalid_session_operation",
    };
    error_envelope(request_id, code, &error.to_string(), runtime_id)
}

fn error_envelope(request_id: u64, code: &str, message: &str, runtime_id: &str) -> Envelope {
    response(
        request_id,
        Payload::Error(protocol::Error {
            code: code.to_owned(),
            message: message.to_owned(),
            runtime_instance_id: runtime_id.to_owned(),
            source_id: String::new(),
            recoverable: true,
        }),
    )
}

fn response(request_id: u64, payload: Payload) -> Envelope {
    Envelope {
        protocol_version: PROTOCOL_VERSION,
        request_id,
        payload: Some(payload),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn envelope_round_trips_through_length_delimited_framing() {
        let (mut client, mut server) = tokio::io::duplex(4_096);
        let expected = Envelope {
            protocol_version: 1,
            request_id: 42,
            payload: Some(Payload::Heartbeat(protocol::Heartbeat {
                sequence_number: 7,
            })),
        };

        let write = write_envelope(&mut client, &expected);
        let read = read_envelope(&mut server);
        let (write_result, read_result) = tokio::join!(write, read);

        write_result.unwrap();
        assert_eq!(read_result.unwrap(), Some(expected));
    }

    #[tokio::test]
    async fn hello_negotiates_protocol_version_one() {
        let service = Arc::new(Mutex::new(RecorderService::new()));
        let request = Envelope {
            protocol_version: 1,
            request_id: 9,
            payload: Some(Payload::Hello(protocol::Hello {
                minimum_version: 1,
                maximum_version: 1,
                client_name: "test".into(),
            })),
        };

        let responses = handle_envelope(request, &service).await;
        assert!(matches!(
            responses[0].payload,
            Some(Payload::HelloAck(protocol::HelloAck {
                selected_version: 1,
                ..
            }))
        ));
    }

    #[tokio::test]
    async fn transport_batch_is_always_acknowledged() {
        let service = Arc::new(Mutex::new(RecorderService::new()));
        let runtime_id = {
            let mut locked = service.lock().await;
            locked
                .register_instance("persistent".into(), "test".into())
                .runtime_instance_id()
                .to_owned()
        };
        let request = Envelope {
            protocol_version: 1,
            request_id: 10,
            payload: Some(Payload::TransportSnapshotBatch(
                protocol::TransportSnapshotBatch {
                    snapshots: vec![protocol::TransportSnapshot {
                        runtime_instance_id: runtime_id.clone(),
                        sequence_number: 99,
                        callback_monotonic_time_ns: 1,
                        host_time_ns: None,
                        project_sample_position: Some(512),
                        sample_rate: 48_000.0,
                        is_playing: true,
                        is_recording: false,
                        is_looping: false,
                        is_offline_rendering: false,
                        tempo: Some(120.0),
                        time_signature_numerator: Some(4),
                        time_signature_denominator: Some(4),
                        ppq_position: Some(0.0),
                    }],
                },
            )),
        };

        let responses = handle_envelope(request, &service).await;
        assert!(responses.iter().any(|response| matches!(
            &response.payload,
            Some(Payload::TransportAccepted(protocol::TransportAccepted {
                runtime_instance_id: accepted_runtime_id,
                last_sequence_number: 99,
            })) if accepted_runtime_id == &runtime_id
        )));
    }
}

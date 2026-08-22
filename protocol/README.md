# IPC protocol

`daw_video_recorder.proto` is the canonical IPC contract between the VST3 plug-in and the Rust recording service.

Messages are framed as:

1. four-byte unsigned little-endian payload length;
2. one serialized `Envelope` payload.

The initial Windows transport is a per-user Named Pipe. Message payloads are limited to 1 MiB. Preview pixels will use a separate shared-memory transport; they must not be embedded in control messages.


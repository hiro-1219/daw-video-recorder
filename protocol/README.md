# IPC protocol

`daw_video_recorder.proto` is the canonical IPC contract between the VST3 plug-in and the Rust recording service.

For the implemented lifecycle, threading model, preview byte layout, backpressure behavior, and GUI-facing facade, see [`docs/IPC_API_HANDOFF.md`](../docs/IPC_API_HANDOFF.md).

Messages are framed as:

1. four-byte unsigned little-endian payload length;
2. one serialized `Envelope` payload.

The implemented Windows transport is the local Named Pipe `\\.\pipe\daw-video-recorder-v2`. Encoded envelopes are limited to 4 MiB on both the Rust and C++ sides.

Preview pixels currently travel inline as `PreviewFrame.frame_data` on the same framed Named Pipe connection as control messages. Rust currently emits downsampled `rgb24` frames; successful preview start/stop requests have no direct acknowledgement, and preview frames are unsolicited envelopes with `request_id = 0`.

Windows named shared memory is only a possible future optimization. It is not part of the current protocol or data path.

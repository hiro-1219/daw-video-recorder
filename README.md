# DAW Video Recorder

DAW Video Recorder is an AGPL-licensed VST3 and companion recording service for recording camera video in sync with a DAW transport. The initial compatibility target is Studio One on Windows.

The repository currently contains the first implementation foundation:

- `plugin/`: C++/JUCE VST3 transport observer and real-time-safe queue
- `recorder/`: Rust recording-service state machine and local IPC server
- `protocol/`: versioned Protocol Buffers IPC contract
- `SPECIFICATION.md`: requirements and architecture

## Prerequisites

- Rust stable
- CMake 3.25 or later
- Visual Studio 2022 with Desktop development with C++
- A JUCE checkout supplied through `DAWVR_JUCE_DIR` or `third_party/JUCE`

The Rust service currently contains prost-compatible declarations matching the canonical `.proto` file, so a system `protoc` installation is not required for the first milestone. Automated schema code generation will replace these declarations when the C++ IPC client is connected.

## Build the recording service

```powershell
cargo test --workspace
cargo build --release -p daw-video-recorder-service
```

## Configure the VST3 build

```powershell
cmake -S . -B build -DDAWVR_JUCE_DIR=C:/path/to/JUCE
cmake --build build --config Release
```

The generated VST3 bundle is placed below `build/plugin/DAWVideoRecorderPlugin_artefacts/<Config>/VST3/`.

## Current implementation status

The VST3 captures host transport snapshots into a real-time-safe SPSC queue. Its background worker starts and connects to the Rust service, negotiates protocol v1, enumerates Windows cameras, synchronizes the selected camera, Project Root, and Record Arm, and sends acknowledged transport batches. Record Arm prepares the selected Media Foundation camera and keeps a bounded, timestamped frame ring. DAW recording edges then start and stop per-take H.264 MP4 writers and synchronization JSON below `<Project Root>/Recordings` without reopening the camera between consecutive takes. Frames captured while the start event is in transit over IPC are recovered from the ring using the event's original monotonic timestamp. During recording, a bounded two-second output holdback lets the service discard frames later than the original DAW stop timestamp.

The current capture path is the first single-session vertical slice. Camera sensor-latency calibration, shared pipelines for multiple instances using the same camera, interrupted MP4 recovery, preview, and Studio One timing validation remain in progress.

During development, start the plug-in host from the repository root or set `DAWVR_SERVICE_PATH` to the full path of `daw-video-recorder-service.exe`. Installed builds look for the service at `%LOCALAPPDATA%\DAW Video Recorder\daw-video-recorder-service.exe`.

## License

This project is licensed under GNU Affero General Public License v3.0 only. See `LICENSE` and `THIRD_PARTY_NOTICES.md`.

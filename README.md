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

The VST3 currently captures host transport snapshots into a real-time-safe SPSC queue. Its background worker drains the queue, but Protocol Buffers serialization and the Named Pipe connection are the next milestone. The Rust service already accepts length-delimited Protocol Buffers over a Windows Named Pipe and implements instance/session state transitions; camera capture and encoding are not connected yet.

## License

This project is licensed under GNU Affero General Public License v3.0 only. See `LICENSE` and `THIRD_PARTY_NOTICES.md`.

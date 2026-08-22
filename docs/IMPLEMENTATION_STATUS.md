# Implementation status

## Milestone 1 — foundation

Implemented:

- JUCE 9.0.0 VST3 that passes audio through unchanged
- Record Arm parameter and persistent plug-in instance ID
- manual Studio One Project Root selection and persistence
- block-level transport snapshot capture
- allocation-free SPSC handoff from the audio thread
- non-real-time transport worker
- versioned Protocol Buffers contract
- C++ Protocol Buffers-compatible control-message codec
- C++ Windows Named Pipe client with retry and status reporting
- Hello/version negotiation and per-instance registration
- Project Root, Record Arm, and acknowledged Transport batch delivery
- Rust Windows Named Pipe server and length-delimited framing
- multiple runtime sessions for duplicated persistent plug-in IDs
- recording state machine and offline-render suppression
- atomic JSON synchronization-metadata writer

Not yet implemented:

- recording-service automatic launch and lifetime management
- Windows camera enumeration and Media Foundation capture
- preview shared memory
- encoder/muxer and recoverable media container
- `Recordings` directory creation from the selected Project Root
- finalized take notifications and Studio One integration tests

## Verified commands

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cmake -S . -B build -G "Visual Studio 17 2022" -A x64
cmake --build build --config Debug --target DAWVideoRecorderPlugin_VST3 -j 4
cmake --build build --config Debug --target DAWVideoRecorderIpcSmoke -j 4
```

The IPC smoke test launches the Rust service and verifies a real C++/Rust round trip through the Windows Named Pipe, including protocol negotiation, registration, configuration, transport delivery, and acknowledgement.

This environment supplied duplicate `Path` and `PATH` variables. They were normalized in the CMake process before invoking MSBuild; this is an execution-environment workaround, not a project requirement.

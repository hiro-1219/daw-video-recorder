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
- recording-service automatic launch with environment, installed, and development paths
- Hello/version negotiation and per-instance registration
- Project Root, Record Arm, and acknowledged Transport batch delivery
- Rust Windows Named Pipe server and length-delimited framing
- multiple runtime sessions for duplicated persistent plug-in IDs
- recording state machine and offline-render suppression
- atomic JSON synchronization-metadata writer
- Windows Media Foundation camera enumeration with stable symbolic-link IDs
- live camera list, selection, and selected-camera state persistence in the VST3 UI
- DAW recording-edge connection to a per-session Media Foundation capture pipeline
- native uncompressed camera input encoded as H.264 in an MP4 container
- collision-resistant take directories below `<Project Root>/Recordings`
- atomic synchronization metadata with DAW sample positions and captured-frame counts
- finalized-take path notification and display in the VST3 UI
- Record Arm-time camera preparation with continuous frame warm-up
- immediate per-take writer switching without reopening the camera between consecutive takes
- prepared-camera release on disarm, instance removal, and capture failure
- bounded two-second raw-frame ring buffer while Record Arm is active
- frame timestamps correlated to the Windows performance-counter clock
- DAW start-edge recovery from the ring buffer using the original audio-callback timestamp
- first-frame synchronization offset and recovered-frame count in take metadata
- bounded two-second output holdback during recording
- logical DAW stop-edge trimming before MP4 finalization
- last-frame synchronization offset and discarded-tail count in take metadata

Camera enumeration initializes the complete Media Foundation platform (`MFSTARTUP_FULL`). The
reduced `MFSTARTUP_LITE` mode returned an empty list for valid UVC devices during Windows testing.

Not yet implemented:

- recording-service idle shutdown and full lifetime management
- camera sensor-latency calibration and long-duration clock-drift measurement
- configurable holdback duration and handling for IPC stalls longer than the holdback window
- shared capture/encoder pipelines for sessions using the same camera
- encoding-profile selection and format enumeration in the GUI
- preview shared memory
- fragmented/recoverable container and interrupted-take recovery
- Studio One integration and long-duration synchronization tests

## Verified commands

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cmake -S . -B build -G "Visual Studio 17 2022" -A x64
cmake --build build --config Debug --target DAWVideoRecorderPlugin_VST3 -j 4
cmake --build build --config Debug --target DAWVideoRecorderIpcSmoke -j 4
```

The IPC smoke test lets the C++ client automatically launch the Rust service and verifies a real C++/Rust round trip through the Windows Named Pipe, including protocol negotiation, registration, camera enumeration, configuration, transport delivery, and acknowledgement. When a camera is available, it also records and finalizes two consecutive takes through one prepared capture pipeline, verifies distinct MP4 and metadata paths, validates start and end synchronization offsets, requires at least one frame to be recovered from the Arm-time ring buffer, and verifies that an IPC-latency tail frame is discarded across the two takes.

This environment supplied duplicate `Path` and `PATH` variables. They were normalized in the CMake process before invoking MSBuild; this is an execution-environment workaround, not a project requirement.

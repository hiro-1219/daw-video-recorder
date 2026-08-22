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
- bounded encoded-GOP ring buffer while Record Arm is active
- frame timestamps correlated to the Windows performance-counter clock
- DAW start-edge recovery from the encoded GOP ring using the original audio-callback timestamp
- first-frame synchronization offset and recovered-frame count in take metadata
- bounded two-second output holdback during recording
- logical DAW stop-edge trimming before MP4 finalization
- last-frame synchronization offset and discarded-tail count in take metadata
- source-ID keyed capture registry shared by multiple VST runtime sessions
- reference-counted camera preparation and release across sessions
- one encoded H.264 packet shared by reference across independent per-session muxers
- writer-error isolation so one failed output does not terminate the shared camera thread
- independent Capture Pipelines for different stable camera source IDs
- profile-keyed shared H.264 Media Foundation transforms for sessions using the same camera
- separate encoders when bitrate, GOP duration, or hardware preference differs
- hardware H.264 MFT preference with automatic software MFT fallback
- encoded GOP ring recovery and keyframe-safe mid-session take starts
- bounded asynchronous MP4 muxer queues that never block the camera capture thread
- independent MP4 finalization threads and queue-backpressure error isolation
- protocol v2 encoding-profile and synchronization-offset configuration
- VST3 bitrate presets, hardware preference, and -5000 to +5000 ms synchronization offset controls
- metadata schema v2 with selected encoding backend, configured offset, and clock-drift measurements
- deterministic 30-minute drift-calculation coverage
- physical-camera smoke tests for shared hardware/software profiles and two simultaneous cameras

Camera enumeration initializes the complete Media Foundation platform (`MFSTARTUP_FULL`). The
reduced `MFSTARTUP_LITE` mode returned an empty list for valid UVC devices during Windows testing.

Not yet implemented:

- recording-service idle shutdown and full lifetime management
- automatic camera sensor-latency calibration
- configurable holdback duration and handling for IPC stalls longer than the holdback window
- resolution and frame-rate enumeration in the GUI
- preview shared memory
- fragmented/recoverable container and interrupted-take recovery
- real-time 30-minute Studio One synchronization acceptance test

## Verified commands

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cmake -S . -B build -G "Visual Studio 17 2022" -A x64
cmake --build build --config Debug --target DAWVideoRecorderPlugin_VST3 -j 4
cmake --build build --config Debug --target DAWVideoRecorderIpcSmoke -j 4
cargo test --test capture_smoke -- --ignored --nocapture
cargo test --test multi_camera_smoke -- --ignored --nocapture
```

The IPC smoke test lets two C++ clients automatically launch and connect to the Rust service. It verifies protocol v2 profile/offset configuration, metadata schema v2, shared hardware H.264 encoding, the same first and last frame timestamps in independent MP4 files, a second take while another client remains attached, encoded-GOP recovery, and tail trimming. The ignored physical-camera tests exercise hardware/software profile separation and two independent camera pipelines; they are run explicitly on a suitable Windows host.

This environment supplied duplicate `Path` and `PATH` variables. They were normalized in the CMake process before invoking MSBuild; this is an execution-environment workaround, not a project requirement.

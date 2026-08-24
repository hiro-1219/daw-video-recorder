# DAW playback visibility in Auto view

- Status: `integrated`
- Direction: `Codex -> Antigravity`
- Date: `2026-08-24`
- Owner: `Codex`

## User-visible goal

Show the selected recorded MP4 in the VST GUI during DAW playback, including when Record Arm remains enabled after recording.

## Contract or facade delta

No wire-schema, protocol-version, or `PluginProcessor` facade change. `VideoPlaybackPolicy` is the shared GUI-internal source-selection and DAW-sample mapping contract. Metadata `sync_offset_ns` is subtracted because it is first-frame time minus corrected DAW-start time. `MFVideoDecoder` normalizes stored `\\?\` paths only for the Media Foundation source-reader call.

## Threading and lifetime

The editor timer resolves one presentation source per tick and submits a non-blocking time request. A low-priority `RecordedPlaybackWorker` owns Media Foundation file access, coalesces to the newest request at a maximum of 15 fps, reads sequentially during normal forward playback, and publishes copied image snapshots. The message and audio threads never wait for recorded decoding. Decoder failures are retained for the selected path rather than retried continuously.

## GUI impact

Auto selects recorded playback while the DAW is playing and a take exists, regardless of Arm. Active recording selects live; stopped and armed selects live; stopped and unarmed with a take selects recorded. Recorded Take and Live Camera remain explicit overrides. Missing take, unavailable DAW position, range, and decoder failures have distinct placeholders.

## Changed files

- `plugin/src/VideoPlaybackPolicy.h`
- `plugin/src/RecordedPlaybackWorker.*`
- `plugin/src/PluginEditor.*`
- `plugin/src/VideoPreviewComponent.*`
- `plugin/src/TakeSyncManager.*`
- `tools/ipc-smoke/*`
- `docs/*`

## Validation

- `cargo fmt --all -- --check`: passed.
- `cargo test --workspace`: 26 passed; physical-camera Rust tests remained ignored by default.
- Debug and Release VST3/IPC smoke builds: passed.
- `DAWVideoRecorderIpcSmoke.exe --playback-logic-only`: passed in Debug and Release.
- `DAWVideoRecorderIpcSmoke.exe --response-routing-only`: passed in Release.
- Full Release physical-camera IPC smoke: passed with two detected cameras, three finalized MP4 files, signed-offset mapping, and Media Foundation frame decode.
- Optimized Debug physical-camera IPC smoke: passed with two cameras, three finalized MP4 files, and background initial/seek/sequential frame decode at 640x360.
- Optimized Release physical-camera IPC smoke: passed after a clean service restart with the same asynchronous playback checks.
- `--playback-worker-file` persisted-MP4 test: passed in Debug and Release at 640x360.
- Optimized Debug and Release VST3/IPC smoke builds: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: blocked by pre-existing Rust lints in `capture_windows.rs`, `ipc.rs`, and `preview.rs`.
- Studio One Auto + Arm playback: passed user verification on 2026-08-24.
- Optimized background playback performance: passed user verification in Studio One on 2026-08-24.

## Open items

- none

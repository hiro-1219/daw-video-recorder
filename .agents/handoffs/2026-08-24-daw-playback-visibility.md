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

The editor timer resolves one presentation source per tick. That result controls synchronous recorded-frame decoding on the message thread and the existing background live-preview subscription. Decoder failures are retained for the selected path rather than retried at 30 Hz.

## GUI impact

Auto selects recorded playback while the DAW is playing and a take exists, regardless of Arm. Active recording selects live; stopped and armed selects live; stopped and unarmed with a take selects recorded. Recorded Take and Live Camera remain explicit overrides. Missing take, unavailable DAW position, range, and decoder failures have distinct placeholders.

## Changed files

- `plugin/src/VideoPlaybackPolicy.h`
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
- `cargo clippy --workspace --all-targets -- -D warnings`: blocked by pre-existing Rust lints in `capture_windows.rs`, `ipc.rs`, and `preview.rs`.
- Studio One Auto + Arm playback: passed user verification on 2026-08-24.

## Open items

- Recorded playback currently seeks and decodes synchronously on the GUI timer; move this work to a bounded background decoder before treating playback performance as complete.

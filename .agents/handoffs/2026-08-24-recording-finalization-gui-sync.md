# Recording finalization and GUI take sync

- Status: `integrated`
- Direction: `Codex -> Antigravity`
- Date: `2026-08-24`
- Owner: `Codex`

## User-visible goal

Keep the recording service connected through DAW record edges so finalized MP4 files appear in the new synchronized-take GUI.

## Contract or facade delta

No wire-schema or protocol-version change. `ServiceClient` now correlates a pending request only with its expected response kind or `Error`. New metadata uses `<stem>.sync.json`; the GUI also reads legacy `<stem>.mp4.json` files. `TakeSyncManager` returns copied take-list and selection snapshots instead of unlocked references.

## Threading and lifetime

The receiver thread dispatches preview and finalized-take events before request correlation and waits for `TransportAccepted` after transport-side state events. Recorded-file decoding runs only while a recorded view is visible. Preview subscription follows the editor view mode without stopping the service client or transport worker.

## GUI impact

The existing Auto, Recorded Take, and Live Camera modes remain. Auto uses live preview while armed/recording and recorded playback otherwise. Finalized and legacy takes populate the selector without treating `.part` files as complete.

## Changed files

- `plugin/src/ServiceClient.*`
- `plugin/src/TakeSyncManager.*`
- `plugin/src/PluginEditor.*`
- `recorder/src/take.rs`

## Validation

- `cargo fmt --all -- --check`: passed.
- `cargo test --workspace`: 26 tests passed; physical-camera tests remained ignored by default.
- `cmake --build build --config Debug --target DAWVideoRecorderPlugin_VST3 -j 4`: passed.
- `cmake --build build --config Debug --target DAWVideoRecorderIpcSmoke -j 4`: passed.
- `DAWVideoRecorderIpcSmoke.exe --response-routing-only`: passed.
- Release service and VST3 installation hashes matched their build artifacts.
- Full Release physical-camera IPC smoke with preview enabled: passed with two detected cameras, concurrent clients, two consecutive primary takes, three finalized non-empty MP4 files, `.sync.json` metadata, buffered GOP frames, discarded tail frames, and no recording-edge reconnect.
- `cargo clippy --workspace --all-targets -- -D warnings`: blocked by pre-existing lints in `capture_windows.rs`, `preview.rs`, and an IPC test literal outside this change.

## Open items

- Existing `.mp4.part` files are intentionally left unchanged.

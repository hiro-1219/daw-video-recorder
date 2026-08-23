# DAW Video Recorder agent instructions

## Read first

- Read `docs/IPC_API_HANDOFF.md` before changing the recording service, IPC protocol, DAW transport integration, camera handling, or the GUI-facing service API.
- Read `.agents/handoffs/` for an open cross-tool request before starting work that crosses the core/GUI boundary.
- Treat `protocol/daw_video_recorder.proto` as the canonical wire schema. The Rust definitions in `recorder/src/protocol.rs` and the C++ implementation in `plugin/src/ProtocolCodec.*` are maintained mirrors.

## Ownership

Codex owns the recording and integration core:

- `recorder/**`
- `protocol/**`
- `tools/ipc-smoke/**`
- `plugin/src/PluginProcessor.*`
- `plugin/src/ServiceClient.*`
- `plugin/src/ProtocolCodec.*`
- `plugin/src/SpscQueue.h`
- `plugin/src/TransportSnapshot.h`

Antigravity owns the VST presentation layer:

- `plugin/src/PluginEditor.*`
- `plugin/src/VideoPreviewComponent.*`
- future presentation-only JUCE components, styling, and visual assets

`plugin/CMakeLists.txt` is shared. Antigravity may register presentation-only source files. Changes to dependencies, formats, binary settings, protocol code, or core build behavior remain Codex-owned.

Do not make visual-design changes in Antigravity-owned files unless the user explicitly asks Codex to do so. If a core change affects the GUI-facing API, update the API reference and leave a handoff record using `.agents/handoffs/TEMPLATE.md`.

## Interface rules

- The GUI consumes `PluginProcessor` and its exposed value types. It must not open the Named Pipe, encode or decode Protocol Buffers, enumerate cameras, or call Media Foundation directly.
- Keep blocking IPC, camera access, file I/O, encoding, allocation-heavy work, and locks out of the DAW audio callback.
- Preserve bounded queues and failure isolation. A slow preview consumer must not block camera capture or recording.
- Never reuse a Protocol Buffers field number. For an IPC change, update the `.proto`, Rust mirror, C++ codec, protocol/API documentation, and relevant tests together. Make protocol-version compatibility an explicit decision.

## Verification

For core changes, run the checks relevant to the touched code:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cmake --build build --config Debug --target DAWVideoRecorderPlugin_VST3 -j 4
cmake --build build --config Debug --target DAWVideoRecorderIpcSmoke -j 4
```

Physical-camera smoke tests are ignored by default and require a suitable Windows host and camera hardware.

# VST GUI integration context

Apply this rule when working on `plugin/src/PluginEditor.*`, `plugin/src/VideoPreviewComponent.*`, or another presentation-only JUCE component.

Read @/docs/IPC_API_HANDOFF.md and @/plugin/src/PluginProcessor.h before editing.

- Poll immutable/copyable snapshots through `PluginProcessor`; do not retain references into service-owned state.
- Treat preview frames as revisioned snapshots. Skip unchanged revisions and validate width, height, format, and byte length before rendering.
- Currently emitted preview pixels are row-major RGB24. JPEG decoding exists on the GUI side but is not currently emitted by Rust.
- Display connection, empty-source, no-frame, and recoverable-error states without blocking the JUCE message thread.
- Opening, closing, hiding, or toggling preview should be expressed through `PluginProcessor::setPreviewEnabled`. Do not send `RequestPreview` or `StopPreview` directly.
- Do not perform file I/O, IPC, camera access, or expensive pixel conversion in paint callbacks.
- If a required `PluginProcessor` method or snapshot field is missing, create a handoff instead of editing Codex-owned files.

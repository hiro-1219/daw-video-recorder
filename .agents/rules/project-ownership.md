# Project ownership boundary

Apply this rule to all work in this workspace.

Read @/docs/IPC_API_HANDOFF.md before changing the VST GUI or requesting data from the recording service. Also read @/.agents/handoffs/README.md and inspect that directory for an open request relevant to the task.

Antigravity owns:

- `plugin/src/PluginEditor.*`
- `plugin/src/VideoPreviewComponent.*`
- presentation-only JUCE components, styling, layout, and visual assets
- presentation-source registration in `plugin/CMakeLists.txt`

Codex owns:

- `recorder/**`, including Windows camera enumeration, Media Foundation capture, preview conversion, encoding, recording, metadata, and service lifetime
- `protocol/**` and all IPC compatibility decisions
- `plugin/src/PluginProcessor.*`, `ServiceClient.*`, `ProtocolCodec.*`, `SpscQueue.h`, and `TransportSnapshot.h`
- core build dependencies and IPC smoke tools

Use only the public `PluginProcessor` facade from presentation code. Do not open the Named Pipe, parse Protocol Buffers, enumerate devices, or call Media Foundation from GUI code.

If the GUI needs a new core capability, use the `/handoff-api-change` workflow and stop at the ownership boundary. Do not implement a temporary duplicate in the presentation layer. After Codex returns a change, consume the documented facade and keep protocol details out of the GUI.

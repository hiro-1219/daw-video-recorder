# VST plug-in boundary

The root `AGENTS.md` applies here. Within `plugin/`, keep the boundary below explicit.

## Codex-owned core

- `PluginProcessor.*`: DAW audio-thread integration, persisted state, and the GUI-facing facade
- `ServiceClient.*`: process launch, Named Pipe transport, request correlation, background receive, and thread-safe snapshots for the GUI
- `ProtocolCodec.*`: Protocol Buffers-compatible wire codec
- `SpscQueue.h` and `TransportSnapshot.h`: real-time-safe DAW transport handoff

## Antigravity-owned presentation

- `PluginEditor.*`
- `VideoPreviewComponent.*`
- presentation-only components and visual assets added later

Presentation code may call the public `PluginProcessor` facade and copy its snapshot values. It must not depend on raw `protocol::Response`, the Named Pipe, Media Foundation, or recorder internals.

When the presentation layer needs a new capability, record the requested behavior and proposed facade shape with `.agents/handoffs/TEMPLATE.md`; do not bypass the facade. When Codex changes that facade, update `docs/IPC_API_HANDOFF.md` and create or update the corresponding handoff before Antigravity resumes GUI work.

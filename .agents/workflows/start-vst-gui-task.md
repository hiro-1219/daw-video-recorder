---
description: Start or resume a VST presentation-layer task without crossing the Codex-owned core boundary.
---

# Start VST GUI task

1. Read `.agents/rules/project-ownership.md`, `.agents/rules/vst-gui-context.md`, `docs/IPC_API_HANDOFF.md`, and `plugin/src/PluginProcessor.h`.
2. Inspect `git status` and the dated records in `.agents/handoffs/`. Do not overwrite unrelated user changes.
3. State the intended GUI behavior and list the presentation-owned files that will change.
4. Confirm that the existing `PluginProcessor` facade supplies every required input and action.
5. If the facade is insufficient, run `/handoff-api-change`; do not edit core-owned files.
6. Implement only the presentation layer. `plugin/CMakeLists.txt` may be changed only to register presentation-only sources.
7. Build `DAWVideoRecorderPlugin_VST3`, inspect errors for accidental boundary violations, and summarize the GUI behavior and validation performed.

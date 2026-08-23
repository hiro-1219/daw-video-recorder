# Agent workspace configuration

This directory contains Antigravity workspace customizations and cross-tool handoff records.

- `rules/`: persistent Antigravity project context and ownership constraints.
- `workflows/`: repeatable Antigravity slash workflows.
- `handoffs/`: dated requests and results exchanged between Antigravity and Codex.

Codex does not discover this directory as project instructions by itself. Codex starts from the repository `AGENTS.md`, which explicitly points here. Antigravity discovers workspace rules and workflows from `.agents/rules/` and `.agents/workflows/`.

The shared technical reference is `docs/IPC_API_HANDOFF.md`. The wire schema remains `protocol/daw_video_recorder.proto`.

After opening the repository in Antigravity, verify the workspace rule activation in the Customizations panel:

- `project-ownership.md`: **Always On**
- `vst-gui-context.md`: **Model Decision** or a glob covering `plugin/src/PluginEditor.*`, `plugin/src/VideoPreviewComponent.*`, and future presentation-only components

Antigravity stores workspace rules under `.agents/rules/`, but activation mode is selected at the rule level in the application. The rule text also states its intended scope so the constraint remains understandable in source control.

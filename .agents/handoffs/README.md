# Cross-tool handoffs

Use this directory for changes that cross the Codex-owned core and Antigravity-owned presentation boundary.

Create one dated file per coherent change using `TEMPLATE.md`. Update the same file as it moves through the states below:

- `requested`: Antigravity needs a core or facade change from Codex.
- `in-progress`: Codex has accepted and is implementing the request.
- `ready-for-gui`: Codex has completed and verified the core change.
- `integrated`: Antigravity has consumed the change and verified the GUI.
- `rejected`: the requested shape was not implemented; the record explains the replacement or reason.

Handoffs are coordination records, not a second API specification. Keep durable contract details in `docs/IPC_API_HANDOFF.md` and the canonical wire schema in `protocol/daw_video_recorder.proto`.

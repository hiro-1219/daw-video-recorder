---
description: Exchange a GUI-facing API request or completed core API change between Antigravity and Codex.
---

# Handoff API change

1. Read `AGENTS.md`, `docs/IPC_API_HANDOFF.md`, and `.agents/handoffs/README.md`.
2. Inspect the current diff and existing handoff records. Reuse the relevant record instead of creating duplicates.
3. Determine the direction:
   - Antigravity to Codex: describe the user-visible need, required facade behavior, threading expectations, fallback UI, and acceptance criteria. Do not prescribe raw IPC unless the wire contract itself must change. Set status to `requested`.
   - Codex to Antigravity: describe the implemented facade or contract delta, compatibility impact, changed files, tests, and exact GUI integration steps. Set status to `ready-for-gui`.
4. Create or update `.agents/handoffs/YYYY-MM-DD-short-title.md` using `TEMPLATE.md`. Keep the record factual and link to `docs/IPC_API_HANDOFF.md` rather than copying the whole contract.
5. If consuming a `ready-for-gui` handoff, verify the documented facade against the headers and diff, implement only presentation-owned changes, run the VST3 build, and set status to `integrated` with the validation result.
6. Leave unresolved compatibility, lifetime, or thread-safety questions explicit; never bridge them with undocumented GUI-side behavior.

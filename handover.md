# Context handover

Prepared 2026-10-27. This records conversation-only context and user instructions.
Project behavior, verification evidence and remaining feature work are deliberately
left to the repository documentation and plans.

## User intent and scope

The user said MIDI could not be connected because another context was using it.
They repeatedly requested device-free musical work. That availability constraint
was stated on 2026-10-21 and has not been rechecked. Keep subsequent work
device-free unless the user explicitly changes the scope, including avoiding
virtual MIDI endpoints and qualification flows that contact MIDI services.

The user chose these approaches when asked:

- Complete selectors for history/phrases first, preserving capture and expression
  behavior, before broader phrase conversion or multiple-arp design work.
- Interpret the next phrase-substitution feature as authored templates, rather
  than recorded MIDI phrases converted against a newly played chord.
- Use a dedicated channel per arp lane, rather than per-note channel allocation.
  The user explicitly approved this architectural choice before implementation.

These decisions explain the delivered scope; they do not authorize a broader
phrase-conversion or allocation architecture. The latest request was documentation
cleanup and this handover. No next implementation milestone has been selected.

## Instructions supplied in chat

- Informational questions are read-only requests. Do not treat background,
  criticism, or a description of the user's changes as implementation authority.
- Make the smallest coherent requested change. Optional features, refactors,
  tooling or dependency changes require permission. Stop and explain a required
  architectural change before implementing it. Do not add async patterns,
  network I/O or databases without permission.
- Every plan must include `Additional work beyond the prompt`, with `None.` when
  there is no extra work. Ask before performing listed extra work.
- Do not launch the developed program, services, demos or full runtime flows
  without explicit instruction. Focused unit tests are allowed for requested
  implementation or verification. Do not run the Python verification sequence
  for documentation-only changes.
- Do not run destructive filesystem cleanup commands. Changes under
  `/Users/tom/.codex` require explicit authorization.
- Requested repository changes must be committed and pushed. Stage new files.
  Do not switch branches or create merge commits; use worktrees only when
  requested or necessary. Updates to a recent commit should use a fixup unless
  that would invalidate a release tag. Keep uv dependency/lock changes in a
  separate commit.
- For Python changes, the user's verification sequence is pytest, Ruff checking
  and formatting, ty, and pyupgrade for the configured Python version. Their
  pasted Ruff example names `recs`; use this project's actual package paths.
- The user calls Pydantic BaseModel classes "data classes" and prefers frozen
  models where possible, explicit type hints, `object` over `Any`, and Tyro on
  Pydantic models for CLIs. Avoid additions to `__init__.py`, re-exports and import
  aliases, except the specified NumPy alias. Prefer existing dependencies.
- Keep communication concise and direct. Avoid em dashes, flattery and emojis.
  Informational answers must end with `Confidence: N/10, one sentence reason`,
  followed by commit IDs/messages or a one-sentence account when no commit was
  created.

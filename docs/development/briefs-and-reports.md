# Task briefs and worker reports

A task brief tells a worker what to do, and a worker report tells the orchestrator what was done.
The two roles are described in
[AGENTS.md §3](../../AGENTS.md#3-operating-model-orchestrator-and-workers).

## The brief

A **brief** names the task and its acceptance criteria, the paths the worker owns (may edit) and
the paths it must not edit, the context (doc sections, earlier decisions, known pitfalls), the
exact signatures or IPC names to implement or use, and the exact commands that must pass.

## The report

A **report** has these sections, in order: **Result** (one paragraph: what exists now that did not
before); **Files changed** (one line per path); **Verification** (every command from the brief,
pass or fail, with the last lines when they matter, and each failure you could not fix); **Not
verified / Windows-only behaviour** (each claim you could not prove on this host, each with a
manual test, [AGENTS.md §11](../../AGENTS.md#11-manual-tests-on-windows)); **Requested changes
outside my scope** (exact diffs, or "none"); **Open questions / follow-ups** (concrete, or
"none"). Its reader did not watch you work, so state facts and do not narrate.

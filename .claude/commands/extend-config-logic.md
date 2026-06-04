---
name: extend-config-logic
description: Workflow command scaffold for extend-config-logic in aish.
allowed_tools: ["Bash", "Read", "Write", "Grep", "Glob"]
---

# /extend-config-logic

Use this workflow when working on **extend-config-logic** in `aish`.

## Goal

Add or update configuration structures and logic, including model/provider resolution and environment variable expansion.

## Common Files

- `src/config/mod.rs`
- `src/config/resolve.rs`

## Suggested Sequence

1. Understand the current state and failure mode before editing.
2. Make the smallest coherent change that satisfies the workflow goal.
3. Run the most relevant verification for touched files.
4. Summarize what changed and what still needs review.

## Typical Commit Signals

- Update src/config/mod.rs with new structs, logic, or loading mechanisms.
- Optionally create or update src/config/{feature}.rs for specific config features (e.g., resolve.rs).

## Notes

- Treat this as a scaffold, not a hard-coded script.
- Update the command if the workflow evolves materially.
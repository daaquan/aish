---
name: add-provider-adapter
description: Workflow command scaffold for add-provider-adapter in aish.
allowed_tools: ["Bash", "Read", "Write", "Grep", "Glob"]
---

# /add-provider-adapter

Use this workflow when working on **add-provider-adapter** in `aish`.

## Goal

Add support for a new AI provider (e.g., OpenAI, Anthropic, Gemini, Mock) by implementing its adapter and registering it in the provider module.

## Common Files

- `src/provider/{provider}.rs`
- `src/provider/mod.rs`

## Suggested Sequence

1. Understand the current state and failure mode before editing.
2. Make the smallest coherent change that satisfies the workflow goal.
3. Run the most relevant verification for touched files.
4. Summarize what changed and what still needs review.

## Typical Commit Signals

- Create or update src/provider/{provider}.rs with the new provider implementation.
- Update src/provider/mod.rs to register or expose the new provider.

## Notes

- Treat this as a scaffold, not a hard-coded script.
- Update the command if the workflow evolves materially.
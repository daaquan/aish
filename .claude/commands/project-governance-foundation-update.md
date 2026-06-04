---
name: project-governance-foundation-update
description: Workflow command scaffold for project-governance-foundation-update in aish.
allowed_tools: ["Bash", "Read", "Write", "Grep", "Glob"]
---

# /project-governance-foundation-update

Use this workflow when working on **project-governance-foundation-update** in `aish`.

## Goal

Establishes or updates project governance by adding or modifying license, code of conduct, contributing guidelines, GitHub templates, and related documentation.

## Common Files

- `LICENSE`
- `CODE_OF_CONDUCT.md`
- `CONTRIBUTING.md`
- `.github/ISSUE_TEMPLATE/bug_report.md`
- `.github/ISSUE_TEMPLATE/feature_request.md`
- `.github/ISSUE_TEMPLATE/config.yml`

## Suggested Sequence

1. Understand the current state and failure mode before editing.
2. Make the smallest coherent change that satisfies the workflow goal.
3. Run the most relevant verification for touched files.
4. Summarize what changed and what still needs review.

## Typical Commit Signals

- Add or update LICENSE file
- Add or update CODE_OF_CONDUCT.md
- Add or update CONTRIBUTING.md
- Add or update PR and issue templates in .github/ISSUE_TEMPLATE/ and .github/PULL_REQUEST_TEMPLATE.md
- Update README.md and CLAUDE.md to reflect governance changes

## Notes

- Treat this as a scaffold, not a hard-coded script.
- Update the command if the workflow evolves materially.
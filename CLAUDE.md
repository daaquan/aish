# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Status

Early scaffold. No application code, build system, or tests exist yet. This file documents the conventions to follow as the codebase grows. Update the **Build & Test** and **Architecture** sections with real commands and structure once they exist — do not invent them before then.

## Language

- All code, comments, identifiers, commit messages, PR titles/descriptions, and code-level documentation: **English**.

## License

- **MIT** (`LICENSE`). Permissive: contributions are licensed under MIT (same as the project, inbound = outbound). Keep the `SPDX-License-Identifier: MIT` marker in `README.md`; every source file starts with `// SPDX-License-Identifier: MIT`.

## GitHub Workflow

- **Direct commits to `main` allowed.** Solo-maintainer workflow: commit straight to `main`. No branch-per-feature or PR is required, though you may still open a PR for larger or riskier work.
- **Conventional Commits** for messages: `<type>: <description>` where type is one of `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `perf`, `ci`.
- **No DCO sign-off required.** Plain `git commit` is fine; `Signed-off-by` is optional.
- **Keep changes scoped.** One logical unit of functionality per commit; do not bundle unrelated changes.
- **When you do open a PR**, merge after green CI (squash, merge, or rebase — your choice). No required review. PR description should summarize all commits and include a test plan (`git diff main...HEAD`).
- See `CONTRIBUTING.md` for the full contributor guide and `CODE_OF_CONDUCT.md`.

## Build & Test

```
# build:  cargo build
# test:   cargo test --all
# lint:   cargo clippy --all-targets -- -D warnings && cargo fmt --all -- --check
# single test: cargo test <name> -- --test-threads=1
```

## Architecture

_To be filled once code exists. Document the big-picture structure that spans multiple files — not an exhaustive file listing._

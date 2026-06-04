# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Status

Early scaffold. No application code, build system, or tests exist yet. This file documents the conventions to follow as the codebase grows. Update the **Build & Test** and **Architecture** sections with real commands and structure once they exist — do not invent them before then.

## Language

- All code, comments, identifiers, commit messages, PR titles/descriptions, and code-level documentation: **English**.

## License

- **AGPL-3.0-only** (`LICENSE`). Copyleft: contributions are licensed under AGPL-3.0-only (same as the project). Any code added must be license-compatible with AGPL-3.0. Keep the `SPDX-License-Identifier: AGPL-3.0-only` marker in `README.md`; add per-file SPDX headers to source files once they exist.

## GitHub Workflow

- **Direct commits to `main` allowed.** Solo-maintainer workflow: commit straight to `main`. No branch-per-feature or PR is required, though you may still open a PR for larger or riskier work.
- **Conventional Commits** for messages: `<type>: <description>` where type is one of `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `perf`, `ci`.
- **DCO sign-off required.** Every commit needs a `Signed-off-by` line — always commit with `git commit -s`.
- **Keep changes scoped.** One logical unit of functionality per commit; do not bundle unrelated changes.
- **When you do open a PR**, squash merge after green CI. `main` is protected (no force-push). PR description should summarize all commits and include a test plan (`git diff main...HEAD`).
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

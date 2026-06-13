# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Status

Working CLI (v0.6.x). An AI copilot for the command line: built-in
subcommands (`commit`, `pr`, `review`, `changelog`, `ask`, …) wrap everyday
developer commands and use configurable model providers to draft clean
summaries and troubleshoot output. Architecture decisions live in `docs/adr/`.

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
build:  cargo build
test:   cargo test --all
lint:   cargo clippy --all-targets -- -D warnings && cargo fmt --all -- --check
single test: cargo test <name> -- --test-threads=1
```

## Architecture

Single Rust binary. Tools are **built-in subcommands** — there is no plugin
system, by decision: see `docs/adr/0001-no-plugin-architecture.md` before
proposing one.

- `src/main.rs` — CLI dispatch; `run_commit` drives the commit flow
  (staged diff → provider → confirm/edit loop → `git commit`).
- `src/cli.rs` — clap definitions. Global `--json` and `--verbose` flags.
- `src/tool/` — built-in tool logic; `tool/commit.rs` builds the prompt and
  post-processes the model reply. New tools go here.
- `src/provider/` — `Provider` trait (`chat`) with Anthropic, OpenAI-compatible
  (incl. Ollama/Kilo), Gemini, mock, and a retry decorator. Selected via model
  aliases in config.
- `src/config/` — `~/.aish/config.yaml` (override `$AISH_CONFIG`): providers,
  model aliases, commit settings, pricing; `validate()` powers
  `aish config check`; `resolve.rs` maps alias → provider + model.
- `src/cache.rs` / `src/audit.rs` / `src/usage.rs` — deterministic response
  cache, JSONL audit log, and `aish usage` cost summaries over that log.
- `tests/commit_e2e.rs` — end-to-end commit flows via `assert_cmd` with
  `AISH_PROVIDER=mock` (no network).

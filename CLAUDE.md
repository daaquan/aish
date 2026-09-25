# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Status

Working CLI (v0.13.x). An AI copilot for the command line: built-in
subcommands (`commit`, `pr`, `review`, `changelog`, `ask`, `fix`, `run`, …) wrap
everyday developer commands and use configurable model providers to draft clean
summaries, diagnose failures, and turn intent into commands.

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
release: cargo test --release --lib update::   # CI: release-only endpoint gate
```

## Architecture

Single Rust binary. Tools are **built-in subcommands** — there is no plugin
system, by decision. Do not propose a plugin architecture.

- `src/main.rs` — parses `Cli` and forwards it to `aish::commands::run`.
- `src/cli.rs` — clap definitions. Global `--json` and `--verbose` flags.
- `src/commands/` — `mod.rs` dispatches each subcommand, mostly to a module
  of its own. Each generating command's module does the I/O (config, git,
  confirm prompts, output, audit) around `tool/`: `commands/commit.rs`
  drives the commit flow (staged diff → provider → confirm/edit loop →
  `git commit`), and `generate.rs` is the cache/provider pipeline they all
  share. `setup`, `config`, `cache`, `update` and `uninstall` have no
  `tool/` behind them; the last two are CLI glue over the core logic in
  `src/update.rs` / `src/uninstall.rs`.
- `src/tool/` — built-in tool logic; `tool/commit.rs` builds the prompt and
  post-processes the model reply. New tools go here.
- `src/provider/` — `Provider` trait (`chat`) with Anthropic, OpenAI-compatible
  (incl. Ollama/Kilo), Gemini, mock, and a retry decorator. Selected via model
  aliases in config.
- `src/config/` — `config.yaml` in the data dir (`$AISH_HOME`, default
  `~/.aish`; `$AISH_CONFIG` overrides the file): providers, model aliases,
  commit settings, pricing; `validate()` powers `aish config check`;
  `resolve.rs` maps alias → provider + model.
- `src/paths.rs` — resolves the data dir (config, cache, audit log), plus the
  helpers that create dirs and files owner-only on unix.
- `src/cache.rs` / `src/audit.rs` / `src/usage.rs` — deterministic response
  cache, JSONL audit log, and `aish usage` cost summaries over that log.
- `tests/commit_e2e.rs` — end-to-end commit flows via `assert_cmd` with
  `AISH_PROVIDER=mock` (no network).

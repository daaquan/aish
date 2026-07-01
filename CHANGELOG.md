<!-- SPDX-License-Identifier: MIT -->

# Changelog

All notable changes to **aish** are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.13.0] — 2026-07-01

### Added

- `aish commit -a`/`--all` stages tracked changes first (`git add -u`),
  mirroring `git commit -a`. Untracked files are still left alone.
- `aish pr --draft` passes through to `gh pr create --draft`.

## [0.12.0] — 2026-06-26

### Added

- Configurable commit style: an optional free-form `commit.instructions`
  field in `~/.aish/config.yaml` is appended to the commit-message prompt as
  extra style guidance (gitmoji prefixes, longer subjects, a mandatory body,
  …). The output-format guardrails stay fixed, so the generated message is
  still clean and `--json`/postprocessing keep working. Unset = previous
  behavior unchanged.

## [0.11.0] — 2026-06-25

### Added

- `aish commit --edit` opens your `$EDITOR` pre-filled with the generated
  message and git's standard commit template (`git commit -e`); saving commits,
  emptying the message aborts. The default `[Y/n/e]` prompt and `--apply`
  direct-commit are unchanged.

## [0.10.0] — 2026-06-22

### Added

- Forge-aware release publishing: `scripts/publish-release.sh` creates a
  release and uploads assets to whichever forge `origin` points at — GitHub
  via `gh`, GitLab via `glab` — so a tag can be cut from GitHub Actions or
  GitLab CI through one entrypoint. Set `AISH_FORGE=github|gitlab` to override
  detection (e.g. self-hosted GitLab).
- `.gitlab-ci.yml` mirroring the GitHub CI/release workflows, so the project
  builds, tests, and publishes releases from GitLab as well.
- GitLab release downloads in the install path, alongside GitHub.

### Fixed

- Provider errors now surface the upstream 400 response body as a
  `BadRequest`, instead of a generic failure, and UTF-8 truncation of long
  error bodies no longer panics on multi-byte boundaries.

## [0.9.0] — 2026-06-14

### Added

- `aish run <prompt>` — turns a natural-language description into a single
  shell command, grounded in the current OS and shell. The command is shown
  behind a confirm/edit gate (`[Y/n/e(dit)]`) before it runs; `--yes` is the
  only path to no-prompt execution and `--print` emits the command without
  running it. Runs via `sh -c` (pipes, globs, and `&&` work) and propagates
  the command's exit code; aborting or `--print` exits 0. The inverse of
  `aish fix`. Honors global `--json`/`--verbose` and `--model`/`--lang`/
  `--no-cache`.

## [0.8.0] — 2026-06-14

### Added

- `aish fix <cmd>` — runs a command, streams its combined output through, and
  on a nonzero exit appends a model-generated diagnosis plus a suggested fix.
  Always propagates the wrapped command's exit code (safe in scripts and `&&`
  chains); success without `--always` is a transparent pass-through with no
  model call. `--shell` runs the command via `sh -c` for pipes and redirects;
  `--always` diagnoses even on success. Command output is tail-capped at 12k
  chars (failures live at the end). Diagnoses and suggests only — it never
  edits files or re-runs the command.

## [0.7.1] — 2026-06-13

### Changed

- On first-run config auto-generation, aish now prints a one-line notice
  pointing at `aish setup` (the template ships without API keys, so commands
  fail until one is configured).

## [0.7.0] — 2026-06-13

### Added

- `aish setup` — interactive configuration wizard. Prompts to enable each
  provider, choose how its API key is stored (plaintext in the config or a
  `${ENV_VAR}` reference, per key), pick a model, and select a default alias,
  then writes `~/.aish/config.yaml` (mode `0600`, backing up any existing file).
- `aish setup --repair` — restore the template config, backing up the current
  file to `<path>.bak`.
- A config is now created automatically on first run when none exists.
- New provider examples in the config template: `openrouter`, `deepseek`,
  and `groq` (all OpenAI-compatible).

### Changed

- Corrected the `kilo` provider endpoint to `https://api.kilo.ai/api/gateway`.

### Removed

- `aish config init` — superseded by `aish setup` and first-run auto-generation.

## [0.6.0] — 2026-06-11

### Added

- `aish pr` — generate a PR title/body from the branch diff against the
  default branch and create the PR via `gh pr create` (#21). Confirm/edit
  loop like `commit`; `--apply` skips the prompt, `--base` overrides
  default-branch auto-detection (origin/HEAD → main/master).
- `aish review` — model review of the staged diff, or the branch diff with
  `--branch`/`--base` (#22). Findings grouped by severity
  (CRITICAL/HIGH/MEDIUM/LOW); `--json` for CI consumption.
- `aish changelog` — Keep-a-Changelog style entries from commits in
  latest-tag..HEAD, overridable with `--from`/`--to` (#23).
- `aish cache stats` / `aish cache clear` — inspect or empty the response
  cache; `clear` confirms first (`-y`/`--yes` skips) (#24).
- `aish completions <shell>` — completion scripts for bash, zsh, fish,
  elvish, and powershell via clap_complete (#25).
- `aish config check --ping` — after static validation, send one minimal
  request per configured provider to verify reachability and credentials;
  nonzero exit on any failure (CI gate) (#26).
- `aish ask "<question>"` — one-shot questions with piped stdin as context
  (capped at 12k chars), cached like other commands (#27).

### Changed

- All generating commands now share one cache + provider pipeline
  (`commands::generate`); the cache stores the raw model reply.

### Fixed

- Stale `AGPL-3.0-only` SPDX marker in `cache.rs` corrected to MIT.
- E2E flake: `uninstall` tests no longer hit ETXTBSY under parallel test
  runs (test binary is now copied via spawned `cp`) (#28).

## [0.5.0] — 2026-06-11

### Added

- `aish update` — self-update from GitHub releases (#19). Compares the
  running version against the latest release tag, downloads the matching
  `aish-<OS>-<arch>` asset, and atomically replaces the binary. Supports
  `--check` (report only; nonzero exit when outdated, usable as a CI gate),
  `--version <tag>` to pin a release, and the global `--json` flag.
  Refuses to touch binaries installed via `cargo install`.
- `aish uninstall` — remove the installed binary (#20). Prompts for
  confirmation (default no); `--yes` skips the prompt, `--purge` also
  deletes the data dir (`$AISH_HOME`, default `~/.aish`) after path-safety
  validation. Without `--purge` the data dir is kept and its size reported.

## [0.4.0] — 2026-06-11

### Changed

- **Breaking:** removed the subprocess plugin system introduced in 0.2.0.
  `commit` is a built-in subcommand again — no `aish plugin install` step.
  Rationale (formerly ADR-0001): the complexity of the
  stdio ABI, installer, and two-repo sync was not justified by a single
  plugin, and the install step hurt UX. The `aish plugin` command group is
  gone; `[plugins.<name>]` config tables are ignored (top-level `commit:` is
  canonical again); the `daaquan/aish-plugins` repo is archived.
- The interactive commit prompt now re-asks after an edit
  (`[Y/n/e(dit)]` → edit → shows the edited message → confirm), instead of
  committing the edited text immediately.

### Removed

- `aish plugin install/update/list/enable/disable/uninstall`.
- The stdio plugin host, JSONL protocol, manifest registry, prebuilt-binary
  fetcher, and the `toml`, `sha2`, `fs2` dependencies.

## [0.3.1] — 2026-06-07

### Fixed

- Plugin install tests that mutate the global `AISH_HOME` env var are now
  serialized (`#[serial]`), fixing a CI race where the default
  multi-threaded test runner let one test clear the env mid-run of another
  and panic with `no entry found for key`.

## [0.3.0] — 2026-06-06

### Added

- `aish plugin install` now downloads a prebuilt plugin binary from the
  registry's GitHub Releases (`{name}-v{version}` tag, `{name}-{target}`
  asset) and verifies it against the release `SHA256SUMS`, removing the
  client-side `cargo build` from the common install path. Installs fall
  back to building from source only when no prebuilt asset exists for the
  host target.

## [0.2.1] — 2026-06-06

### Fixed

- Improved `aish plugin install` diagnostics when Cargo cannot find the Rust
  standard library for the host target, including a concrete `rustup target add`
  recovery command.

## [0.2.0] — 2026-06-05

### Added

- Subprocess plugin system: `aish plugin install/update/list/enable/disable/uninstall`.
  Tools are external binaries spoken to over a stdio JSON ABI; core exposes
  `model.chat` (provider keys stay in core) and `audit.record` services.
- Per-plugin configuration via `[plugins.<name>]` tables — each plugin sees only
  its own scoped config block.
- Global `--json` flag for machine-readable output (`config check`, `usage`);
  `config check --json` still exits nonzero on errors, so it works as a CI gate.
- Release workflow that cross-compiles static-musl Linux and native macOS
  binaries for every installer target, published under raw `uname`-based asset
  names (`aish-$(uname -s)-$(uname -m)`).

### Changed

- `commit` is no longer built in. Install it as a plugin:
  `aish plugin install commit`.
- Relicensed from **AGPL-3.0** to the **MIT License**; dropped the CLA and
  DCO sign-off requirements — contributions are welcome with no extra ceremony.
- Linux release binaries are now built as **static musl** builds for
  portability across distros.

### Security

- The host enforces per-phase timeouts on every plugin (startup / request /
  service / reap) and SIGKILLs a child that overstays; plugin stderr is drained
  into a bounded 64 KiB ring buffer so a flood cannot grow host memory.
- The plugin frame reader enforces the 1 MiB frame cap *during* the read, so a
  plugin that never emits a newline cannot buffer unbounded input.
- Non-UTF-8 external argv is captured and rejected with a clear, positional error
  instead of being forwarded or surfacing a generic parser message.

## [0.1.0] — 2026-06-05

### Added

- `aish commit` — generates a Conventional Commits message from your staged
  diff, with interactive confirm, `--apply`, and DCO `--signoff`.
- Provider-agnostic backend behind a single config and model alias:
  Anthropic, OpenAI, Google Gemini, Ollama, and Kilo.
- `AISH_PROVIDER=mock` test mode returning a canned `$AISH_MOCK_REPLY` with no
  network calls, for offline/CI smoke checks.
- `install.sh` install script and project governance foundation
  (`CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, design specs).

[Unreleased]: https://github.com/daaquan/aish/compare/v0.8.0...HEAD
[0.8.0]: https://github.com/daaquan/aish/compare/v0.7.1...v0.8.0
[0.7.1]: https://github.com/daaquan/aish/compare/v0.7.0...v0.7.1
[0.7.0]: https://github.com/daaquan/aish/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/daaquan/aish/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/daaquan/aish/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/daaquan/aish/compare/v0.3.1...v0.4.0
[0.3.1]: https://github.com/daaquan/aish/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/daaquan/aish/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/daaquan/aish/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/daaquan/aish/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/daaquan/aish/releases/tag/v0.1.0

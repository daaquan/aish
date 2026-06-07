<!-- SPDX-License-Identifier: MIT -->

# Changelog

All notable changes to **aish** are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/daaquan/aish/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/daaquan/aish/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/daaquan/aish/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/daaquan/aish/releases/tag/v0.1.0

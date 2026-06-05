<!-- SPDX-License-Identifier: MIT -->

# Changelog

All notable changes to **aish** are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Subprocess plugin system: `aish plugin install/list/enable/disable/uninstall`.
  Tools are external binaries spoken to over a stdio JSON ABI; core exposes
  `model.chat` (keys stay in core) and `audit.record` services.
- Release workflow that cross-compiles binaries for every installer target,
  published under raw `uname`-based asset names (`aish-$(uname -s)-$(uname -m)`).

### Changed

- `commit` is no longer built in. Install it as a plugin:
  `aish plugin install commit`.
- Relicensed from **AGPL-3.0** to the **MIT License**; dropped the CLA and
  DCO sign-off requirements — contributions are welcome with no extra ceremony.
- Linux release binaries are now built as **static musl** builds for
  portability across distros.

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

[Unreleased]: https://github.com/daaquan/aish/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/daaquan/aish/releases/tag/v0.1.0

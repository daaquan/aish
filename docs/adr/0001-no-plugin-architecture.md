<!-- SPDX-License-Identifier: MIT -->

# ADR-0001: Drop the subprocess plugin architecture; `commit` is a built-in

**Status:** Accepted · **Date:** 2026-06-11 · **Author:** daaquan

## Context

v0.2 (2026-06-05) turned aish into a plugin host: tools were external
subprocess binaries speaking a stdio JSONL ABI, installed from a separate
registry repo (`daaquan/aish-plugins`), with host services (`model.chat`,
`audit.record`), per-phase timeouts, frame caps, manifest/permission checks,
prebuilt-binary installs (v0.3), and a two-repo release pipeline. `commit`
was extracted as the first — and only — plugin. A marketplace accepting
third-party, any-language plugin uploads was considered as the long-term
direction.

A 2026-06-11 architecture review found the plugin seam was the dominant
source of friction: the ABI contract (`Frame`), git helpers, and editor
helpers were copy-pasted across the two repos and already drifting (the
plugin side carried a `/dev/tty` editor fix the core lacked, with zero
protocol tests); the host loop, installer, and prebuilt fetcher accounted
for ~1,700 lines with no unit coverage.

## Decision

Revert to built-in tools. `aish commit` is a core subcommand again. The
plugin host, protocol, installer, prebuilt fetcher, manifest registry, and
the `aish plugin` command group are deleted. The `aish-plugins` GitHub repo
is archived (kept read-only for history). The marketplace plan is cancelled.

## Reasons

1. **Complexity is not worth it.** Subprocess lifecycle + JSONL ABI +
   installer + checksum verification + two-repo version sync is a heavy
   maintenance load for a solo maintainer, paid on every change.
2. **Only one plugin ever existed.** The ABI had a single consumer; an
   interface with one adapter is a hypothetical seam, and this one never
   attracted a second adapter.
3. **UX burden.** `aish plugin install commit` (with a trust prompt and a
   Rust toolchain fallback) before the tool works at all is worse than a
   binary that works out of the box.

## Consequences

- Future tools are added as built-in subcommands under `src/tool/`.
- The provider layer, config, cache, audit, and usage tracking are
  unchanged — they were never plugin-specific.
- Users upgrading from v0.2/v0.3: `[plugins.commit]` config tables are
  ignored (the top-level `commit:` block is canonical again); installed
  plugin binaries under `~/.aish` are inert and can be deleted.
- The v0.2 plugin design specs under `docs/superpowers/` remain as
  historical record; this ADR supersedes them.

## Revisit when

A second or third tool with a genuinely external author or language
requirement appears. Until a real second adapter exists, do not re-propose
a plugin seam (subprocess, dynamic library, or SDK crate) in architecture
reviews.

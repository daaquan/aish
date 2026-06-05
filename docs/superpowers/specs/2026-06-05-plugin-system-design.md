<!-- SPDX-License-Identifier: MIT -->

# aish v0.2 — Plugin System + `commit` Extraction — Design

**Status:** Approved (brainstorming) · **Date:** 2026-06-05 · **Author:** daaquan
**Reviewed by:** codex gpt-5.5 (2026-06-05) — findings folded in (§Protocol, §Security, §Install).

## Vision

v0.1 shipped `commit` as a built-in subcommand. v0.2 turns aish into a true
plugin host: tools are **external subprocess plugins** that are installed,
enabled, and disabled independently of the core binary. `commit` is the first
plugin and the proof of the ABI.

```
aish plugin install commit     # build + install from the registry
aish commit                    # core spawns the commit plugin over stdio
aish plugin disable commit     # tool gone; `aish commit` errors again
```

This realizes the v0.1 spec's deferred item *"External plugin loader + manifest +
ABI (subprocess plugins over stdio)"*.

## Scope decisions

Settled during brainstorming; these constrain the whole design.

| Axis | Decision | Rationale |
|------|----------|-----------|
| Coupling | **Subprocess plugins over a stdio JSON ABI** | No in-process registry, no dynamic libs. Process isolation; any-language plugins later; stable ABI instead of a brittle Rust ABI. |
| Boundary | **Fat plugin + host callbacks** | Plugin owns the whole flow (git read, prompt, render, commit); core exposes a small set of host *services*. Core becomes an RPC server for the duration of one invocation. |
| Bootstrap | **Fully external — core ships zero tools** | `aish commit` errors until `aish plugin install commit`. Cleanest core, plugin-first. |
| Distribution | **Registry git repo, source crates, build-on-install** | `git@github.com:daaquan/aish-plugins.git` is a cargo workspace, one crate per plugin; install builds via cargo. |
| Trust model | **Installed plugins are fully trusted native code** | build-on-install + a native subprocess can run arbitrary code. Capabilities authorize host *services*, not OS behavior. Documented, not sandboxed (§Security). |

## Topology

- **aish core** — owns config, secrets, the provider layer, audit, the plugin
  host, and the management commands (`plugin`, `config`, `providers`, `models`).
  Ships **no** tools.
- **aish-plugins** (separate repo) — cargo workspace, one crate per plugin;
  `commit/` is the first. Each plugin is a standalone binary speaking the ABI.
- Dispatch: `aish commit` → clap captures the unknown subcommand → core resolves
  the installed+enabled plugin that declares `commit` → spawns it.

## Protocol (stdio ABI)

Newline-delimited JSON (JSONL), one frame per line, over the child's
**stdin/stdout**. Reserved channels:

| Channel | Use |
|---------|-----|
| child **stdin** | host → plugin frames |
| child **stdout** | plugin → host frames |
| child **stderr** | plugin logs (surfaced on `--verbose`) |
| **`/dev/tty`** | **all human-facing UI** — the plugin opens `/dev/tty` directly for rendering the suggestion and reading `[Y/n]`. |

> **Terminal ownership (codex #12/#13).** stdin/stdout are the protocol channel,
> so the plugin must NOT use them for the `[Y/n]` prompt or to print the
> suggested message. Interactive plugins open `/dev/tty`. If `/dev/tty` is
> unavailable (non-interactive/CI), the plugin treats it as "no input" and must
> be driven by flags (`--apply`) or it aborts without committing.

### Frames

```
host → plugin   {"id":1,"type":"invoke","subcommand":"commit","args":["--apply"],
                 "cwd":"/repo","config":{...},"services":["model.chat","audit.record"]}
plugin → host   {"id":2,"type":"request","op":"model.chat","payload":{...}}
host → plugin   {"id":2,"type":"response","ok":true,"payload":{...}}
plugin → host   {"id":3,"type":"request","op":"audit.record","payload":{...}}
host → plugin   {"id":3,"type":"response","ok":true}
plugin → host   {"id":1,"type":"result","ok":true,"payload":{"exit":0}}
```

- Frame types: `invoke`, `request`, `response`, `result`. v0.2 supports
  **plugin-initiated `request`s only** (the host never initiates a `request`
  mid-invoke); the shape is bidirectional so host-initiated requests can be
  added under a later ABI without a redesign (codex YAGNI).
- **`id` rules (codex #7):** `u64`. Host owns `invoke`/`result` ids; the
  requester owns its `request` id and the matching `response` echoes it. A
  duplicate or unknown id is a protocol error → terminate the plugin.
- **Frame limits (codex #4):** max frame = **1 MiB**; oversize line, invalid
  UTF-8, malformed JSON, missing `id`/`type`, or unknown ABI major → host kills
  the plugin and reports a structured error. Unknown *fields* are ignored
  (additive-compatible).
- **`abi` versioning (codex #11):** manifest `abi = "1"` ⇒ protocol major 1.
  Additive frame fields are ignored; any new required op bumps to ABI 2. The host
  advertises the service ops it supports in the `invoke` frame's `services` list,
  so a plugin can detect missing services rather than hanging.

### Host services (the only callbacks core exposes)

| op | Behavior | Boundary |
|----|----------|----------|
| `model.chat` | Core resolves `model` alias → provider+model, injects the key, calls the provider, returns `{content, usage}`. | API keys + network stay in core. Enforces alias resolution and config-scoped bounds: max prompt bytes, allowed alias, temperature range (codex #8). |
| `audit.record` | Append a JSONL line to `~/.aish/audit.log`. | Centralized audit; plugin cannot bypass it. |

Everything else the plugin does itself in its own process/cwd: read the staged
diff (`git diff --cached`), prompt/render via `/dev/tty`, run `git commit`.

### Concurrency / deadlock avoidance (codex #3)

The host runs **four concurrent tasks** per plugin invocation, joined by bounded
channels:

1. **stdout reader** — parse frames, dispatch `request`/`result`.
2. **stderr drainer** — always draining so a chatty plugin can't block.
3. **stdin writer** — serialize `response`/`invoke` frames.
4. **child waiter** — reap exit status.

The request handler never blocks on writing to child stdin while stdout is not
being drained. A bounded queue applies backpressure without deadlock.

### Timeouts (codex #5)

Distinct, configurable budgets: **startup** (spawn → first frame), **per
host-service request**, **idle silence** (no frame and no outstanding request),
and **graceful-shutdown → kill** (after the host closes stdin).

The interactive `[Y/n]` prompt is a problem for an idle timer: the plugin is
blocked on `/dev/tty` with no protocol traffic. v0.2 resolves this by ordering —
the plugin requests `model.chat` (and any other service) **before** it prompts.
By the time it blocks on the prompt it owes the host nothing, so the host applies
**no idle timeout once the plugin has no outstanding service request**; it only
enforces startup, per-request, and shutdown budgets. A wedged plugin that hangs
*before* responding is still caught by the per-request/startup timers.

### Error propagation (codex #6)

Precedence, mapped to structured error codes (not free strings):

| Situation | Outcome |
|-----------|---------|
| `result ok:true` then exit 0 | success, use `result.payload.exit` |
| `result` then exit ≠ 0 | host-level error (plugin misbehaved post-result) |
| exit before any `result` | plugin crash; report captured stderr tail |
| host service request fails | `response ok:false` with `{code, message}` |
| unknown op requested | `response ok:false` `code=unknown_op` (plugin may continue) |
| protocol violation (§Frame limits) | kill plugin, structured `protocol_error` |

## Manifest — `aish-plugin.toml` (per plugin)

```toml
name = "commit"
version = "0.1.0"
description = "AI commit message from the staged diff"
abi = "1"
subcommands = ["commit"]

# Declared host-service permissions. NOT a sandbox — gates RPC ops only.
[permissions]
model = true        # may call model.chat
audit = true        # may call audit.record
```

The host rejects a `request` for an op the manifest didn't declare with
`response ok:false code=permission_denied`.

## Install / registry

State on disk:

```
~/.aish/registry/                 # cached clone of the registry repo (pinned)
~/.aish/plugins/<name>/           # installed binary + aish-plugin.toml
~/.aish/plugins.toml              # installed-plugin state (see below)
```

`plugins.toml` entry (codex #10, kept minimal per YAGNI):

```toml
[commit]
version = "0.1.0"
abi = "1"
enabled = true
path = "~/.aish/plugins/commit/commit"
subcommands = ["commit"]
source = "git@github.com:daaquan/aish-plugins.git"
revision = "<git-sha>"            # pinned commit actually built
binary_sha256 = "<hash>"
```

Commands:

- `aish plugin install <name>` — clone/pull the registry **to a pinned SHA** →
  locate `<name>/` → **show source + version + revision and confirm** → build →
  install → record (enabled).
- `aish plugin list` — installed plugins + enabled state.
- `aish plugin enable <name>` / `disable <name>` — toggle `enabled`.
- `aish plugin uninstall <name>` — remove dir + entry.
- Registry URL overridable via `config.yaml` (`[plugins] registry = "..."`);
  a **local filesystem path** is accepted as a registry for tests/dev.

### Build-on-install pipeline (atomic, codex #1 #14 #16)

1. Resolve registry to a pinned git SHA (no silent `pull` drift; upgrades are
   explicit via re-install).
2. `cargo build --release --locked -p <crate>` with an **isolated `CARGO_HOME`**
   under `~/.aish/`.
3. Build + stage into a **temp dir**; compute `binary_sha256`.
4. **Atomic rename** temp → `~/.aish/plugins/<name>/`.
5. Update `plugins.toml` under a **file lock** (guards concurrent installs).

### Conflict policy (codex #15)

If enabling/installing a plugin would make two **enabled** plugins declare the
same subcommand, the operation is **rejected** with a clear error. No implicit
priority.

## Security

> **Plugins are fully trusted native executables.** Install builds and runs
> arbitrary code (`build.rs`, proc-macros, the plugin binary itself). The
> `[permissions]` block authorizes **host RPC services only** — it is not an OS
> sandbox and does not constrain file/network/process access by the plugin.

Mitigations actually implemented in v0.2 (codex #1):

- Registry pinned to an explicit git SHA; recorded in `plugins.toml`.
- `install` prints the source repo, revision, and version and **requires
  confirmation** before building (skippable with `--yes`).
- Isolated `CARGO_HOME`; `--locked` builds.
- `binary_sha256` recorded; re-verified before each spawn (tamper detection).
- `config` forwarding is **sanitized** (codex #9): the `invoke` frame carries
  only the plugin-scoped slice (`config.commit`) plus host metadata — never
  provider keys or unrelated config. Secrets never cross the process boundary;
  `model.chat` keeps them in core.

## Core code changes

- **CLI:** delete `Command::Commit` + `run_commit`. clap
  `allow_external_subcommands(true)` → `Command::External(Vec<String>)` catch-all
  routed to the plugin dispatcher. Keep `config`/`providers`/`models`; add
  `plugin`.
- **Replace `src/tool/`** (the now-obsolete in-process `Tool`/`Registry` seam)
  with `src/plugin/`:
  - `protocol.rs` — frame types + serde, frame-size guard, id/abi rules.
  - `host.rs` — spawn, the 4-task concurrent loop, service dispatch, timeouts,
    error mapping.
  - `services.rs` — `model.chat` (reuses `build_provider` + `resolve_model`;
    keeps the `AISH_PROVIDER=mock` hook in core for offline tests) and
    `audit.record` (reuses `audit::record`).
  - `manifest.rs` — `aish-plugin.toml` + `plugins.toml` parse/serialize, file
    lock, conflict check.
  - `install.rs` — registry pin/clone, build pipeline, atomic install.
- **config:** `config.yaml` keeps a `commit:` block; the host forwards it as the
  sanitized `invoke.config` slice. Add optional `[plugins] registry`.

## The `commit` plugin (in aish-plugins)

- Move `build_messages` + `postprocess` **verbatim** (with their unit tests) into
  `commit/src/`.
- New `main.rs`: detect plugin mode (invoked by host), run the stdio loop:
  read `invoke` → `git diff --cached` (abort-friendly if empty) → build messages
  → `request model.chat` → `postprocess` → render + `[Y/n]` on `/dev/tty`
  (or `--apply`) → `git commit` → `request audit.record` → send `result`.

## Testing

TDD throughout; ≥80% coverage; **no network in CI**.

- **Unit:** protocol serde roundtrip + frame-limit/violation handling; id/abi
  rules; manifest + `plugins.toml` parse and conflict detection; install
  resolution against a **local fixture registry** (no network); the preserved
  `build_messages` / `postprocess` tests in the plugin crate.
- **Integration:** host loop driven against a **fake plugin** (a tiny test
  binary) exercising `model.chat` + `audit.record`, id correlation, `ok:false`
  paths, crash-before-result, oversize-frame kill, and a timeout.
- **E2E:** install `commit` from a local fixture registry into a temp
  `$AISH_HOME`; run `aish commit` in a temp git repo with `AISH_PROVIDER=mock`;
  assert the message and the resulting commit. Use one full-build smoke test plus
  a tiny fixture plugin for the rest so plugin E2E doesn't dominate CI time
  (codex #17).

## Phasing (for the implementation plan)

1. `protocol.rs` + `manifest.rs` types and rules (+ unit tests).
2. `host.rs` + `services.rs`: spawn, 4-task loop, timeouts, error mapping —
   tested against the fake plugin.
3. `install.rs`: registry pin + atomic build-on-install (local-path + git SHA).
4. CLI rewire: external subcommands, the `plugin` command, remove built-in
   `commit`.
5. Scaffold the `aish-plugins` workspace + `commit` crate; move logic; wire the
   ABI; `/dev/tty` UI.
6. E2E + docs (README, CHANGELOG, flip the v0.1 spec's "deferred" item to done).

## Deferred to v0.3+

- Host-initiated `request`s (server push to plugins).
- Prebuilt-binary distribution / signed artifacts / a real marketplace index.
- OS-level sandboxing of plugins (seccomp/namespaces) to make `[permissions]`
  enforceable beyond RPC.
- Non-`commit` plugins; richer permission prompts.

## Definition of done (v0.2)

`aish plugin install commit && git add . && aish commit` builds the commit plugin
from the registry, runs it over the stdio ABI, and produces + applies a
Conventional Commits message — with `plugin list/enable/disable/uninstall`,
sanitized config forwarding, the `model.chat`/`audit.record` services,
trusted-install confirmation, structured error/timeout handling, and the test
suite above green in CI. Core ships zero built-in tools.

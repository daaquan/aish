<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# aish v0.1 — MVP Design

**Status:** Approved (brainstorming) · **Date:** 2026-06-04 · **Author:** daaquan

## Vision

`aish` is an AI-native shell: an AI-powered, extensible shell for developers
(`bash` / `zsh` / `fish` / `aish`). Capabilities are exposed as **Tools** and
executed through AI, with a **provider-agnostic** model layer so aish never
depends on a single AI vendor.

This document specifies **v0.1 only**. v0.1 proves the smallest end-to-end loop:

```
git add .
aish commit       # diff → AI → "feat(core): add plugin loader" → git commit
```

The broader vision (agent runtime, external plugins, permission engine,
marketplace, non-commit tools) is intentionally **deferred** — see §10.

## Scope decisions

These were settled during brainstorming and constrain the whole design:

| Axis | Decision | Rationale |
|------|----------|-----------|
| Core language | **Rust** | Matches existing tooling; single static binary; safe + fast. |
| Plugins in v0.1 | **In-core, no external loader** | Build `commit` as a built-in; defer plugin SDK/manifest/ABI until the Tool API is proven by real usage. |
| Agent loop | **None** | Commit generation is a single deterministic call (diff → one completion → message). No tool-calling/ReAct loop needed. |
| Providers | **5 across 3 API shapes** | Anthropic, OpenAI, Google Gemini, Ollama, Kilo gateway. |
| Provider impl | **Hand-rolled adapters** (`reqwest`) | Realizes "provider agnostic" directly; AGPL-clean deps; full control of request/response + errors. |

## Architecture

```
aish commit [--apply]
   │
 cli (clap)        parse args
   │
 config            load ~/.aish/config.yaml, expand ${ENV}, resolve model alias → provider+model
   │
 git               git diff --cached   (guard: nothing staged)
   │
 tool::commit      build messages (Conventional Commits rules + style + language + diff)
   │
 provider (trait)  .chat(req) → 1 of 3 adapters
   │
 render            print suggestion → [Y/n] or --apply → git commit -m
   │
 audit             append JSONL line
```

No agent module, no plugin loader. Only the modules listed below.

## Crate layout

Single binary crate with a `lib.rs` so logic is exercised by integration tests;
`main.rs` stays thin.

```
aish/
├── Cargo.toml
├── src/
│   ├── main.rs          # clap entry, dispatch subcommands
│   ├── lib.rs           # re-exports for integration tests
│   ├── cli.rs           # arg structs (commit, config init, providers/models list)
│   ├── config/
│   │   ├── mod.rs       # Config structs, load + ${ENV} expansion
│   │   └── resolve.rs   # model alias → (provider, model)
│   ├── provider/
│   │   ├── mod.rs       # Provider trait, ChatRequest/Response, Role, ProviderError, build_provider()
│   │   ├── anthropic.rs # API shape 1
│   │   ├── openai.rs    # API shape 2 — OpenAI + Ollama + Kilo via base_url
│   │   └── gemini.rs    # API shape 3
│   ├── tool/
│   │   ├── mod.rs       # internal Tool trait + registry seam (for v0.2 external loader)
│   │   └── commit.rs    # git.commit.message.generate
│   ├── git.rs           # staged diff read + commit apply (std::process::Command)
│   └── audit.rs         # JSONL append
└── tests/
    └── commit_e2e.rs    # temp git repo + MockProvider
```

Vision's `internal/{agent,plugin,permission,model}` directories are **not
created** in v0.1. The `tool/` module keeps an internal `Tool` trait + registry
so the future external subprocess loader drops in without restructuring — the
seam exists, the ABI does not.

Every source file carries a per-file `// SPDX-License-Identifier: AGPL-3.0-only`
header (per repository `CLAUDE.md`).

## Provider layer

One trait, three concrete adapter shapes, five reachable providers.

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, ProviderError>;
    // stream() is deferred to v0.2 — commit needs a single completion only.
}

pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
}
pub struct Message      { pub role: Role, pub content: String }
pub enum   Role         { System, User, Assistant }
pub struct ChatResponse { pub content: String, pub usage: Option<Usage> }
pub struct Usage        { pub prompt_tokens: u32, pub completion_tokens: u32 }
```

Adapter → provider mapping:

| Adapter (`src/provider/`) | Serves | How |
|---------------------------|--------|-----|
| `anthropic.rs` | Anthropic | Native Messages API. |
| `openai.rs` | OpenAI, Ollama, Kilo gateway | OpenAI-compatible `/v1/chat/completions`; Ollama + Kilo selected via `base_url` override. |
| `gemini.rs` | Google Gemini | Native `generateContent` API. |

`build_provider(cfg, alias) -> Box<dyn Provider>` constructs the right adapter
from resolved config.

> **Assumption to confirm during implementation:** Kilo gateway is
> OpenAI-compatible (`base_url` + bearer key). If it uses a custom protocol it
> needs its own adapter; the trait absorbs that without other changes.

## Configuration

Location: `~/.aish/config.yaml` (override via `$AISH_CONFIG`).

```yaml
providers:
  anthropic: { api_key: ${ANTHROPIC_API_KEY} }
  openai:    { api_key: ${OPENAI_API_KEY} }
  google:    { api_key: ${GOOGLE_API_KEY} }
  ollama:    { base_url: http://localhost:11434/v1 }
  kilo:      { api_key: ${KILO_API_KEY}, base_url: https://gateway.kilo.example/v1 }

models:
  default: { provider: anthropic, model: claude-opus-4-8 }
  fast:    { provider: openai,    model: gpt-5-mini }
  local:   { provider: ollama,    model: qwen3-coder }

commit:
  style: conventional     # commit message style
  language: en            # output language
  model: default          # which model alias commit uses
```

Rules:

- `${VAR}` expansion happens at load time.
- A missing **required** secret fails fast: the error names the variable and
  **never prints its value** (security boundary validation).
- Invalid YAML reports line context.
- `aish config init` writes a commented template; it never overwrites an
  existing file without `--force`.

## Data flow — `aish commit`

1. Parse args: `aish commit [--apply] [--model <alias>] [--style <s>] [--lang <l>]`.
2. Load + validate config; resolve model alias → `(provider, model)`.
3. Read staged diff via `git diff --cached`. If nothing is staged, exit with a
   friendly message (non-error).
4. Build messages: a system prompt encoding Conventional Commits rules + target
   language + subject length limits; the user message carries the diff. Very
   large diffs are capped/truncated with a marker so the request stays bounded.
5. `provider.chat()` → message text; post-process (strip ``` fences, trim
   whitespace).
6. Render the suggestion. With `--apply`, skip the prompt and run
   `git commit -m <message>`. Otherwise prompt `[Y/n]`: `Y` commits, `n` aborts.
   (Interactive edit `e` is deferred to v0.2.)
7. Append an audit line: `{ts, tool, provider, model, prompt_tokens,
   completion_tokens, decision}` to `~/.aish/audit.log` (JSONL).

## Error handling

Typed errors in the library (`thiserror`); `anyhow` at the binary boundary.
Each failure gives a distinct, user-friendly message and exit code:

| Condition | Message intent |
|-----------|----------------|
| No config file | Point to `aish config init`. |
| Missing env secret | Name the variable; do not print its value. |
| Bad YAML | Report line context. |
| Not a git repo | State it plainly. |
| Nothing staged | Friendly, non-error exit. |
| `git` binary missing | State it plainly. |
| Provider 401 / 429 / timeout / bad-model | Mapped `ProviderError` variants; detail behind `--verbose`. |
| Empty / unusable model output | **Do not commit**; show raw output + warn. |

No `api_key` is ever logged. Errors are never silently swallowed.

## Testing

TDD throughout; target ≥80% coverage; **no network in CI**.

- **Unit:** config load + `${ENV}` expansion + alias resolution; prompt builder;
  response post-processing (fence stripping); each adapter's request-build and
  response-parse against **recorded JSON fixtures** (no live calls).
- **Integration:** the commit flow with a `MockProvider` returning canned text —
  assert rendered output and that `--apply` invokes a commit (against a temp git
  repo).
- **E2E:** create a temp git repo, stage a file, run the binary with the mock
  provider selected via env (e.g. `AISH_PROVIDER=mock`), assert exit code and
  emitted message.
- **Real-provider smoke tests:** marked `#[ignore]`, run only with env keys
  present.

## v0.1 commands

```
aish config init        # write a commented config template
aish providers list     # list configured providers + whether a key/endpoint is present
aish models list        # list aliases → provider/model
aish commit             # interactive: suggest, then [Y/n]
aish commit --apply     # generate and commit without prompting
```

## Deferred to v0.2+

- ~~External plugin loader + manifest + ABI (subprocess plugins over stdio).~~
  **Done in v0.2** — see `docs/superpowers/specs/2026-06-05-plugin-system-design.md`.
- Agent / tool-calling loop (multi-step reasoning).
- Permission engine and richer audit policy.
- Streaming responses (`Provider::stream`).
- Interactive edit (`e`) in the commit prompt.
- Marketplace, web UI, cloud/remote runtime.
- Non-commit tools (GitHub, GitLab, Docker, Jira, browser, …) — all extend the
  same Tool API.

## Definition of done (v0.1)

`git add . && aish commit` produces a Conventional Commits message via any
configured provider and, on confirmation (or `--apply`), creates the git commit
— with config init, provider/model listing, audit logging, typed error
handling, and the test suite above green in CI.

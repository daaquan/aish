<!-- SPDX-License-Identifier: MIT -->

# Security Policy

## Supported Versions

aish is a solo-maintained project on a rolling release. Only the latest
released version receives security fixes.

| Version | Supported |
| ------- | --------- |
| 0.13.x  | ✅        |
| < 0.13  | ❌        |

Upgrade to the latest release before reporting an issue.

## Reporting a Vulnerability

**Do not open a public issue for security problems.**

Report privately via GitHub Security Advisories:
<https://github.com/daaquan/aish/security/advisories/new>

If you cannot use GitHub, email daaquan@gmail.com with `[aish security]` in
the subject.

Please include:

- affected version (`aish --version`) and platform
- steps to reproduce, ideally a minimal config or command
- impact you believe it has
- any suggested fix

### What to expect

- **Acknowledgement**: within 7 days.
- **Assessment and plan**: within 14 days.
- **Fix**: released as soon as practical; critical issues get a patch release.
- **Credit**: reporters are credited in the advisory and `CHANGELOG.md` unless
  they ask otherwise.

This is a volunteer project with no bug bounty.

## Scope

In scope: the `aish` binary, its configuration handling, provider clients,
cache, and audit log.

Out of scope:

- vulnerabilities in upstream model providers (Anthropic, OpenAI, Gemini,
  Ollama) — report those to the vendor
- vulnerabilities in third-party crates — report upstream, though telling us
  so we can bump the dependency is welcome
- issues that require an attacker to already have local write access to
  `~/.aish/` or the user's shell

## Security model and user responsibilities

aish sends the content you pass it (diffs, command output, prompts) to the
model provider you configure. Treat that as disclosure to a third party.

- **API keys** live in `~/.aish/config.yaml`, which aish writes with mode
  `600`. Prefer environment variable interpolation
  (`api_key: ${ANTHROPIC_API_KEY}`) over literal keys, and never commit the
  file.
- **Audit log** (`~/.aish/audit.log`, JSONL) records metadata only — tool,
  provider, model, token counts, and your decision. No prompt or response text,
  no keys.
- **Cache** (`~/.aish/cache/`) stores provider *responses* on disk, keyed by a
  hash of the request. Responses can contain your code. Files are written with
  your default umask, so on a shared machine tighten `~/.aish` yourself:
  `chmod 700 ~/.aish`. `aish cache clear` empties it.
- **`aish run` / `aish fix`** can propose shell commands. Read what is proposed
  before confirming, and do not pass `--yes` (no-prompt execution) when the
  input comes from an untrusted source.

## Disclosure

We follow coordinated disclosure. Please give us 90 days before going public,
or sooner by mutual agreement once a fix ships.

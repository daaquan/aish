<!-- SPDX-License-Identifier: MIT -->

# Security Policy

## Supported Versions

aish is a solo-maintained project on a rolling release. Only the latest
released version receives security fixes.

| Version        | Supported |
| -------------- | --------- |
| Latest release | ✅        |
| Older releases | ❌        |

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
- attacks by someone who already runs code as you, controls your shell or the
  whole environment an `aish` invocation sees (which includes `LD_PRELOAD` and
  the like, so it amounts to running code as you), or can write files in the
  data dir (`$AISH_HOME`, default `~/.aish`) such as `config.yaml` or cache
  entries — anything that access already lets them do directly

In scope, though, is anything in aish that turns partial control over a
single invocation, such as a wrapper or CI job that lets a caller set a few of
its environment variables (`HTTP_PROXY` and the like, or an API key variable
the config interpolates), into an effect on later invocations, or escalates
it: for example redirecting a self-update (which is why the `AISH_UPDATE_*`
endpoint overrides are compiled out of release builds), or one invocation
planting a cache entry that later ones trust.

Picking the config file, by contrast, is config-level control, not partial
control. Its `${VAR}` interpolation reads any environment variable by name,
and aish sends the values, as the `api_key` or inside the URL, with the
invocation's prompts and diffs to whatever `base_url` the file names. So
whoever picks the config file — the one `$AISH_CONFIG` names or else
`config.yaml` in the data dir (`$AISH_HOME`, default `~/.aish`) — decides
where your prompts, diffs and every variable it names are sent: treat it like
your shell rc, and never let an untrusted caller set `$AISH_CONFIG`,
`$AISH_HOME` or `$HOME`, which moves `~`. An effect such an invocation has on
later ones that use your own config, such as a planted cache entry, is still
in scope.

## Security model and user responsibilities

aish sends the content you pass it (diffs, command output, prompts) to the
model provider you configure. Treat that as disclosure to a third party.

- **API keys** live in `config.yaml` in the data dir (`$AISH_HOME`, default
  `~/.aish`), or in the file `$AISH_CONFIG` names instead. aish writes it with
  mode `600`. Prefer environment variable interpolation
  (`api_key: ${ANTHROPIC_API_KEY}`) over literal keys, and never commit the
  file.
- **Audit log** (`audit.log` in the data dir, JSONL) records metadata only —
  tool, provider, model, token counts, and your decision. No prompt or
  response text, no keys.
- **Cache** (`cache/` in the data dir) stores provider *responses* on disk,
  keyed by a hash of the request. Responses can contain your code. On unix,
  aish creates the data dir and `cache/` with mode `700`, and the files it
  writes there with mode `600`. It never changes the mode of a directory that
  already exists, since a `$AISH_HOME` may be shared on purpose, so tighten a
  data dir created by an older version, or an existing directory you pointed
  `$AISH_HOME` at, yourself: `chmod 700 "${AISH_HOME:-$HOME/.aish}"`. aish
  ignores a `$AISH_HOME` that is not an absolute path (such as a quoted
  `~/...`) and uses `~/.aish`, so in that case run `chmod 700 ~/.aish`.
  `aish cache clear` empties the cache; it asks first and treats
  non-interactive input as "no", so pass `--yes` when running it from a script.
- **Cache keys** cover the provider name, its endpoint (`base_url`), a
  SHA-256 hash of its API key (never the key itself), the proxy settings the
  request is sent with (`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY` and
  `NO_PROXY` in either case, and whether `REQUEST_METHOD` is set, which turns
  them off), the model, and every message, so one invocation's
  `$AISH_CONFIG`, API key or proxy variable does not decide what later ones are
  served. aish never uses the operating system's proxy settings. The key does
  not cover name resolution: where the host name a plain-http request
  connects to — the `base_url`'s or, when a proxy applies, the proxy's — is
  looked up in DNS, resolver variables such as glibc's `HOSTALIASES`,
  `LOCALDOMAIN` and `RES_OPTIONS` can send one invocation's request, and so
  the reply cached for later ones, to another server. An HTTPS endpoint's
  certificate check rules that out, and a direct request to an IP address or
  to a name `/etc/hosts` answers, such as `localhost`, is not affected.
- **`aish run`** turns your prompt into a shell command and runs it after a
  confirm prompt. Read what is proposed before confirming. Both `--yes` and the
  global `--json` flag skip that prompt and run the command immediately, so
  pass neither when the input comes from an untrusted source; use `--print` to
  review the command without running it. (`aish fix` only runs the command you
  give it; it never executes the model's suggested fix.)
- **`AISH_PROVIDER=mock`** makes aish answer with `$AISH_MOCK_REPLY` instead
  of calling a provider. It is honoured in release builds too, for offline
  smoke checks, and replaces the model's reply for that invocation only: a
  cached mock reply is only served to a later run with the same
  `$AISH_MOCK_REPLY`, and mock replies never share cache entries with real
  providers.
- **`aish update`** downloads your platform's asset from this repository's
  GitHub releases over HTTPS and installs it once it starts with ELF or
  Mach-O magic bytes. That check only rejects error pages; there is no
  separate checksum or signature, so you are trusting GitHub's TLS and
  whoever can publish releases to `daaquan/aish` — which includes the
  third-party GitHub Actions the release workflow runs, since release assets
  are uploaded with `--clobber` and can be replaced in place. A checksum made
  by the same workflow would add no second trust root, so none is published.
  To build exactly the code you reviewed instead, pin the commit (tags can be
  moved) and its lockfile:
  `cargo install --locked --git https://github.com/daaquan/aish --rev <commit>`.

## Disclosure

We follow coordinated disclosure. Please give us 90 days before going public,
or sooner by mutual agreement once a fix ships.

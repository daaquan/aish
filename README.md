# aish

<!-- SPDX-License-Identifier: MIT -->

> **AI copilot for your command line.** aish wraps the commands developers run
> every day — commit, PR, review, changelog, ask — and uses a model to draft
> clean summaries and troubleshoot output, all as built-in subcommands.
>
> See [`docs/adr/`](docs/adr/) for architecture decisions.

## Commit messages

`aish commit` generates a commit message from your staged diff and asks
before committing:

```bash
git add .
aish commit            # suggest a message, then [Y/n/e(dit)]
aish commit --apply    # generate and commit without prompting
aish commit --signoff  # add a DCO Signed-off-by trailer
```

Answering `e` opens `$EDITOR` on the suggestion and re-asks with the edited
message. Identical requests (same diff, model, style, language) are served
from a local cache without a model call (`--no-cache` to bypass).
Inspect or empty the cache with `aish cache stats` / `aish cache clear`
(`--yes` skips the prompt).

A config is created automatically on first run. Run `aish setup` for an
interactive wizard that prompts for provider API keys (stored as plaintext or
`${ENV_VAR}` references) and a default model, or `aish setup --repair` to
restore the template (backing up any existing file). Configure style, language,
and model alias in `~/.aish/config.yaml`:

```yaml
commit:
  style: conventional
  language: en
  model: default
```

## Pull requests

`aish pr` generates a PR title and description from the commits and diff of
your branch against the default branch, and asks before creating the PR via
[`gh`](https://cli.github.com):

```bash
aish pr                # suggest title/body, then [Y/n/e(dit)]
aish pr --apply        # generate and run `gh pr create` without prompting
aish pr --base develop # diff against a specific base branch
```

Like `aish commit`, answering `e` opens `$EDITOR` (first line = title, rest =
body), responses are cached (`--no-cache` to bypass), and `--model` / `--lang`
override the config.

## Code review

`aish review` sends a diff to the model and prints findings grouped by
severity (CRITICAL/HIGH/MEDIUM/LOW):

```bash
git add .
aish review                 # review the staged diff
aish review --branch        # review the branch diff against the default branch
aish review --base develop  # diff against a specific base branch
aish review --json          # machine-readable findings for CI
```

## Changelog entries

`aish changelog` summarizes commits between two refs into Keep-a-Changelog
style entries (Added/Changed/Fixed/Removed), ready to paste into a release:

```bash
aish changelog                  # latest tag .. HEAD
aish changelog --from v0.4.0    # explicit range start
aish changelog --from v0.4.0 --to v0.5.0
```

## One-shot questions

`aish ask` answers a single question; piped stdin becomes context:

```bash
aish ask "what does EXDEV mean?"
cargo build 2>&1 | aish ask "explain this error"
```

Piped input is capped at 12k chars. Identical question+context pairs are
served from the cache (`--no-cache` to bypass).

## Troubleshooting failures

`aish fix` runs a command, streams its output through, and — when it exits
nonzero — appends a diagnosis and a suggested fix:

```bash
aish fix cargo build              # diagnose only when the build fails
aish fix npm test                 # works with any command
aish fix --shell "make 2>&1 | tail"   # run via `sh -c` for pipes/redirects
aish fix --always ./deploy.sh     # explain even on success
```

It **diagnoses and suggests; it never edits files or re-runs the command**,
and it always exits with the wrapped command's exit code, so it is safe in
scripts and `&&` chains. On success (without `--always`) it is a transparent
pass-through and makes no model request. Command output is tail-capped at
12k chars before being sent (the failure usually lives at the end).

## Natural language to a command

`aish run` is the inverse of `fix`: describe what you want, and the model
emits a single shell command, shown behind a confirm/edit gate before it runs:

```bash
aish run delete all merged git branches   # show command, then [Y/n/e(dit)]
aish run --print compress the logs folder # print the command, do not run it
aish run --yes restart the dev server     # skip the prompt and run immediately
```

Generating and running an arbitrary shell command has a high blast radius, so
the command **never executes without an explicit gate**: the confirm prompt is
the default, `--yes` is the only path to no-prompt execution, and `--print`
emits the command without running it. The command runs via `sh -c` (so pipes,
globs, and `&&` work) and `aish run` propagates its exit code; aborting or
`--print` exits 0.

### JSON output (CI/CD)

The global `--json` flag makes built-in commands emit machine-readable JSON on
stdout instead of human text. `config check --json` still exits nonzero on
errors, so it works as a pipeline gate.

```bash
aish config check --json        # {"ok":true|false,"issues":[...]} ; nonzero exit on errors
aish config check --ping        # also send one real request per provider (auth/network gate)
aish usage --json               # {"by_model":{...},"total":{...}}
```

> **Testing:** setting `AISH_PROVIDER=mock` returns a canned message
> (`$AISH_MOCK_REPLY`) without calling any provider — used by the test suite
> and useful for offline/CI smoke checks.

## Shell completions

```bash
aish completions zsh  > "${fpath[1]}/_aish"                       # zsh
aish completions bash > /etc/bash_completion.d/aish               # bash
aish completions fish > ~/.config/fish/completions/aish.fish     # fish
```

## Updating & uninstalling

```bash
aish update              # self-update to the latest GitHub release
aish update --check      # report only; nonzero exit when outdated (CI gate)
aish update --version 0.5.0   # pin a specific release tag

aish uninstall           # remove the binary (asks first; keeps ~/.aish)
aish uninstall --purge   # also delete ~/.aish (config, cache, audit log)
aish uninstall --yes     # skip the confirmation prompt
```

Binaries installed via `cargo install` are detected and left alone — use
`cargo install aish` / `cargo uninstall aish` there instead.

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md) and the [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).
Contributions are welcome — no CLA or sign-off required.

## License

Licensed under the **MIT License** (`MIT`). See [`LICENSE`](LICENSE) for the full text.

aish is free software: you may use, modify, and redistribute it — including in
closed-source and commercial products — provided the copyright notice and license
text are preserved.

Copyright © 2026 daaquan.

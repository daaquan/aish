# aish

<!-- SPDX-License-Identifier: MIT -->

> See [`docs/superpowers/specs/`](docs/superpowers/specs/) for design specs and
> [`docs/adr/`](docs/adr/) for architecture decisions.

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

Configure style, language, and model alias in `~/.aish/config.yaml`
(`aish config init` writes a commented template):

```yaml
commit:
  style: conventional
  language: en
  model: default
```

### JSON output (CI/CD)

The global `--json` flag makes built-in commands emit machine-readable JSON on
stdout instead of human text. `config check --json` still exits nonzero on
errors, so it works as a pipeline gate.

```bash
aish config check --json        # {"ok":true|false,"issues":[...]} ; nonzero exit on errors
aish usage --json               # {"by_model":{...},"total":{...}}
```

> **Testing:** setting `AISH_PROVIDER=mock` returns a canned message
> (`$AISH_MOCK_REPLY`) without calling any provider — used by the test suite
> and useful for offline/CI smoke checks.

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

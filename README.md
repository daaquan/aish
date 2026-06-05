# aish

<!-- SPDX-License-Identifier: MIT -->

> See [`docs/superpowers/specs/`](docs/superpowers/specs/) for design specs.

## Plugins

aish ships no tools by default. Install them from the plugin registry:

```bash
aish plugin install commit     # build + install the commit plugin
aish plugin update commit      # rebuild + reinstall (omit name for all)
aish plugin list               # show installed plugins + state
aish plugin disable commit     # turn it off without uninstalling
aish plugin enable commit
aish plugin uninstall commit
```

Once installed:

```bash
git add .
aish commit            # suggest a message, then [Y/n]
aish commit --apply    # generate and commit without prompting
```

Plugins are trusted native executables built from source on install. The default
registry is `git@github.com:daaquan/aish-plugins.git` (override with
`AISH_REGISTRY`). See the [plugin system design](docs/superpowers/specs/2026-06-05-plugin-system-design.md)
for the stdio ABI.

Even for trusted plugins the host guards the boundary: each plugin runs under
per-phase timeouts and is SIGKILLed if it overstays, its stderr is drained into a
bounded buffer, oversized protocol frames are rejected as they are read, and
non-UTF-8 arguments are refused rather than forwarded.

### JSON output (CI/CD)

The global `--json` flag makes built-in commands emit machine-readable JSON on
stdout instead of human text. `config check --json` still exits nonzero on
errors, so it works as a pipeline gate.

```bash
aish config check --json        # {"ok":true|false,"issues":[...]} ; nonzero exit on errors
aish usage --json               # {"by_model":{...},"total":{...}}
```

> **Testing:** setting `AISH_PROVIDER=mock` makes the host's `model.chat` service
> return a canned message (`$AISH_MOCK_REPLY`) without calling any provider —
> used by the test suite and useful for offline/CI smoke checks.

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md) and the [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).
Contributions are welcome — no CLA or sign-off required.

## License

Licensed under the **MIT License** (`MIT`). See [`LICENSE`](LICENSE) for the full text.

aish is free software: you may use, modify, and redistribute it — including in
closed-source and commercial products — provided the copyright notice and license
text are preserved.

Copyright © 2026 daaquan.

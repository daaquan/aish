# aish

<!-- SPDX-License-Identifier: AGPL-3.0-only -->

> Early scaffold. No application code yet — see [`docs/superpowers/specs/`](docs/superpowers/specs/) for design specs.

## Usage (v0.1)

```bash
# Generate a commit message from staged changes (interactive confirm)
aish commit

# Generate and apply immediately, with DCO sign-off
aish commit --apply --signoff
```

> **Testing:** setting `AISH_PROVIDER=mock` makes `aish commit` return a canned
> message (`$AISH_MOCK_REPLY`) without calling any provider — used by the test
> suite and useful for offline/CI smoke checks.

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md) and the [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).
All contributions require a DCO `Signed-off-by` line (`git commit -s`).

## License

Licensed under the **GNU Affero General Public License v3.0 only** (`AGPL-3.0-only`).
See [`LICENSE`](LICENSE) for the full text.

aish is free software: you may use, modify, and redistribute it under the terms of the
AGPL. Because of AGPL **§13**, if you run a modified version of aish to provide a service
over a network, you must also offer the complete corresponding source of your modified
version to its users.

Copyright © 2026 daaquan.

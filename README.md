# aish

<!-- SPDX-License-Identifier: MIT -->

> Early scaffold. No application code yet — see [`docs/superpowers/specs/`](docs/superpowers/specs/) for design specs.

## Usage (v0.1)

```bash
# Generate a commit message from staged changes (interactive confirm)
aish commit

# Generate and apply immediately
aish commit --apply
```

> **Testing:** setting `AISH_PROVIDER=mock` makes `aish commit` return a canned
> message (`$AISH_MOCK_REPLY`) without calling any provider — used by the test
> suite and useful for offline/CI smoke checks.

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md) and the [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).
Contributions are welcome — no CLA or sign-off required.

## License

Licensed under the **MIT License** (`MIT`). See [`LICENSE`](LICENSE) for the full text.

aish is free software: you may use, modify, and redistribute it — including in
closed-source and commercial products — provided the copyright notice and license
text are preserved.

Copyright © 2026 daaquan.

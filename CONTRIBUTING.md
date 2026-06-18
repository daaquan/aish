# Contributing to aish

Thanks for your interest in contributing! This document describes how we work.
By participating you agree to follow our [Code of Conduct](CODE_OF_CONDUCT.md).

All project communication, code, comments, commit messages, issues, and pull requests
are written in **English**.

## License of contributions

aish is licensed under the **MIT License**. By submitting a contribution you agree that
it is licensed under the same terms (inbound = outbound). No CLA or DCO sign-off is
required — a plain `git commit` is fine.

## Workflow

1. **Commit directly to `main`** for most work — this is a solo-maintainer project.
   For larger or riskier changes you may branch and open a PR, but branch naming is up
   to you and a single PR may cover several related changes.
2. **Commit messages** follow [Conventional Commits](https://www.conventionalcommits.org/):

   ```
   <type>: <description>

   <optional body>
   ```

   Allowed types: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `perf`, `ci`.

   Examples:
   - `feat: add interactive REPL loop`
   - `fix: escape backticks in command parser`
   - `docs: document the run subcommand`

3. **If you open a PR** against `main`, the description should:
   - Summarize the change set (`git diff main...HEAD` shows it all).
   - Explain the motivation.
   - Include a **test plan** (how you verified it; mark TODOs if any).
4. **Merge.** A PR merges once it has **green CI** (the `ci` workflow runs
   `cargo fmt --check`, `cargo clippy -D warnings`, and `cargo test --all`).
   Squash, merge, and rebase merges are all allowed — pick whatever keeps history clean.
   No approving review is required.

## Branch protection (maintainer setup)

`main` blocks force-pushes and deletion; everything else is open. Maintainers apply this
once. The GitHub branch-protection API expects a JSON body with nested objects, so pass
it via `--input` (dotted `-f` keys do **not** nest correctly):

```bash
gh api -X PUT repos/{owner}/{repo}/branches/main/protection --input - <<'JSON'
{
  "required_status_checks": null,
  "enforce_admins": false,
  "required_pull_request_reviews": null,
  "restrictions": null,
  "allow_force_pushes": false,
  "allow_deletions": false
}
JSON
```

To require green CI before merge, set `required_status_checks` to
`{ "strict": true, "contexts": ["build-test"] }` (the `ci` workflow's job).
It is left `null` above so admins can still merge if CI is unavailable.

## Development setup

A standard Rust toolchain (stable) is all you need.

- Build: `cargo build`
- Test: `cargo test --all`
- Lint: `cargo clippy --all-targets -- -D warnings && cargo fmt --all -- --check`
- Run a single test: `cargo test <name> -- --test-threads=1`

The test suite runs offline: set `AISH_PROVIDER=mock` to return a canned reply
(`$AISH_MOCK_REPLY`) without calling any provider.

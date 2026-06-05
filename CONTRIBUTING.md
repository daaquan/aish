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
   - `docs: document plugin API`

3. **If you open a PR** against `main`, the description should:
   - Summarize the change set (`git diff main...HEAD` shows it all).
   - Explain the motivation.
   - Include a **test plan** (how you verified it; mark TODOs if any).
4. **Merge.** A PR merges once it has **green CI** (once CI workflows exist).
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

`required_status_checks` is `null` until CI exists; once CI workflows are added, set it to
`{ "strict": true, "contexts": ["<job-name>"] }` to require green CI before merge.

## Development setup

_TBD — no build system exists yet. Build, lint, and test commands will be documented here
once application code lands. Do not assume commands that are not listed._

- Build: TBD
- Test: TBD
- Lint: TBD
- Run a single test: TBD

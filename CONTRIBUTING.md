# Contributing to aish

Thanks for your interest in contributing! This document describes how we work.
By participating you agree to follow our [Code of Conduct](CODE_OF_CONDUCT.md).

All project communication, code, comments, commit messages, issues, and pull requests
are written in **English**.

## License of contributions

aish is licensed under **AGPL-3.0-only**. By submitting a contribution you agree that it
is licensed under the same terms (inbound = outbound).

## Developer Certificate of Origin (DCO)

We use the [DCO](https://developercertificate.org/) instead of a CLA. Every commit must
carry a `Signed-off-by` line certifying that **you wrote the patch, or otherwise have the
right to submit it under the project's license**.

Add it automatically:

```bash
git commit -s -m "feat: add the thing"
```

This appends:

```
Signed-off-by: Your Name <your.email@example.com>
```

The name/email must match your `git config user.name` / `user.email`.

Forgot to sign? Fix the last commit:

```bash
git commit --amend -s --no-edit
```

Fix an entire branch:

```bash
git rebase --signoff main
```

If the branch was already pushed, update it with a lease-protected force-push:

```bash
git push --force-with-lease
```

A DCO check runs on every PR (see `.github/workflows/dco.yml`) and must pass before
merge. Because we squash-merge, maintainers ensure the final squash commit also keeps a
`Signed-off-by` line.

## Workflow

1. **Branch off `main`.** Never commit feature work directly to `main`.
   Name branches `type/short-desc`, e.g. `feat/repl-core`, `fix/prompt-escaping`.
2. **One feature per PR.** Keep each PR a single, self-contained unit of functionality.
   Do not bundle unrelated changes — split them into separate PRs.
3. **Commit messages** follow [Conventional Commits](https://www.conventionalcommits.org/):

   ```
   <type>: <description>

   <optional body>
   ```

   Allowed types: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `perf`, `ci`.

   Examples:
   - `feat: add interactive REPL loop`
   - `fix: escape backticks in command parser`
   - `docs: document plugin API`

4. **Open a PR** against `main`. The PR description must:
   - Summarize **all** commits in the branch, not just the latest
     (`git diff main...HEAD` shows the full change set).
   - Explain the motivation.
   - Include a **test plan** (how you verified it; mark TODOs if any).
5. **Merge.** A PR merges once it has **green CI** (once CI workflows exist).
   We **squash merge** so `main` keeps one commit per feature.
   Keep PRs small and reviewable.

## Branch protection (maintainer setup)

`main` is protected: squash-only merges and no force-pushes.
Maintainers apply this once. The GitHub branch-protection API expects a JSON body with
nested objects, so pass it via `--input` (dotted `-f` keys do **not** nest correctly):

```bash
gh api -X PUT repos/{owner}/{repo}/branches/main/protection --input - <<'JSON'
{
  "required_status_checks": null,
  "enforce_admins": true,
  "required_pull_request_reviews": null,
  "restrictions": null,
  "allow_force_pushes": false
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

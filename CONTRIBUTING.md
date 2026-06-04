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

A DCO check runs on every PR and must pass before merge.

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
5. **Review & merge.** A PR merges once it has **≥1 approving review** and **green CI**.
   We **squash merge** so `main` keeps one commit per feature. Keep PRs small and
   reviewable.

## Branch protection (maintainer setup)

`main` is protected: squash-only merges, ≥1 required review, required status checks, and
no force-pushes. Maintainers apply this once via:

```bash
gh api -X PUT repos/:owner/:repo/branches/main/protection \
  -f required_pull_request_reviews.required_approving_review_count=1 \
  -F enforce_admins=true \
  -F required_status_checks=null \
  -F restrictions=null \
  -F allow_force_pushes=false
```

(Adjust `required_status_checks` once CI exists.)

## Development setup

_TBD — no build system exists yet. Build, lint, and test commands will be documented here
once application code lands. Do not assume commands that are not listed._

- Build: TBD
- Test: TBD
- Lint: TBD
- Run a single test: TBD

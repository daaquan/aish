# Governance Foundation — Design Spec

**Date:** 2026-06-04
**Status:** Approved (pending written-spec review)
**Repo:** aish (early scaffold — no application code yet)

## Goal

Establish the project's operational and legal foundation before application code is
written: license, contribution workflow, code of conduct, and GitHub templates. All
project-facing text is in English.

## Decisions

| Topic | Decision |
|-------|----------|
| License | Apache-2.0 (permissive, OSI-approved, explicit patent grant) |
| Copyright | daaquan, 2026 |
| Contributor sign-off | DCO (`Signed-off-by`, via `git commit -s`) |
| Code of Conduct | Contributor Covenant 2.1 |
| CoC enforcement contact | daaquan@gmail.com |
| Merge policy | Squash merge, ≥1 approving review, green CI, protected `main` |
| Commit format | Conventional Commits (`feat/fix/refactor/docs/test/chore/perf/ci`) |
| Branch naming | `type/short-desc` (e.g. `feat/repl-core`) |
| PR scope | One feature per PR |
| Dependabot / CI workflows | Deferred — added with first code (ecosystem unknown) |

## Deliverables

1. **`LICENSE`** — full Apache-2.0 text.
2. **`NOTICE`** — copyright attribution line.
3. **`CONTRIBUTING.md`**
   - English-only policy.
   - Workflow: branch off `main` → `type/short-desc` → one feature/PR → ≥1 review +
     green CI → squash merge.
   - Conventional Commits spec + examples.
   - DCO: what it certifies, how to sign (`git commit -s`), fixing unsigned commits.
   - PR expectations: branch-wide summary (`git diff main...HEAD`) + test plan; small,
     scoped, reviewable.
   - Dev setup / build / test: `TBD` until code lands (no fabricated commands).
4. **`CODE_OF_CONDUCT.md`** — Contributor Covenant 2.1, contact = daaquan@gmail.com.
5. **`.github/PULL_REQUEST_TEMPLATE.md`** — summary / motivation / test plan / checklist
   (DCO signed, Conventional Commit title, one feature, docs updated).
6. **`.github/ISSUE_TEMPLATE/`** — `bug_report.md`, `feature_request.md`, `config.yml`.
7. **`CLAUDE.md`** (extend existing) — add: license=Apache-2.0, DCO sign-off requirement,
   branch naming, squash policy. Keep Build/Architecture sections `TBD`.
8. **Branch-protection note** — documented in `CONTRIBUTING.md` with the `gh api` command;
   applied manually by the maintainer (not automated here).

## Out of Scope (YAGNI)

- CI workflow files (no build system yet).
- `dependabot.yml` (no package ecosystem yet).
- CLA tooling, SECURITY.md, governance/maintainer hierarchy (solo project; revisit if
  community grows).

## Success Criteria

- Repo carries a valid Apache-2.0 license discoverable by GitHub.
- A new contributor can read `CONTRIBUTING.md` and produce a correctly-formatted,
  signed, single-feature PR without asking questions.
- `CLAUDE.md` reflects the same rules so future Claude sessions follow them.
- No fabricated build/test/architecture content anywhere.

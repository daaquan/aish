#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Publish a release asset to whichever forge `origin` points at: GitHub via the
# `gh` CLI, GitLab via `glab`. Both follow the same two-step flow — create the
# release if it does not exist yet, then upload the asset — so a tag can be cut
# from GitHub Actions or GitLab CI with one entrypoint.
#
# Usage:
#   publish-release.sh <tag> <asset>     # create release (idempotent) + upload
#   publish-release.sh --detect-forge <url>
#   publish-release.sh --self-test
#
# The forge is inferred from the origin remote. Set AISH_FORGE=github|gitlab to
# override (e.g. self-hosted GitLab whose host is not gitlab.com).
set -euo pipefail

detect_forge() {
  local url="${1:-}"
  case "${AISH_FORGE:-}" in
    github | gitlab)
      echo "$AISH_FORGE"
      return 0
      ;;
  esac
  case "$url" in
    *github.com*) echo github ;;
    *gitlab.com*) echo gitlab ;;
    *)
      echo "cannot infer forge from remote '$url'; set AISH_FORGE=github|gitlab" >&2
      return 1
      ;;
  esac
}

publish() {
  local forge="$1" tag="$2" asset="$3"
  case "$forge" in
    github)
      gh release create "$tag" --title "$tag" --generate-notes --verify-tag 2>/dev/null || true
      gh release upload "$tag" "$asset" --clobber
      ;;
    gitlab)
      # ponytail: glab has no --clobber; a re-run of an existing asset errors.
      # Re-uploads are rare (tag = immutable release) so we don't pre-delete.
      glab release create "$tag" --name "$tag" 2>/dev/null || true
      glab release upload "$tag" "$asset"
      ;;
    *)
      echo "unsupported forge: $forge" >&2
      return 1
      ;;
  esac
}

case "${1:-}" in
  --self-test)
    [ "$(detect_forge https://github.com/u/r)" = github ]
    [ "$(detect_forge git@github.com:u/r.git)" = github ]
    [ "$(detect_forge https://gitlab.com/u/r)" = gitlab ]
    [ "$(detect_forge git@gitlab.com:u/r.git)" = gitlab ]
    [ "$(AISH_FORGE=gitlab detect_forge https://git.example.com/u/r)" = gitlab ]
    if detect_forge https://example.com/u/r 2>/dev/null; then
      echo "expected unknown forge to fail" >&2
      exit 1
    fi
    echo ok
    ;;
  --detect-forge)
    detect_forge "${2:?usage: publish-release.sh --detect-forge <url>}"
    ;;
  *)
    tag="${1:?usage: publish-release.sh <tag> <asset>}"
    asset="${2:?usage: publish-release.sh <tag> <asset>}"
    remote="$(git remote get-url origin 2>/dev/null || true)"
    publish "$(detect_forge "$remote")" "$tag" "$asset"
    ;;
esac

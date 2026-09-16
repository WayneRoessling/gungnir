#!/usr/bin/env bash
# Copyright (C) 2026 Roessling Digital Solutions LLC
# SPDX-License-Identifier: AGPL-3.0-or-later
# Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md
#
# Re-run the cross-document checks of every open pull request against the main that
# just moved, and post the result on the pull request's head commit.
#
# A pull request's own checks ran against main as it was when the branch was last pushed.
# The document checks read documents other pull requests change -- build_uaf.py reads the
# gap register, the roadmap and the verification table -- so two pull requests can each
# pass and still leave main red once both merge, which happened on 2026-09-16 (#106 and
# #107). This puts the combination in front of whoever merges the second one.
#
# Only pull requests from branches of this repository are checked: the merged tree's
# generators run with a token that can write commit statuses, and a fork's code must not.
# Needs GH_TOKEN, GITHUB_REPOSITORY, GITHUB_SERVER_URL and GITHUB_RUN_ID, as Actions sets.
#
# To try it without touching the repository, set RECHECK_PRS to "<number> <sha>" lines,
# which replaces the listing, and RECHECK_DRY_RUN=1, which prints each status instead of
# posting it. Both matter: a trial run that reaches the real `gh` posts real statuses.
set -uo pipefail

context="cross-document / against current main"
main_sha=$(git rev-parse HEAD)
run_url="${GITHUB_SERVER_URL:-}/${GITHUB_REPOSITORY:-}/actions/runs/${GITHUB_RUN_ID:-}"
if [ -n "${RECHECK_PRS:-}" ]; then
  prs="$RECHECK_PRS"
else
  prs=$(gh pr list --state open --base main --limit 100 \
    --json number,headRefOid,isCrossRepository \
    --jq '.[] | select(.isCrossRepository | not) | "\(.number) \(.headRefOid)"')
fi
if [ -z "$prs" ]; then
  echo "no open pull requests from branches of this repository"
  exit 0
fi

# The merge identity is passed per command, never written with `git config`: a local
# trial runs in a worktree, and worktrees share the repository's configuration.
identity=(-c user.name=cross-document-recheck -c user.email=cross-document-recheck@invalid)
summary=()
while read -r number head; do
  echo "::group::#$number ($head) with main ${main_sha:0:7}"
  dir=$(mktemp -d)
  git fetch -q origin "pull/$number/head" || echo "could not fetch pull/$number/head; using $head as it is"
  git worktree add -q --detach "$dir" "$main_sha"
  if ! git "${identity[@]}" -C "$dir" merge -q --no-edit "$head"; then
    state=error
    description="does not merge cleanly with main ${main_sha:0:7}"
  elif (cd "$dir" && BASE_REF="$main_sha" bash .github/scripts/check_documents.sh cross); then
    state=success
    description="document checks pass merged with main ${main_sha:0:7}"
  else
    state=failure
    description="document checks fail merged with main ${main_sha:0:7}; merge main to see why"
  fi
  git worktree remove --force "$dir"
  echo "::endgroup::"
  if [ -n "${RECHECK_DRY_RUN:-}" ]; then
    echo "dry run: would post $state on $head: $description"
  else
    gh api -X POST "repos/$GITHUB_REPOSITORY/statuses/$head" \
      -f state="$state" -f context="$context" \
      -f description="$description" -f target_url="$run_url" >/dev/null
  fi
  summary+=("#$number: $state -- $description")
done <<< "$prs"
printf '%s\n' "${summary[@]}"

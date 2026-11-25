#!/usr/bin/env bash
# Merge upstream OpenPosterDB into a fork sync branch (fork tooling, see docs/louk/FORK.md).
#
#   scripts/louk/sync-upstream.sh [--local-tests] [--no-push] [<upstream-tag-or-branch>]
#
# What it does:
#   1. fetches `upstream` (github.com/PNRxA/openposterdb) and `origin` (the fork on Forgejo);
#   2. if the target (default `upstream/main`, or the tag/branch you give) is already contained in
#      `origin/main`, says "already up to date" and stops without touching anything;
#   3. otherwise creates `louk/sync-<date>` from `origin/main` and merges the target into it;
#   4. on conflicts it stops, lists the conflicted files and points at the divergence table in
#      docs/louk/FORK.md (the merge is left in progress so you can resolve it);
#   5. on a clean merge it pushes the branch to `origin`. Forgejo mirrors the push to GitHub within
#      about 30 s, where GitHub Actions runs the `Tests` workflow on every branch push;
#   6. prints where to watch CI and the next steps.
#
# It never touches `main`: it only creates a new branch and never pushes anything else.
#
# TESTS RUN ON GITHUB ACTIONS, NOT ON THIS MACHINE. On the dev box (a 4-8 GB slice shared with a
# streaming server) a cargo build or a vitest run can stall everything, so do not run them casually.
# `--local-tests` is the exception for a person who accepts that: it runs, one after the other,
# `timeout 60m nice -n 19 cargo test -j1` in api/ and `timeout 30m nice -n 19 npx vitest run` in web/.
# Check `grep full /sys/fs/cgroup/user.slice/user-*/memory.pressure` first and do not run it
# next to another heavy job.
#
# `--no-push` stops after the merge (for a dry run). Environment: GITHUB_REPO (default
# l0unk/openposterdb) only changes the printed CI URL.
set -euo pipefail

GITHUB_REPO="${GITHUB_REPO:-l0unk/openposterdb}"
UPSTREAM_REMOTE=upstream
ORIGIN_REMOTE=origin
BASE_BRANCH=main

local_tests=0
push=1
target=""

usage() {
  sed -n '2,/^set -euo/p' "${BASH_SOURCE[0]}" | sed '$d' | sed 's/^# \{0,1\}//'
}

die() { echo "error: $*" >&2; exit 1; }

while [ $# -gt 0 ]; do
  case "$1" in
    --local-tests) local_tests=1 ;;
    --no-push) push=0 ;;
    -h|--help) usage; exit 0 ;;
    -*) die "unknown option: $1 (see --help)" ;;
    *)
      [ -z "$target" ] || die "only one upstream tag or branch can be given (got '$target' and '$1')"
      target="$1"
      ;;
  esac
  shift
done

git rev-parse --git-dir >/dev/null 2>&1 || die "run this inside the fork's git clone"
cd "$(git rev-parse --show-toplevel)"

git remote get-url "$UPSTREAM_REMOTE" >/dev/null 2>&1 \
  || die "no '$UPSTREAM_REMOTE' remote; add it: git remote add upstream https://github.com/PNRxA/openposterdb.git"
git remote get-url "$ORIGIN_REMOTE" >/dev/null 2>&1 || die "no '$ORIGIN_REMOTE' remote (the fork on Forgejo)"

if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
  die "the working tree has uncommitted changes; commit or stash them first (the script switches branches)"
fi
if [ -e "$(git rev-parse --git-dir)/MERGE_HEAD" ]; then
  die "a merge is already in progress; finish it (git commit) or abort it (git merge --abort) first"
fi

echo "==> fetching $UPSTREAM_REMOTE and $ORIGIN_REMOTE"
git fetch --tags "$UPSTREAM_REMOTE"
git fetch "$ORIGIN_REMOTE"

# Resolve the target to a commit. A bare name is tried as an upstream tag, then an upstream branch.
if [ -z "$target" ]; then
  target_ref="$UPSTREAM_REMOTE/$BASE_BRANCH"
  target_label="$target_ref"
elif git rev-parse -q --verify "refs/tags/$target^{commit}" >/dev/null; then
  target_ref="refs/tags/$target"
  target_label="tag $target"
elif git rev-parse -q --verify "refs/remotes/$UPSTREAM_REMOTE/$target^{commit}" >/dev/null; then
  target_ref="refs/remotes/$UPSTREAM_REMOTE/$target"
  target_label="$UPSTREAM_REMOTE/$target"
else
  die "'$target' is neither a tag nor a branch of $UPSTREAM_REMOTE"
fi
target_sha="$(git rev-parse "$target_ref^{commit}")"
base_ref="$ORIGIN_REMOTE/$BASE_BRANCH"
git rev-parse -q --verify "$base_ref^{commit}" >/dev/null || die "$base_ref not found; is the fork's main on $ORIGIN_REMOTE?"

echo "    target: $target_label ($target_sha)"
echo "    fork:   $base_ref ($(git rev-parse "$base_ref"))"

if git merge-base --is-ancestor "$target_sha" "$base_ref"; then
  echo
  echo "Already up to date: $target_label is contained in $base_ref. Nothing to merge."
  exit 0
fi

# Branch name: louk/sync-<date>, with -2, -3 ... when that name is taken locally or on origin.
date_part="$(date +%Y-%m-%d)"
branch="louk/sync-$date_part"
n=2
while git rev-parse -q --verify "refs/heads/$branch" >/dev/null \
   || git rev-parse -q --verify "refs/remotes/$ORIGIN_REMOTE/$branch" >/dev/null; do
  branch="louk/sync-$date_part-$n"
  n=$((n + 1))
done

echo
echo "==> creating $branch from $base_ref and merging $target_label"
git switch -c "$branch" "$base_ref" --no-track

merge_msg="Merge upstream ${target_label} into the fork

Upstream sync: brings the fork up to date with $target_label ($target_sha)."

if ! git merge --no-ff -m "$merge_msg" "$target_sha"; then
  echo
  echo "MERGE CONFLICTS. Conflicted files:" >&2
  git diff --name-only --diff-filter=U | sed 's/^/  /' >&2
  cat >&2 <<EOF

Nothing was pushed and main is untouched; you are on $branch with the merge in progress.
Each conflicted file is probably one of the fork's hooks: see the divergence table in
docs/louk/FORK.md (section "Divergence points") for what each hook does and how to keep it.
The usual fix keeps upstream's new code and re-adds the one-line hook marked with a louk-fork comment.

Then:
  git add <files> && git commit        # finish the merge
  git push -u $ORIGIN_REMOTE $branch   # CI runs on GitHub
or give up with:
  git merge --abort && git switch - && git branch -D $branch
EOF
  exit 2
fi

echo "    merged cleanly: $(git rev-parse --short HEAD)"

if [ "$local_tests" -eq 1 ]; then
  echo
  echo "==> --local-tests: cargo test -j1 (can take up to 60 min; one heavy job at a time on the dev box)"
  ( cd api && timeout 60m nice -n 19 cargo test -j1 )
  echo "==> --local-tests: vitest"
  ( cd web && timeout 30m nice -n 19 npx vitest run )
fi

if [ "$push" -eq 0 ]; then
  echo
  echo "--no-push: stopped after the merge. Push it with: git push -u $ORIGIN_REMOTE $branch"
  exit 0
fi

echo
echo "==> pushing $branch to $ORIGIN_REMOTE (Forgejo mirrors it to GitHub, where the tests run)"
git push -u "$ORIGIN_REMOTE" "$branch"

head_sha="$(git rev-parse HEAD)"
cat <<EOF

Done. Branch $branch is at $head_sha.

Watch CI (the mirror takes up to about 30 s to deliver the push):
  https://github.com/$GITHUB_REPO/actions?query=branch%3A$(printf '%s' "$branch" | sed 's|/|%2F|g')
  The run for this commit must be green: Backend tests, Frontend unit tests, E2E tests.
  CI is the test loop; do not run cargo on the dev box (or use --local-tests knowingly).

Next steps:
  1. Read the diff upstream brought in: git log --oneline $base_ref..$branch
     Check it against docs/louk/FORK.md: did upstream touch a file listed in the divergence table?
  2. When CI is green, merge $branch into main (a normal reviewed merge, never a force push).
  3. If you want it deployed, tag it from main: git tag -a v<upstream-version>-louk.1 and push the
     tag to $ORIGIN_REMOTE (Forgejo) only. See "Releases" in docs/louk/FORK.md.
EOF

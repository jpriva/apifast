#!/usr/bin/env sh
# Updates the apifast that install-local.sh installed to the newest version
# tagged in this repository (v<major.minor.patch>, made by tools/release.sh).
# The version is built in a git worktree of its own (target/update/), so the
# working copy, its branch and its changes are left as they are; then that
# version's own install-local.sh installs it, keeping the one it replaces
# (install-local.sh --rollback goes back).
#
#   packaging/linux/update.sh           # to the newest tag
#   packaging/linux/update.sh v0.2.0    # to that one, newer or older
#   packaging/linux/update.sh --check   # only say what is installed and what is newest
#
# PREFIX is where apifast is installed (default: ~/.local).
set -eu

cd "$(dirname "$0")/../.."
prefix=${PREFIX:-$HOME/.local}

# The remote's tags too, when there is a remote.
if [ -n "$(git remote)" ]; then
    git fetch --tags --quiet || echo "warning: could not fetch the tags; using the ones here" >&2
fi
newest=$(git tag -l 'v[0-9]*.[0-9]*.[0-9]*' --sort=-v:refname | head -1)
if [ -z "$newest" ]; then
    echo "there is no version tag (v<major.minor.patch>) yet: tools/release.sh makes them" >&2
    exit 1
fi
installed=$("$prefix/bin/apifast" --version 2>/dev/null | cut -d' ' -f2) || installed=""

if [ "${1:-}" = "--check" ]; then
    echo "installed: ${installed:-none} (in $prefix), newest: ${newest#v}"
    exit 0
fi
wanted=${1:-$newest}
git rev-parse -q --verify "refs/tags/$wanted" >/dev/null || {
    echo "there is no tag $wanted" >&2
    exit 1
}
if [ "${wanted#v}" = "$installed" ]; then
    echo "apifast $installed is installed: nothing to do"
    exit 0
fi
# Without a version asked for, only ever forward.
if [ $# -eq 0 ] && [ -n "$installed" ] \
    && [ "$(printf '%s\n%s\n' "$installed" "${newest#v}" | sort -V | tail -1)" = "$installed" ]; then
    echo "apifast $installed is newer than the newest tag, $newest: nothing to do"
    exit 0
fi

worktree=target/update/$wanted
rm -rf "$worktree"
git worktree prune
git worktree add --quiet --detach "$worktree" "$wanted"
trap 'git worktree remove --force "$worktree"' EXIT
# One build directory for every update, so each reuses the last one's work.
CARGO_TARGET_DIR="$PWD/target/update-build" PREFIX="$prefix" \
    "$worktree/packaging/linux/install-local.sh"

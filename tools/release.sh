#!/usr/bin/env sh
# Makes a new version of apifast: sets it in the workspace (Cargo.toml and
# Cargo.lock) and in the AppStream metainfo, commits that, and tags the
# commit v<version>. Nothing is pushed or published.
#
#   tools/release.sh 0.2.0
#
# The version is semantic (major.minor.patch) and must be newer than the
# current one; the working tree must be clean.
set -eu

cd "$(dirname "$0")/.."
new=${1:?usage: tools/release.sh <major.minor.patch>}
case "$new" in
    *[!0-9.]* | .* | *. | *..*) echo "not a version: $new (major.minor.patch)" >&2; exit 1 ;;
esac
[ "$(printf '%s' "$new" | tr -cd . | wc -c)" -eq 2 ] || { echo "not a version: $new (major.minor.patch)" >&2; exit 1; }

current=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)
newest=$(printf '%s\n%s\n' "$current" "$new" | sort -t. -k1,1n -k2,2n -k3,3n | tail -1)
if [ "$new" = "$current" ] || [ "$newest" != "$new" ]; then
    echo "$new is not newer than $current" >&2
    exit 1
fi
if [ -n "$(git status --porcelain)" ]; then
    echo "the working tree has changes: commit or stash them first" >&2
    exit 1
fi
if git rev-parse -q --verify "refs/tags/v$new" >/dev/null; then
    echo "the tag v$new already exists" >&2
    exit 1
fi

# The workspace's version, and the lock file's copies of it.
sed -i "/^\[workspace.package\]/,/^\[/s/^version = \".*\"/version = \"$new\"/" Cargo.toml
cargo update --workspace --offline --quiet
# A release entry in the metainfo, newest first, as AppStream wants.
metainfo=packaging/linux/com.tryxtek.apifast.metainfo.xml
sed -i "s|  <releases>|  <releases>\n    <release version=\"$new\" date=\"$(date +%Y-%m-%d)\"/>|" "$metainfo"

git add Cargo.toml Cargo.lock "$metainfo"
git commit -q -m "release: v$new"
git tag -a "v$new" -m "apifast $new"
echo "apifast $current -> $new: committed and tagged v$new (not pushed)"

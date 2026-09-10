#!/usr/bin/env sh
# Copy a release from the private source repo to the public mirror using the local `gh` login.
# Fallback for when the release workflow has no RELEASES_TOKEN secret.
#   scripts/mirror-release.sh v0.2.1
set -eu
TAG="${1:?usage: mirror-release.sh vX.Y.Z}"
SRC="jackcanon/OH-Hive-src"
# NOTE: this external repo must be renamed on GitHub (or a new one created) to
# jackcanon/hive-releases before this script will work — Jack needs to do this
# manually, it's not something editable from the monorepo.
DST="jackcanon/hive-releases"

tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
gh release download "$TAG" -R "$SRC" -D "$tmp"
if gh release view "$TAG" -R "$DST" >/dev/null 2>&1; then
  gh release upload "$TAG" -R "$DST" --clobber "$tmp"/*
else
  gh release create "$TAG" -R "$DST" --title "$TAG" --notes "Hive node (\`hive\`) and regional server (\`hive-server\`) binaries.

Install: \`curl -fsSL https://ohghive.com/install.sh | sh\` — then \`hive pair\`, \`hive work\`.

Mirrored from the source repo's $TAG release. Verify with SHA256SUMS." "$tmp"/*
fi
echo "→ https://github.com/$DST/releases/tag/$TAG"

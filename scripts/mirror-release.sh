#!/usr/bin/env sh
# Copy a release from the private source repo to the public mirror using the local `gh` login.
# Fallback for when the release workflow has no RELEASES_TOKEN secret.
#   scripts/mirror-release.sh v0.2.1
set -eu
TAG="${1:?usage: mirror-release.sh vX.Y.Z}"
SRC="jackcanon/OH-Hive-src"
# Public mirror repo -- this really is jackcanon/ohhive-releases (confirmed live: it has real
# releases v0.1.0-v0.4.0). A prior rebrand commit (e2b9b5b, 2026-09-10) had this pointed at
# jackcanon/hive-releases, a repo that has never existed, with a comment wrongly blaming a missing
# manual rename -- same root cause as the identical bug fixed in .github/workflows/release.yml.
DST="jackcanon/ohhive-releases"

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

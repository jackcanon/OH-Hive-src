#!/usr/bin/env sh
# OH Hive installer — puts `hive` (node) and `hive-server` (regional server) in ~/.local/bin.
#   curl -fsSL https://ohghive.com/install.sh | sh
#   HIVE_VERSION=v0.1.0 sh install.sh        # pin a version
# Then:  hive pair   →  hive work
set -eu

REPO="jackcanon/OH-Hive-src"
BIN_DIR="${HIVE_BIN_DIR:-$HOME/.local/bin}"
VERSION="${HIVE_VERSION:-latest}"

os=$(uname -s); arch=$(uname -m)
case "$os" in
  Darwin) case "$arch" in arm64) target=aarch64-apple-darwin ;; x86_64) target=x86_64-apple-darwin ;; esac ;;
  Linux)  case "$arch" in aarch64|arm64) target=aarch64-unknown-linux-musl ;; x86_64) target=x86_64-unknown-linux-musl ;; esac ;;
  *) echo "unsupported OS: $os (Windows: download the .zip from GitHub Releases)"; exit 1 ;;
esac
[ -n "${target:-}" ] || { echo "unsupported arch: $arch"; exit 1; }

if [ "$VERSION" = "latest" ]; then
  VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p')
  [ -n "$VERSION" ] || { echo "could not determine latest release (private repo? set HIVE_VERSION and GITHUB_TOKEN)"; exit 1; }
fi
V=${VERSION#v}
asset="ohhive-$V-$target.tar.gz"
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT

# Public repo: plain release URLs. Private repo (token set): resolve asset ids via the API.
fetch() { # $1 asset name, $2 dest
  if [ -n "${GITHUB_TOKEN:-}" ]; then
    id=$(curl -fsSL -H "Authorization: Bearer $GITHUB_TOKEN" "https://api.github.com/repos/$REPO/releases/tags/$VERSION" \
         | tr ',' '\n' | awk -v n="$1" '/"id":/ {id=$0} /"name":/ && index($0, "\"" n "\"") {print id; exit}' | sed 's/[^0-9]//g')
    [ -n "$id" ] || return 1
    curl -fsSL -H "Authorization: Bearer $GITHUB_TOKEN" -H "Accept: application/octet-stream" \
         "https://api.github.com/repos/$REPO/releases/assets/$id" -o "$2"
  else
    curl -fsSL "https://github.com/$REPO/releases/download/$VERSION/$1" -o "$2"
  fi
}

echo "→ downloading ohhive $VERSION for $target"
fetch "$asset" "$tmp/ohhive.tgz"
if fetch SHA256SUMS "$tmp/SHA256SUMS" 2>/dev/null; then
  want=$(grep " $asset\$" "$tmp/SHA256SUMS" | cut -d' ' -f1)
  have=$( (sha256sum "$tmp/ohhive.tgz" 2>/dev/null || shasum -a 256 "$tmp/ohhive.tgz") | cut -d' ' -f1)
  [ "$want" = "$have" ] || { echo "checksum mismatch"; exit 1; }
  echo "→ checksum ok"
fi
mkdir -p "$BIN_DIR"
tar -xzf "$tmp/ohhive.tgz" -C "$BIN_DIR"
chmod +x "$BIN_DIR/hive" "$BIN_DIR/hive-server"
echo "→ installed to $BIN_DIR"
case ":$PATH:" in *":$BIN_DIR:"*) ;; *) echo "   add to PATH:  export PATH=\"$BIN_DIR:\$PATH\"" ;; esac
echo
echo "Next:"
echo "  hive pair        # link this machine to your Hive account"
echo "  hive work        # start taking cards (Ctrl-C checks out)"

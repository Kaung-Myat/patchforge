#!/usr/bin/env bash
set -Eeuo pipefail

REPOSITORY="${PATCHFORGE_GITHUB_REPOSITORY:-Kaung-Myat/patchforge}"
VERSION="${PATCHFORGE_CLI_VERSION:-latest}"
INSTALL_DIR="${PATCHFORGE_CLI_INSTALL_DIR:-}"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64|Linux-amd64)
    ASSET="patchforge-linux-amd64.tar.gz"
    ;;
  *)
    echo "PatchForge CLI currently provides a prebuilt binary for Linux amd64 only." >&2
    exit 1
    ;;
esac

if [[ -z "$INSTALL_DIR" ]]; then
  if [[ "$(id -u)" -eq 0 ]]; then
    INSTALL_DIR="/usr/local/bin"
  else
    INSTALL_DIR="$HOME/.local/bin"
  fi
fi

if [[ "$VERSION" == "latest" ]]; then
  RELEASE_URL="https://github.com/$REPOSITORY/releases/latest/download"
elif [[ "$VERSION" =~ ^v[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]]; then
  RELEASE_URL="https://github.com/$REPOSITORY/releases/download/$VERSION"
else
  echo "PATCHFORGE_CLI_VERSION must be 'latest' or a semantic tag such as v0.4.1." >&2
  exit 1
fi

TEMP_DIR="$(mktemp -d)"
trap 'rm -rf -- "$TEMP_DIR"' EXIT

curl -fsSL "$RELEASE_URL/$ASSET" -o "$TEMP_DIR/$ASSET"
curl -fsSL "$RELEASE_URL/$ASSET.sha256" -o "$TEMP_DIR/$ASSET.sha256"
(
  cd "$TEMP_DIR"
  sha256sum --check "$ASSET.sha256"
)
tar -C "$TEMP_DIR" -xzf "$TEMP_DIR/$ASSET"

mkdir -p "$INSTALL_DIR"
install -m 0755 "$TEMP_DIR/patchforge-linux-amd64/patchforge" "$INSTALL_DIR/patchforge"

echo "PatchForge CLI installed: $INSTALL_DIR/patchforge"
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
  echo "Add $INSTALL_DIR to PATH before running patchforge."
fi

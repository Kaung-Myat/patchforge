#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPOSITORY_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"

PATCHFORGE_SERVER_URL="${PATCHFORGE_SERVER_URL:-https://104-207-92-53.sslip.io}"
PATCHFORGE_APP_ID="${PATCHFORGE_APP_ID:-03140f79-0046-4cff-bc98-dbb3db94f02a}"
PATCHFORGE_SSH_TARGET="${PATCHFORGE_SSH_TARGET:-kmt@104.207.92.53}"
PATCHFORGE_SSH_PORT="${PATCHFORGE_SSH_PORT:-22022}"
PATCHFORGE_REMOTE_STORAGE_DIR="${PATCHFORGE_REMOTE_STORAGE_DIR:-/opt/patchforge/storage}"
PATCHFORGE_TOKEN_FILE="${PATCHFORGE_TOKEN_FILE:-$HOME/.config/patchforge/vps-publisher-token}"

if [[ ! -f "$PATCHFORGE_TOKEN_FILE" ]]; then
  echo "Publisher token file not found: $PATCHFORGE_TOKEN_FILE" >&2
  exit 1
fi

TOKEN_MODE="$(stat -c '%a' "$PATCHFORGE_TOKEN_FILE")"
if [[ "$TOKEN_MODE" != "600" ]]; then
  echo "Publisher token file must have mode 600: $PATCHFORGE_TOKEN_FILE" >&2
  exit 1
fi

PATCHFORGE_TOKEN="$(<"$PATCHFORGE_TOKEN_FILE")"
if [[ -z "$PATCHFORGE_TOKEN" ]]; then
  echo "Publisher token file is empty" >&2
  exit 1
fi
export PATCHFORGE_TOKEN

exec cargo run --manifest-path "$REPOSITORY_ROOT/cli/Cargo.toml" -- publish \
  --server-url "$PATCHFORGE_SERVER_URL" \
  --app-id "$PATCHFORGE_APP_ID" \
  --ssh-target "$PATCHFORGE_SSH_TARGET" \
  --ssh-port "$PATCHFORGE_SSH_PORT" \
  --remote-storage-dir "$PATCHFORGE_REMOTE_STORAGE_DIR" \
  "$@"

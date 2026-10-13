#!/usr/bin/env bash
set -Eeuo pipefail

INSTALL_DIR="${PATCHFORGE_INSTALL_DIR:-/opt/patchforge}"
OUTPUT_FILE="$INSTALL_DIR/dokploy.env"
POSTGRES_VOLUME="${PATCHFORGE_POSTGRES_VOLUME:-patchforge_postgres_data}"
PRIVATE_KEY_FILE="$INSTALL_DIR/secrets/signing-private-key"
PUBLISHER_TOKEN_FILE="$INSTALL_DIR/secrets/publisher-token"
STORAGE_DIR="$INSTALL_DIR/storage"
SERVER_IMAGE="${PATCHFORGE_SERVER_IMAGE:-ghcr.io/kaung-myat/patchforge-server:latest}"
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

if [[ "${EUID}" -ne 0 ]]; then
  echo "Run with sudo: sudo env PATCHFORGE_DOMAIN=updates.example.com bash scripts/prepare-dokploy-install.sh" >&2
  exit 1
fi

for command in docker openssl sha256sum; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "Required command is missing: $command" >&2
    exit 1
  fi
done

DOMAIN="${PATCHFORGE_DOMAIN:-}"
if [[ -z "$DOMAIN" && -r "$OUTPUT_FILE" ]]; then
  DOMAIN="$(sed -n 's/^PATCHFORGE_DOMAIN=//p' "$OUTPUT_FILE" | tail -n 1)"
fi
if [[ -z "$DOMAIN" && -t 0 ]]; then
  read -r -p "PatchForge domain (for example updates.example.com): " DOMAIN
fi
if [[ ! "$DOMAIN" =~ ^[A-Za-z0-9][A-Za-z0-9.-]*$ ]]; then
  echo "Set PATCHFORGE_DOMAIN to a valid hostname." >&2
  exit 1
fi

install -d -m 0700 "$INSTALL_DIR/secrets" "$INSTALL_DIR/backups"
install -d -m 0750 -o 65532 -g 65532 "$STORAGE_DIR" "$STORAGE_DIR/patches"

FIRST_INSTALL=false
if [[ -f "$OUTPUT_FILE" ]]; then
  echo "Existing Dokploy configuration found; keeping its secrets."
  chmod 600 "$OUTPUT_FILE"
else
  FIRST_INSTALL=true
  DB_PASSWORD="$(openssl rand -hex 32)"
  PUBLISH_TOKEN="$(openssl rand -hex 32)"
  PUBLISH_HASH="$(printf '%s' "$PUBLISH_TOKEN" | sha256sum | cut -d' ' -f1)"
  KEY_ID="main-$(date -u +%Y%m%d)"

  umask 077
  openssl rand -base64 32 | tr -d '\r\n' >"$PRIVATE_KEY_FILE"
  printf '%s' "$PUBLISH_TOKEN" >"$PUBLISHER_TOKEN_FILE"
  chown 65532:65532 "$PRIVATE_KEY_FILE"
  chmod 400 "$PRIVATE_KEY_FILE"
  chmod 600 "$PUBLISHER_TOKEN_FILE"

  cat >"$OUTPUT_FILE" <<EOF
PATCHFORGE_DOMAIN=$DOMAIN
PATCHFORGE_DB_PASSWORD=$DB_PASSWORD
PATCHFORGE_KEY_ID=$KEY_ID
PATCHFORGE_PUBLISH_TOKEN_HASHES=$PUBLISH_HASH
PATCHFORGE_SERVER_IMAGE=$SERVER_IMAGE
PATCHFORGE_POSTGRES_VOLUME=$POSTGRES_VOLUME
PATCHFORGE_PRIVATE_KEY_FILE=$PRIVATE_KEY_FILE
PATCHFORGE_STORAGE_DIR=$STORAGE_DIR
PATCHFORGE_MAX_ARTIFACT_SIZE_BYTES=104857600
EOF
  chmod 600 "$OUTPUT_FILE"
  unset DB_PASSWORD PUBLISH_TOKEN PUBLISH_HASH
fi

docker volume inspect "$POSTGRES_VOLUME" >/dev/null 2>&1 || \
  docker volume create "$POSTGRES_VOLUME" >/dev/null

if [[ ! -s "$PRIVATE_KEY_FILE" ]]; then
  echo "Signing key is missing or empty: $PRIVATE_KEY_FILE" >&2
  exit 1
fi
if [[ ! -s "$PUBLISHER_TOKEN_FILE" ]]; then
  echo "Publisher token is missing or empty: $PUBLISHER_TOKEN_FILE" >&2
  exit 1
fi

cat >"$INSTALL_DIR/deployment-mode" <<'EOF'
dokploy
EOF
chmod 600 "$INSTALL_DIR/deployment-mode"

if [[ -f "$SCRIPT_DIR/patchforge-dokploy.sh" ]]; then
  install -m 0755 "$SCRIPT_DIR/patchforge-dokploy.sh" /usr/local/bin/patchforge-dokploy
fi

echo
echo "PatchForge Dokploy preparation completed."
echo "Environment file: $OUTPUT_FILE"
echo "PostgreSQL volume: $POSTGRES_VOLUME"
echo "Signing key: $PRIVATE_KEY_FILE"
echo "Publisher token: $PUBLISHER_TOKEN_FILE"
echo "Patch storage: $STORAGE_DIR"
echo "Domain: $DOMAIN"
echo
echo "Next: create a Dokploy Compose service from deploy/dokploy.compose.yaml"
echo "and paste the contents of $OUTPUT_FILE into its Environment tab."
if command -v patchforge-dokploy >/dev/null 2>&1; then
  echo "After deployment, verify it with: sudo patchforge-dokploy doctor"
fi
if [[ "$FIRST_INSTALL" == "true" ]]; then
  echo "Copy the publisher token to a secure publisher workstation after deployment."
fi

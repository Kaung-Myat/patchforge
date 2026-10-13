#!/usr/bin/env bash
set -Eeuo pipefail

INSTALL_DIR="${PATCHFORGE_INSTALL_DIR:-/opt/patchforge}"
CONFIG_FILE="$INSTALL_DIR/config.env"
OUTPUT_FILE="$INSTALL_DIR/dokploy.env"
POSTGRES_VOLUME="${PATCHFORGE_POSTGRES_VOLUME:-patchforge_postgres_data}"
PRIVATE_KEY_FILE="$INSTALL_DIR/secrets/signing-private-key"
STORAGE_DIR="$INSTALL_DIR/storage"
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

if [[ "${EUID}" -ne 0 ]]; then
  echo "Run with sudo: sudo bash scripts/prepare-dokploy-migration.sh" >&2
  exit 1
fi

for command in docker curl; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "Required command is missing: $command" >&2
    exit 1
  fi
done

if [[ ! -r "$CONFIG_FILE" ]]; then
  echo "Existing PatchForge configuration was not found: $CONFIG_FILE" >&2
  exit 1
fi
if [[ ! -r "$PRIVATE_KEY_FILE" ]]; then
  echo "Existing PatchForge signing key was not found: $PRIVATE_KEY_FILE" >&2
  exit 1
fi
if [[ ! -d "$STORAGE_DIR" ]]; then
  echo "Existing PatchForge storage was not found: $STORAGE_DIR" >&2
  exit 1
fi
if ! docker volume inspect "$POSTGRES_VOLUME" >/dev/null 2>&1; then
  echo "Existing PostgreSQL volume was not found: $POSTGRES_VOLUME" >&2
  exit 1
fi

set -a
# shellcheck disable=SC1090
source "$CONFIG_FILE"
set +a

for variable in \
  PATCHFORGE_DOMAIN \
  PATCHFORGE_DB_PASSWORD \
  PATCHFORGE_KEY_ID \
  PATCHFORGE_PUBLISH_TOKEN_HASHES \
  PATCHFORGE_SERVER_IMAGE; do
  if [[ -z "${!variable:-}" ]]; then
    echo "Missing $variable in $CONFIG_FILE" >&2
    exit 1
  fi
  if [[ "${!variable}" == *$'\n'* || "${!variable}" == *$'\r'* ]]; then
    echo "$variable contains an invalid newline" >&2
    exit 1
  fi
done

echo "Creating a database backup before migration..."
if command -v patchforge >/dev/null 2>&1; then
  patchforge backup
else
  echo "The patchforge management command is missing; create a verified database backup before continuing." >&2
  exit 1
fi

umask 077
cat >"$OUTPUT_FILE" <<EOF
PATCHFORGE_DOMAIN=$PATCHFORGE_DOMAIN
PATCHFORGE_DB_PASSWORD=$PATCHFORGE_DB_PASSWORD
PATCHFORGE_KEY_ID=$PATCHFORGE_KEY_ID
PATCHFORGE_PUBLISH_TOKEN_HASHES=$PATCHFORGE_PUBLISH_TOKEN_HASHES
PATCHFORGE_SERVER_IMAGE=$PATCHFORGE_SERVER_IMAGE
PATCHFORGE_POSTGRES_VOLUME=$POSTGRES_VOLUME
PATCHFORGE_PRIVATE_KEY_FILE=$PRIVATE_KEY_FILE
PATCHFORGE_STORAGE_DIR=$STORAGE_DIR
EOF
chmod 600 "$OUTPUT_FILE"

if [[ -f "$SCRIPT_DIR/patchforge-dokploy.sh" ]]; then
  install -m 0755 "$SCRIPT_DIR/patchforge-dokploy.sh" /usr/local/bin/patchforge-dokploy
fi

DOMAIN="${PATCHFORGE_DOMAIN:-}"
if [[ -n "$DOMAIN" ]] && ! getent ahostsv4 "$DOMAIN" | awk '{print $1}' | grep -Fxq "$(curl -4fsS https://api.ipify.org)"; then
  echo "Warning: $DOMAIN does not currently resolve to this VPS public IPv4 address." >&2
fi

echo
echo "Dokploy migration preparation completed."
echo "Environment file: $OUTPUT_FILE"
echo "PostgreSQL volume: $POSTGRES_VOLUME"
echo "Signing key: $PRIVATE_KEY_FILE"
echo "Patch storage: $STORAGE_DIR"
if [[ -n "$DOMAIN" ]]; then
  echo "Domain: $DOMAIN"
fi
echo
echo "No services were stopped. Follow docs/DOKPLOY.md for the controlled cutover."
if command -v patchforge-dokploy >/dev/null 2>&1; then
  echo "After deployment, verify it with: sudo patchforge-dokploy doctor"
fi

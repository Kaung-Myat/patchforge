#!/usr/bin/env bash
set -Eeuo pipefail

INSTALL_DIR="${PATCHFORGE_INSTALL_DIR:-/opt/patchforge}"
INSTALL_VERSION="${PATCHFORGE_INSTALL_VERSION:-main}"
REPOSITORY="${PATCHFORGE_REPOSITORY:-Kaung-Myat/patchforge}"
ASSET_BASE="https://raw.githubusercontent.com/$REPOSITORY/$INSTALL_VERSION"

if [[ "${EUID}" -ne 0 ]]; then
  echo "Run this installer as root: sudo bash install.sh" >&2
  exit 1
fi

if [[ ! -r /etc/os-release ]]; then
  echo "This installer supports Ubuntu 22.04 and 24.04." >&2
  exit 1
fi
# shellcheck disable=SC1091
. /etc/os-release
if [[ "${ID:-}" != "ubuntu" || ! "${VERSION_ID:-}" =~ ^(22\.04|24\.04)$ ]]; then
  echo "Unsupported operating system: ${PRETTY_NAME:-unknown}" >&2
  echo "PatchForge currently supports Ubuntu 22.04 and 24.04." >&2
  exit 1
fi
if [[ "$(dpkg --print-architecture)" != "amd64" ]]; then
  echo "PatchForge currently supports amd64 VPS hosts only." >&2
  exit 1
fi

install_docker() {
  if command -v docker >/dev/null && docker compose version >/dev/null 2>&1; then
    return
  fi

  echo "Installing Docker Engine and Compose..."
  apt-get update
  apt-get install -y ca-certificates curl gnupg openssl jq
  install -m 0755 -d /etc/apt/keyrings
  curl -fsSL https://download.docker.com/linux/ubuntu/gpg -o /etc/apt/keyrings/docker.asc
  chmod a+r /etc/apt/keyrings/docker.asc
  cat >/etc/apt/sources.list.d/docker.sources <<EOF
Types: deb
URIs: https://download.docker.com/linux/ubuntu
Suites: ${UBUNTU_CODENAME:-$VERSION_CODENAME}
Components: stable
Architectures: amd64
Signed-By: /etc/apt/keyrings/docker.asc
EOF
  apt-get update
  apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
}

install_docker
apt-get install -y curl openssl jq

mkdir -p "$INSTALL_DIR/secrets" "$INSTALL_DIR/backups"
install -d -m 0750 -o 65532 -g 65532 \
  "$INSTALL_DIR/storage" "$INSTALL_DIR/storage/patches"
chmod 700 "$INSTALL_DIR/secrets" "$INSTALL_DIR/backups"

CONFIG_FILE="$INSTALL_DIR/config.env"
FIRST_INSTALL=false
if [[ -f "$CONFIG_FILE" ]]; then
  echo "Existing PatchForge configuration found; keeping its secrets."
  set -a
  # shellcheck disable=SC1090
  source "$CONFIG_FILE"
  set +a
else
  FIRST_INSTALL=true
  DOMAIN="${PATCHFORGE_DOMAIN:-}"
  if [[ -z "$DOMAIN" && -t 0 ]]; then
    read -r -p "PatchForge domain (for example updates.example.com): " DOMAIN
  fi
  if [[ ! "$DOMAIN" =~ ^[A-Za-z0-9][A-Za-z0-9.-]*$ ]]; then
    echo "Set PATCHFORGE_DOMAIN to a valid domain before running non-interactively." >&2
    exit 1
  fi

  DB_PASSWORD="$(openssl rand -hex 32)"
  PUBLISH_TOKEN="$(openssl rand -hex 32)"
  PUBLISH_HASH="$(printf '%s' "$PUBLISH_TOKEN" | sha256sum | cut -d' ' -f1)"
  KEY_ID="main-$(date -u +%Y%m%d)"
  openssl rand -base64 32 | tr -d '\r\n' >"$INSTALL_DIR/secrets/signing-private-key"
  printf '%s' "$PUBLISH_TOKEN" >"$INSTALL_DIR/secrets/publisher-token"
  chown 65532:65532 "$INSTALL_DIR/secrets/signing-private-key"
  chmod 400 "$INSTALL_DIR/secrets/signing-private-key"
  chmod 600 "$INSTALL_DIR/secrets/publisher-token"

  SERVER_IMAGE_TAG="latest"
  if [[ "$INSTALL_VERSION" =~ ^v([0-9]+\.[0-9]+\.[0-9]+)$ ]]; then
    SERVER_IMAGE_TAG="${BASH_REMATCH[1]}"
  fi

  cat >"$CONFIG_FILE" <<EOF
PATCHFORGE_DOMAIN=$DOMAIN
PATCHFORGE_DB_PASSWORD=$DB_PASSWORD
PATCHFORGE_KEY_ID=$KEY_ID
PATCHFORGE_PUBLISH_TOKEN_HASHES=$PUBLISH_HASH
PATCHFORGE_SERVER_IMAGE=ghcr.io/kaung-myat/patchforge-server:$SERVER_IMAGE_TAG
EOF
  chmod 600 "$CONFIG_FILE"
  unset DB_PASSWORD PUBLISH_TOKEN PUBLISH_HASH
fi

echo "Downloading the deployment bundle..."
curl -fsSL "$ASSET_BASE/deploy/compose.yaml" -o "$INSTALL_DIR/compose.yaml"
curl -fsSL "$ASSET_BASE/deploy/Caddyfile" -o "$INSTALL_DIR/Caddyfile"
curl -fsSL "$ASSET_BASE/scripts/patchforge-admin.sh" -o /usr/local/bin/patchforge
chmod 644 "$INSTALL_DIR/compose.yaml" "$INSTALL_DIR/Caddyfile"
chmod 755 /usr/local/bin/patchforge

cd "$INSTALL_DIR"
docker compose --env-file config.env -f compose.yaml config --quiet
docker compose --env-file config.env -f compose.yaml pull
docker compose --env-file config.env -f compose.yaml up -d

set -a
# shellcheck disable=SC1090
source "$CONFIG_FILE"
set +a

echo "Waiting for HTTPS health check..."
HEALTHY=false
for _ in $(seq 1 60); do
  if curl --max-time 5 -fsS "https://$PATCHFORGE_DOMAIN/health" >/dev/null 2>&1; then
    HEALTHY=true
    break
  fi
  sleep 2
done
if [[ "$HEALTHY" != "true" ]]; then
  echo "PatchForge did not pass its HTTPS health check." >&2
  docker compose --env-file config.env -f compose.yaml ps >&2
  docker compose --env-file config.env -f compose.yaml logs --tail=80 server caddy >&2
  exit 1
fi

TOKEN="$(<"$INSTALL_DIR/secrets/publisher-token")"
KEY_JSON="$(curl -fsS -H "Authorization: Bearer $TOKEN" "https://$PATCHFORGE_DOMAIN/api/v1/signing-keys")"
PUBLIC_KEY="$(jq -er --arg key "$PATCHFORGE_KEY_ID" '.[] | select(.key_id == $key) | .public_key_base64' <<<"$KEY_JSON")"
unset TOKEN KEY_JSON

echo
echo "PatchForge installation completed."
echo "URL: https://$PATCHFORGE_DOMAIN"
echo "Signing key ID: $PATCHFORGE_KEY_ID"
echo "SDK public key: $PUBLIC_KEY"
echo "Publisher token file: $INSTALL_DIR/secrets/publisher-token"
echo "Retrieve publisher token: sudo patchforge publisher-token"
echo "Management: sudo patchforge status"
if [[ "$FIRST_INSTALL" == "true" ]]; then
  echo
  echo "Run the retrieval command once and copy the token to a secure password manager or publisher workstation."
  echo "The raw token will not be shown by this installer."
fi

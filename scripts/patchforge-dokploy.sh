#!/usr/bin/env bash
set -Eeuo pipefail

INSTALL_DIR="${PATCHFORGE_INSTALL_DIR:-/opt/patchforge}"
CONFIG_FILE="$INSTALL_DIR/dokploy.env"

if [[ "${EUID}" -ne 0 ]]; then
  echo "Run with sudo: sudo patchforge-dokploy $*" >&2
  exit 1
fi
if [[ ! -r "$CONFIG_FILE" ]]; then
  echo "Dokploy configuration was not found: $CONFIG_FILE" >&2
  exit 1
fi

set -a
# shellcheck disable=SC1090
source "$CONFIG_FILE"
set +a

container_for() {
  local role="$1"
  local container
  container="$(docker ps \
    --filter "label=dev.patchforge.role=$role" \
    --format '{{.ID}}' | head -n 1)"
  if [[ -z "$container" ]]; then
    echo "PatchForge $role container is not running." >&2
    return 1
  fi
  printf '%s' "$container"
}

show_status() {
  docker ps \
    --filter "label=dev.patchforge.stack=ota" \
    --format 'table {{.Names}}\t{{.Image}}\t{{.Status}}'
}

COMMAND="${1:-status}"
case "$COMMAND" in
  status)
    show_status
    echo
    curl -fsS "https://$PATCHFORGE_DOMAIN/health"
    echo
    ;;
  logs)
    ROLE="${2:-server}"
    if [[ "$ROLE" != "server" && "$ROLE" != "postgres" ]]; then
      echo "Logs are available for: server, postgres" >&2
      exit 2
    fi
    docker logs --tail=200 "$(container_for "$ROLE")"
    ;;
  backup)
    POSTGRES_CONTAINER="$(container_for postgres)"
    BACKUP="$INSTALL_DIR/backups/patchforge-$(date -u +%Y%m%dT%H%M%SZ).sql.gz"
    install -d -m 0700 "$INSTALL_DIR/backups"
    umask 077
    docker exec "$POSTGRES_CONTAINER" pg_dump -U patchforge patchforge | gzip -9 >"$BACKUP"
    test -s "$BACKUP"
    echo "Database backup created: $BACKUP"
    echo "Patch artifacts remain in: ${PATCHFORGE_STORAGE_DIR:-$INSTALL_DIR/storage}"
    ;;
  doctor)
    show_status
    container_for server >/dev/null
    container_for postgres >/dev/null
    curl -fsS "https://$PATCHFORGE_DOMAIN/health" >/dev/null
    UNAUTH_STATUS="$(curl -sS -o /dev/null -w '%{http_code}' \
      "https://$PATCHFORGE_DOMAIN/api/v1/apps")"
    if [[ "$UNAUTH_STATUS" != "401" ]]; then
      echo "Management authentication check failed: HTTP $UNAUTH_STATUS" >&2
      exit 1
    fi
    test "$(stat -c '%a' "$CONFIG_FILE")" = "600"
    test "$(stat -c '%a' "$INSTALL_DIR/secrets/publisher-token")" = "600"
    test -s "${PATCHFORGE_PRIVATE_KEY_FILE:-$INSTALL_DIR/secrets/signing-private-key}"
    echo "PatchForge Dokploy doctor: all checks passed"
    ;;
  *)
    echo "Usage: sudo patchforge-dokploy {status|logs [server|postgres]|backup|doctor}" >&2
    echo "Deployments, restarts, and image updates are managed from the Dokploy UI." >&2
    exit 2
    ;;
esac

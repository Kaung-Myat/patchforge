#!/usr/bin/env bash
set -euo pipefail

INSTALL_DIR="${PATCHFORGE_INSTALL_DIR:-/opt/patchforge}"
CONFIG_FILE="$INSTALL_DIR/config.env"
COMPOSE_FILE="$INSTALL_DIR/compose.yaml"

if [[ "${EUID}" -ne 0 ]]; then
  echo "Run with sudo: sudo patchforge $*" >&2
  exit 1
fi
if [[ ! -f "$CONFIG_FILE" || ! -f "$COMPOSE_FILE" ]]; then
  echo "PatchForge is not installed in $INSTALL_DIR" >&2
  exit 1
fi

compose() {
  docker compose --env-file "$CONFIG_FILE" -f "$COMPOSE_FILE" "$@"
}

show_publisher_token() {
  local token_file="$INSTALL_DIR/secrets/publisher-token"
  if [[ ! -s "$token_file" ]]; then
    echo "Publisher token is missing or empty: $token_file" >&2
    exit 1
  fi
  if [[ "$(stat -c '%a' "$token_file")" != "600" ]]; then
    echo "Publisher token must have mode 600: $token_file" >&2
    exit 1
  fi
  echo "Sensitive publisher credential follows. Store it securely and never commit it." >&2
  cat "$token_file"
  echo
}

set -a
# shellcheck disable=SC1090
source "$CONFIG_FILE"
set +a

COMMAND="${1:-status}"
case "$COMMAND" in
  status)
    compose ps
    echo
    curl -fsS "https://$PATCHFORGE_DOMAIN/health"
    echo
    ;;
  logs)
    shift || true
    compose logs --tail=200 "$@"
    ;;
  restart)
    compose restart
    ;;
  update)
    compose pull
    compose up -d
    curl --retry 20 --retry-delay 2 --retry-all-errors -fsS "https://$PATCHFORGE_DOMAIN/health"
    echo
    ;;
  backup)
    BACKUP="$INSTALL_DIR/backups/patchforge-$(date -u +%Y%m%dT%H%M%SZ).sql.gz"
    umask 077
    compose exec -T postgres pg_dump -U patchforge patchforge | gzip -9 >"$BACKUP"
    test -s "$BACKUP"
    echo "Database backup created: $BACKUP"
    echo "Patch artifacts remain in: $INSTALL_DIR/storage"
    ;;
  publisher-token)
    show_publisher_token
    ;;
  doctor)
    compose config --quiet
    compose ps
    compose exec -T server test -w /app/storage/patches
    curl -fsS "https://$PATCHFORGE_DOMAIN/health" >/dev/null
    UNAUTH_STATUS="$(curl -sS -o /dev/null -w '%{http_code}' "https://$PATCHFORGE_DOMAIN/api/v1/apps")"
    if [[ "$UNAUTH_STATUS" != "401" ]]; then
      echo "Management authentication check failed: HTTP $UNAUTH_STATUS" >&2
      exit 1
    fi
    test "$(stat -c '%a' "$CONFIG_FILE")" = "600"
    test "$(stat -c '%a' "$INSTALL_DIR/secrets/publisher-token")" = "600"
    echo "PatchForge doctor: all checks passed"
    ;;
  *)
    echo "Usage: sudo patchforge {status|logs [service]|restart|update|backup|doctor|publisher-token}" >&2
    exit 2
    ;;
esac

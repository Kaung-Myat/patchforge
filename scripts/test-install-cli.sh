#!/usr/bin/env bash
set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
TEST_ROOT="$(mktemp -d)"

cleanup() {
  rm -rf -- "${TEST_ROOT}"
}
trap cleanup EXIT

mkdir -p \
  "${TEST_ROOT}/asset/patchforge-linux-amd64" \
  "${TEST_ROOT}/fake-bin" \
  "${TEST_ROOT}/install-bin"

cat >"${TEST_ROOT}/asset/patchforge-linux-amd64/patchforge" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' 'patchforge installer test'
EOF
chmod 0755 "${TEST_ROOT}/asset/patchforge-linux-amd64/patchforge"

tar -C "${TEST_ROOT}/asset" \
  -czf "${TEST_ROOT}/patchforge-linux-amd64.tar.gz" \
  patchforge-linux-amd64
(
  cd "${TEST_ROOT}"
  sha256sum patchforge-linux-amd64.tar.gz \
    >patchforge-linux-amd64.tar.gz.sha256
)

cat >"${TEST_ROOT}/fake-bin/curl" <<'EOF'
#!/usr/bin/env bash
set -Eeuo pipefail

arguments=" $* "
for required in \
  " --fail " \
  " --show-error " \
  " --location " \
  " --retry 5 " \
  " --retry-all-errors " \
  " --retry-delay 2 "; do
  if [[ "${arguments}" != *"${required}"* ]]; then
    printf 'Missing curl retry argument: %s\n' "${required}" >&2
    exit 1
  fi
done

output=""
url=""
while (($# > 0)); do
  case "$1" in
    -o)
      output="$2"
      shift 2
      ;;
    http://* | https://*)
      url="$1"
      shift
      ;;
    *)
      shift
      ;;
  esac
done

case "${url}" in
  *.tar.gz.sha256)
    cp "${PATCHFORGE_INSTALLER_TEST_ROOT}/patchforge-linux-amd64.tar.gz.sha256" \
      "${output}"
    ;;
  *.tar.gz)
    cp "${PATCHFORGE_INSTALLER_TEST_ROOT}/patchforge-linux-amd64.tar.gz" \
      "${output}"
    ;;
  *)
    printf 'Unexpected installer URL: %s\n' "${url}" >&2
    exit 1
    ;;
esac
EOF
chmod 0755 "${TEST_ROOT}/fake-bin/curl"

PATH="${TEST_ROOT}/fake-bin:${PATH}" \
PATCHFORGE_INSTALLER_TEST_ROOT="${TEST_ROOT}" \
PATCHFORGE_CLI_VERSION=v0.6.4 \
PATCHFORGE_CLI_INSTALL_DIR="${TEST_ROOT}/install-bin" \
  bash "${SCRIPT_DIR}/install-cli.sh"

test "$("${TEST_ROOT}/install-bin/patchforge")" = \
  "patchforge installer test"

printf '%s\n' "PatchForge CLI installer test passed."

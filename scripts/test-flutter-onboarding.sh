#!/usr/bin/env bash
set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
SDK_PATH="${REPOSITORY_ROOT}/sdk/flutter/patchforge_sdk"
TEST_ROOT="$(mktemp -d)"
TEST_APP="${TEST_ROOT}/patchforge_onboarding"

cleanup() {
  rm -rf -- "${TEST_ROOT}"
}
trap cleanup EXIT

flutter create \
  --platforms=android \
  --org dev.patchforge \
  --project-name patchforge_onboarding \
  "${TEST_APP}" >/dev/null

(
  cd "${TEST_APP}"
  flutter pub add "patchforge_sdk:{path: ${SDK_PATH}}" >/dev/null
  flutter build apk \
    --release \
    --target-platform android-arm64 \
    --split-per-abi >/dev/null
)

MANIFEST="$(find \
  "${TEST_APP}/build/app/intermediates/packaged_manifests/release" \
  -name AndroidManifest.xml \
  -print \
  -quit)"
APK="${TEST_APP}/build/app/outputs/flutter-apk/app-arm64-v8a-release.apk"

test -n "${MANIFEST}"
test -s "${APK}"
grep -Fq 'android.permission.INTERNET' "${MANIFEST}"
grep -Fq 'dev.patchforge.patchforge_sdk.PatchForgeInitializerProvider' "${MANIFEST}"

printf '%s\n' "PatchForge clean Flutter onboarding test passed."
printf 'Release APK: %s\n' "${APK}"

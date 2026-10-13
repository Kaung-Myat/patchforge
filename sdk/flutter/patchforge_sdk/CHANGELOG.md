## 0.4.1

- Keep the Flutter runtime package version aligned with the PatchForge 0.4.1
  server and CLI release. Runtime behavior remains compatible with 0.4.0.

## 0.4.0

- Align the SDK release with the project initialization and config-driven
  publishing workflow in PatchForge CLI 0.4.0. Runtime APIs remain compatible.

## 0.3.0

- Align the SDK release with the authenticated HTTPS publishing protocol in
  PatchForge Server and CLI 0.3.0. Client runtime APIs remain compatible.

## 0.2.0

- Add stable persistent device identities for deterministic staged rollouts.
- Add `development`, `staging`, `beta`, and `stable` release channels.
- Verify manifest v2 signatures covering channel, rollout percentage, and the
  parent patch number while retaining manifest v1 compatibility.
- Reject explicitly revoked signing key IDs even when an old public key remains
  bundled during a rotation window.

## 0.1.1

- Send the Android device architecture during update checks and reject
  manifests for a different ABI.
- Fall back to the installed `libapp.so` ELF header when upgrading an app whose
  older native plugin does not yet expose architecture discovery.
- Validate explicit `base_patch_number` chains before downloading a delta.
- Normalize manifest artifact paths so both canonical `patches/...` paths and
  older `/artifacts/patches/...` paths resolve without duplicating the server's
  `/artifacts` route.
- Include the resolved URL in patch download HTTP errors.

## 0.0.1

- Verify signed PatchForge update manifests and downloaded artifacts.
- Apply BSDIFF40 delta patches in Dart and verify reconstructed binaries.
- Locate `libapp.so` from Android native libraries or installed APK splits.
- Stage reconstructed binaries with rollback-aware patch state.
- Activate staged Android AOT libraries before Flutter engine startup.
- Re-verify staged binaries natively and expose the loaded patch number.
- Confirm healthy patched boots automatically after the first rendered frame.
- Roll back automatically when the engine load or application health check fails.

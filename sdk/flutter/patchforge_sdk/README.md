# PatchForge SDK

Flutter client for checking signed PatchForge manifests, downloading delta
artifacts, reconstructing `libapp.so`, and staging verified updates.

## Delta update flow

`PatchForge.update()` performs these checks before changing patch state:

1. Validate the app, version, platform, patch number, and manifest version.
2. Verify the Ed25519 signature, including all delta metadata.
3. Download the patch and verify its byte length and SHA-256.
4. Locate the installed Android `libapp.so` and verify its base SHA-256.
5. Apply the BSDIFF40 patch in Dart.
6. Verify the reconstructed file's byte length and SHA-256.
7. Atomically save the reconstructed file and mark the patch as staged.

## Android runtime activation

Release builds automatically register an internal, non-exported Android
initializer. On the next process launch it:

1. Reads the staged patch before Flutter initializes.
2. Re-verifies the staged `libapp.so` size and SHA-256 natively.
3. Passes its canonical internal-storage path to Flutter using
   `--aot-shared-library-name`.
4. Marks the patch as `booting` and records the patch loaded by the engine.

Debug builds deliberately skip AOT activation. After the patched release app
has completed its health checks, call `markPatchSuccessful(patchNumber)`. The
SDK refuses to commit a patch unless that patch is loaded by the current
Flutter engine.

If another process starts while the patch is still `booting`, the initializer
treats the prior startup as failed, removes the pending binary, and restores
the previous patch before Flutter starts.

`PatchForge` automatically waits for the first Flutter frame and a five-second
stabilization period before committing the loaded patch. Applications can set
`bootHealthCheck` and `bootStabilizationDelay` in `PatchForgeConfig` for their
own database, authentication, or service readiness requirements. A failed or
throwing health check rolls the pending patch back.

## Configuration

```dart
final patchForge = PatchForge(
  config: PatchForgeConfig.fromEnvironment(),
);

final result = await patchForge.update();

if (result.status == PatchUpdateStatus.staged) {
  print('Reconstructed binary: ${result.file?.path}');
}
```

`patchforge release` and `patchforge patch` inject the server URL, app ID,
channel, signing key ID, and public key at compile time. They do not place the
publisher token in the application. Construct `PatchForgeConfig` directly only
for custom build systems or embedders.

On Android, the plugin finds an extracted `libapp.so` in the native library
directory or extracts it from the installed base/split APK. Other embedders can
provide the base artifact explicitly:

```dart
PatchForgeConfig(
  baseUrl: 'https://updates.example.com',
  appId: 'your-app-id',
  trustedPublicKeys: const {'main-2026': 'public-key'},
  baseArtifactProvider: (architecture) async => File('/path/to/libapp.so'),
)
```

The server's `architecture` must match one of Android's supported ABIs, and the
base binary must exactly match the signed `base_sha256` value.

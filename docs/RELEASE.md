# Release PatchForge

PatchForge uses matching semantic versions for the server, CLI, Flutter SDK,
and GHCR server image. Complete every check below before publishing a tag.

## 1. Verify locally

```bash
cargo fmt --manifest-path server/Cargo.toml --all -- --check
cargo test --manifest-path server/Cargo.toml --locked
cargo fmt --manifest-path cli/Cargo.toml --all -- --check
cargo test --manifest-path cli/Cargo.toml --locked

cd sdk/flutter/patchforge_sdk
flutter pub get
flutter analyze
flutter test
```

Also validate the Dokploy Compose file with non-secret placeholder values:

```bash
touch /tmp/patchforge-release-test-key
mkdir -p /tmp/patchforge-release-test-storage
PATCHFORGE_DOMAIN=updates.example.com \
PATCHFORGE_DB_PASSWORD=test-password \
PATCHFORGE_KEY_ID=test-key \
PATCHFORGE_PUBLISH_TOKEN_HASHES=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef \
PATCHFORGE_PRIVATE_KEY_FILE=/tmp/patchforge-release-test-key \
PATCHFORGE_STORAGE_DIR=/tmp/patchforge-release-test-storage \
docker compose -f deploy/dokploy.compose.yaml config --quiet
```

## 2. Confirm versions and release notes

For `v0.4.1`, these files must all declare `0.4.1`:

- `server/Cargo.toml`
- `cli/Cargo.toml`
- `sdk/flutter/patchforge_sdk/pubspec.yaml`
- `CHANGELOG.md`
- `sdk/flutter/patchforge_sdk/CHANGELOG.md`

Commit the release changes and wait for the GitHub Actions CI workflow to pass
on `main`.

## 3. Create the release tag

```bash
git tag -a v0.4.1 -m "PatchForge v0.4.1"
git push origin v0.4.1
```

The tag starts the `Publish server image` workflow. Wait until it publishes:

```text
ghcr.io/kaung-myat/patchforge-server:0.4.1
```

The separate `Publish CLI` workflow must also attach the CLI archive and its
SHA-256 file to the GitHub release. Do not update a production deployment or
advertise the CLI installer until both workflows succeed.

## 4. Upgrade the test VPS

Change `PATCHFORGE_SERVER_IMAGE` in the Dokploy Environment tab to the new
version, save, and deploy. Do not use the old standalone `patchforge update`
command for a Dokploy-managed installation.

Verify:

```bash
sudo patchforge-dokploy doctor
curl -fsS https://YOUR_PATCHFORGE_DOMAIN/health
```

Publish and boot one real Android patch before announcing the release.

## 5. Publish release notes

Create the GitHub Release from the same tag and copy the relevant section from
`CHANGELOG.md`. Include upgrade notes whenever configuration, storage, signing,
or manifest compatibility changes.

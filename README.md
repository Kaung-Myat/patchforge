# PatchForge

PatchForge is a self-hosted Flutter OTA proof of concept. It generates signed
`bsdiff` deltas for Android `libapp.so`, verifies reconstructed artifacts, and
serves update manifests to the Flutter SDK.

## Install on a dedicated VPS

The packaged installer currently supports Ubuntu 22.04/24.04 on amd64 with a
domain pointing to the VPS and inbound ports 80/443 open. Download and inspect
the installer, then run it as root:

```bash
curl -fsSLO https://raw.githubusercontent.com/Kaung-Myat/patchforge/main/scripts/install.sh
less install.sh
sudo bash install.sh
```

The installer adds Docker Engine and Compose when needed, generates the
database password, Ed25519 signing key, and publisher credential, downloads a
standalone production Compose bundle, starts the services, and verifies the
public HTTPS health endpoint. Existing secrets are preserved when the installer
is run again.

Manage an installed server with:

```bash
sudo patchforge status
sudo patchforge doctor
sudo patchforge logs server
sudo patchforge backup
sudo patchforge update
```

Runtime files live under `/opt/patchforge`. The raw publisher credential is
stored with mode `0600` at
`/opt/patchforge/secrets/publisher-token`; the API container receives only its
SHA-256 hash. Copy that token once to the secured publisher workstation. The
private signing key is mounted only into the API container as a Compose secret.

Server images are published to
`ghcr.io/kaung-myat/patchforge-server` by the GitHub Actions workflow whenever a
`v*` tag is pushed. Deployments use the `latest` tag by default; set
`PATCHFORGE_SERVER_IMAGE` in `/opt/patchforge/config.env` to pin a release.

## Publisher authentication

All management endpoints for apps, releases, and patches require an HTTP
Bearer publisher token. Device update checks and artifact downloads remain
public so installed applications do not contain publishing credentials.

The server stores only SHA-256 token hashes. Generate a random token and its
configuration hash without placing the raw token in the server environment:

```bash
export PATCHFORGE_TOKEN="$(openssl rand -hex 32)"
cargo run --manifest-path cli/Cargo.toml -- token-hash
```

Put the printed hash in `server/.env`:

```text
PATCHFORGE_PUBLISH_TOKEN_HASHES=<sha256-hash>
```

Keep `PATCHFORGE_TOKEN` in the publisher machine's environment or secret
manager. The CLI automatically sends it as a Bearer token and hides environment
values from help/error output.

Run the publisher CLI on the server host when possible. If publishing across a
network, terminate TLS in front of PatchForge; never transmit a Bearer token
over plain HTTP on an untrusted network.

For zero-downtime rotation, temporarily configure both hashes separated by a
comma, restart the server, move publishers to the new raw token, and then remove
the old hash:

```text
PATCHFORGE_PUBLISH_TOKEN_HASHES=<old-hash>,<new-hash>
```

## Automated publish workflow

The CLI `publish` command performs the full release pipeline:

1. Reads the release and selects the next patch number.
2. Accepts either APK files or extracted `libapp.so` files.
3. Extracts `lib/<architecture>/libapp.so` when APKs are supplied.
4. Creates a BSDIFF40 delta with `bsdiff`.
5. Reconstructs the target with `bspatch` and verifies its SHA-256 and size.
6. Stores the delta under the server's artifact storage directory without
   overwriting an existing file.
7. Creates the signed patch record with a canonical path relative to the
   server's `/artifacts` route and all delta metadata.
8. Fetches and validates the update manifest.
9. Downloads the published artifact and verifies its SHA-256 and size.

The publish command is intended to run on the self-hosted PatchForge server,
where the CLI has local access to the artifact storage directory. This keeps
artifact uploads off the unauthenticated public API.

Prerequisites: the PatchForge API and PostgreSQL must be running, and `bsdiff`
and `bspatch` must be installed.

Run from the repository root:

```bash
cargo run --manifest-path cli/Cargo.toml -- publish \
  --server-url http://127.0.0.1:8080 \
  --app-id <app-uuid> \
  --release-id <release-uuid> \
  --version 1.0.0 \
  --platform android \
  --old path/to/base.apk \
  --new path/to/target.apk \
  --architecture arm64-v8a \
  --storage-dir storage \
  --output path/to/retained-delta.patch
```

The command requires `PATCHFORGE_TOKEN` to be set. A missing or invalid token
is rejected before the CLI reads release management data or creates a patch.

`--patch-number` is optional. When omitted, the CLI uses the highest existing
patch number plus one. `--output` is also optional; the server artifact is
always retained under `<storage-dir>/patches`.

Use `cargo run --manifest-path cli/Cargo.toml -- publish --help` for every
available option.

### Publish directly to a remote VPS

The CLI can upload the generated artifact over SSH before it creates the signed
patch record. This keeps the artifact endpoint read-only while removing the
manual `scp` step:

```bash
scripts/publish-vps.sh \
  --release-id <release-uuid> \
  --version 1.0.1 \
  --platform android \
  --channel stable \
  --architecture arm64-v8a \
  --old path/to/baseline.apk \
  --new path/to/target.apk \
  --output path/to/retained.patch
```

The helper reads the raw publisher token from
`~/.config/patchforge/vps-publisher-token`, which must have mode `0600`. The
token is never copied into the repository or VPS environment; the server keeps
only its SHA-256 hash. Override `PATCHFORGE_SERVER_URL`, `PATCHFORGE_APP_ID`,
`PATCHFORGE_SSH_TARGET`, `PATCHFORGE_SSH_PORT`, or
`PATCHFORGE_REMOTE_STORAGE_DIR` when deploying another installation.

The remote storage path must be absolute and cannot contain spaces or parent
path components. SSH uses batch mode so publishing fails instead of prompting
for a password in CI.

## Architecture compatibility and patch chains

PatchForge supports `arm64-v8a`, `armeabi-v7a`, and `x86_64` as independent
delta chains. The SDK discovers the Android ABI and includes it in every update
check; the server never returns a patch belonging to another architecture. For
apps installed with an older native plugin, the SDK reads the `libapp.so` ELF
machine type so the architecture-aware SDK itself can still arrive over OTA.

Each release registers one immutable baseline SHA-256 and size per
architecture. Every patch also declares `base_patch_number`. The server accepts
the patch only when its base SHA-256 matches either that architecture's release
baseline (`base_patch_number = 0`) or the signed target SHA-256 of the declared
parent patch. This rejects gaps, wrong release binaries, and cross-architecture
patches before they can be published.

Patch numbers are unique per release and architecture, so the same logical
number can be published independently for each ABI. Run `publish` once for each
architecture, supplying APKs that contain the matching `libapp.so`:

```bash
# Repeat with armeabi-v7a and x86_64 artifacts as required.
cargo run --manifest-path cli/Cargo.toml -- publish \
  --server-url http://127.0.0.1:8080 \
  --app-id <app-uuid> \
  --release-id <release-uuid> \
  --version 1.0.0 \
  --old path/to/base.apk \
  --new path/to/target.apk \
  --architecture arm64-v8a \
  --storage-dir storage
```

The CLI automatically registers the first baseline, finds the correct parent
from the supplied base binary, selects the next architecture-specific patch
number, reconstructs the target, and validates the architecture-aware manifest.

## Release channels and staged rollouts

Releases belong to one of `development`, `staging`, `beta`, or `stable`.
Existing releases and SDK installations default to `stable`. Configure another
channel in `PatchForgeConfig(channel: 'beta', ...)` and create the matching
server release with the same channel.

New patches use signed manifest v2 by default. Its signature covers the channel,
rollout percentage, and parent patch number in addition to all v1 fields. The
SDK keeps a random installation ID locally and the server hashes that ID with
the patch identity to make rollout assignment stable across update checks.

Publish to a channel and a deterministic percentage with:

```bash
cargo run --manifest-path cli/Cargo.toml -- publish \
  --server-url http://127.0.0.1:8080 \
  --app-id <app-uuid> \
  --release-id <release-uuid> \
  --version 1.0.0 \
  --old path/to/base.apk \
  --new path/to/target.apk \
  --channel beta \
  --rollout-percentage 25 \
  --architecture arm64-v8a \
  --storage-dir storage
```

A 0% rollout is held back from every device; 100% reaches all devices. For an
installed SDK that only understands manifest v1, first publish a 100% v1 bridge
with `--manifest-version 1`, then publish subsequent patches as v2.

## Signing-key rotation and revocation

The server registers the configured Ed25519 public key at startup. Exactly one
key is `active`; previous keys become `retired`, and retired signatures remain
servable during a migration window. A `revoked` key can no longer authorize an
update manifest. The server refuses to start with a revoked active key and also
refuses to revoke the currently active key.

Rotate keys without locking out installed applications:

1. Generate the next key pair and keep its private key in a secret manager.
2. Add only the next public key to `trustedPublicKeys` and publish this bridge
   patch with the current key.
3. Confirm that devices have booted the bridge.
4. Change `PATCHFORGE_KEY_ID` and `PATCHFORGE_PRIVATE_KEY` to the next key and
   restart the server. This makes the prior key `retired`.
5. Publish and boot a patch signed by the next key.
6. After the migration window, revoke the retired key through the authenticated
   `POST /api/v1/signing-keys/{key_id}/revoke` endpoint and add that key ID to
   the SDK's `revokedKeyIds` in the new-key-signed patch.

Never revoke the old key before the trust bridge has reached the devices that
must continue receiving updates. Inspect registry state through the protected
`GET /api/v1/signing-keys` endpoint.

## Runtime secrets and audit trail

Production deployments should set `PATCHFORGE_PRIVATE_KEY_FILE` instead of
placing `PATCHFORGE_PRIVATE_KEY` in an environment file. The file setting takes
precedence when both exist, which prevents a stale `.env` value from silently
reactivating the wrong key. Mount the file read-only from the host or a runtime
secret manager and restrict host permissions to the service account.

The Compose definition in `infra/docker-compose.yml` mounts an external file as
the Docker secret `/run/secrets/patchforge_private_key`. Before starting it, set
these host environment variables without putting their values in the repository:

```bash
export PATCHFORGE_KEY_ID=rotation-2026
export PATCHFORGE_PRIVATE_KEY_FILE=/absolute/path/outside/repository/patchforge_private_key
export PATCHFORGE_PUBLISH_TOKEN_HASHES=<publisher-token-sha256>
export PATCHFORGE_RUNTIME_UID="$(id -u)"
export PATCHFORGE_RUNTIME_GID="$(id -g)"
docker compose -f infra/docker-compose.yml up --build -d
```

The secret file contains only the Base64-encoded 32-byte Ed25519 private key.
The server validates it at startup and refuses revoked keys. The Compose server
runs as `PATCHFORGE_RUNTIME_UID:PATCHFORGE_RUNTIME_GID` (both default to 1000),
which must own the mode-0600 secret file. This preserves least-privilege file
permissions instead of making the private key readable by every container user.

Authenticated management traffic is recorded in PostgreSQL without storing the
raw Bearer token. Audit rows contain a short token-hash fingerprint, HTTP
method, resource path, response status, and timestamp. Signing-key activation
at startup is recorded with the `system` actor. Read the newest events through:

```text
GET /api/v1/audit-events?limit=100
GET /api/v1/audit-events?limit=100&before_id=<oldest-seen-id>
```

The audit endpoint itself requires publisher authentication and supports limits
from 1 to 500.

## HTTPS and request protection

The production Compose stack exposes only Caddy on ports 80 and 443. The Rust
server remains on the internal Compose network, so clients cannot bypass proxy
TLS or spoof `X-Forwarded-For`. Set `PATCHFORGE_DOMAIN` to a DNS name that points
to the host, then start the full stack:

```bash
export PATCHFORGE_DOMAIN=updates.example.com
export PATCHFORGE_DB_PASSWORD=<random-hex-database-password>
docker compose \
  --env-file .env \
  -f infra/docker-compose.yml \
  -f infra/docker-compose.production.yml \
  up --build -d
```

Always include `docker-compose.production.yml` on an Internet-facing host. It
removes PostgreSQL's host port, replaces the development database password,
makes the API container filesystem read-only, and enables no-new-privileges.
The database password should contain random hexadecimal characters so it can be
embedded safely in the PostgreSQL connection URL.

Caddy obtains and renews public certificates automatically for a reachable
public domain and redirects HTTP to HTTPS. The default `localhost` configuration
uses Caddy's local certificate authority for development.

The server independently applies these protections:

- fixed-window per-client limits for update checks, artifact downloads, and
  authenticated management routes;
- `429 Too Many Requests` with `Retry-After: 60` after a limit is exceeded;
- a 1 MiB maximum body for management requests;
- configurable request timeout;
- `nosniff`, frame denial, no-referrer, permissions policy, and no-store
  response headers;
- HSTS when `PATCHFORGE_HTTPS_ENABLED=true`.

Configure limits through `PATCHFORGE_PUBLIC_RATE_LIMIT_PER_MINUTE`,
`PATCHFORGE_ARTIFACT_RATE_LIMIT_PER_MINUTE`,
`PATCHFORGE_MANAGEMENT_RATE_LIMIT_PER_MINUTE`, and
`PATCHFORGE_REQUEST_TIMEOUT_SECONDS`. Only enable `PATCHFORGE_TRUST_PROXY` when
the application port is inaccessible except through a trusted proxy. The
provided Compose stack sets both proxy trust and HTTPS mode safely; direct
`cargo run` keeps them disabled by default.

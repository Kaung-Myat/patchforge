# Deploy PatchForge with Dokploy

This guide covers both a new PatchForge installation and migration from the
standalone installer. Dokploy's Traefik proxy replaces the PatchForge Caddy
container.

## New installation

### 1. Prepare the persistent data and secrets

Install Dokploy using its official instructions, clone PatchForge on the VPS,
and run the preparation helper. Use an administrator account that can later
upload patch artifacts over SSH:

```bash
sudo git clone https://github.com/Kaung-Myat/patchforge.git /srv/patchforge
sudo chown -R "$USER":"$USER" /srv/patchforge
cd /srv/patchforge

sudo env \
  PATCHFORGE_DOMAIN=updates.example.com \
  PATCHFORGE_PUBLISH_USER="$USER" \
  PATCHFORGE_SERVER_IMAGE=ghcr.io/kaung-myat/patchforge-server:0.2.0 \
  bash scripts/prepare-dokploy-install.sh
```

For inexpensive testing without buying a domain, replace
`updates.example.com` with an IP-based hostname such as
`104-207-92-53.sslip.io`.

The helper is safe to run again: existing secrets are retained. It creates:

- `/opt/patchforge/dokploy.env` for the Dokploy Environment tab.
- `/opt/patchforge/secrets/signing-private-key` for manifest signing.
- `/opt/patchforge/secrets/publisher-token` for the publisher workstation.
- `/opt/patchforge/storage/patches` for immutable delta artifacts.
- The external `patchforge_postgres_data` Docker volume.
- `/usr/local/bin/patchforge-dokploy` for status, logs, backup, and diagnostics.

Never paste the raw publisher token into Dokploy. The API receives only its
SHA-256 hash from `dokploy.env`.

### 2. Create the Dokploy Compose service

In Dokploy:

1. Create a project named `PatchForge`.
2. Create a `Compose` service and select `Docker Compose`.
3. Use the Git repository `https://github.com/Kaung-Myat/patchforge`.
4. Select the release tag matching the server image, for example `v0.2.0`.
5. Set Compose Path to `./deploy/dokploy.compose.yaml`.
6. Paste the output of `sudo cat /opt/patchforge/dokploy.env` into the
   Environment tab and save it.
7. Leave the Domains tab empty when using `sslip.io`; the Compose file supplies
   the HTTPS Traefik route explicitly.
8. Deploy.

### 3. Verify and retain the recovery material

```bash
sudo patchforge-dokploy doctor
sudo patchforge-dokploy backup
curl -fsS https://updates.example.com/health
```

Copy the database backup, signing key, publisher token, and patch artifacts to
encrypted off-server storage. Copy the publisher token to the publisher
workstation with mode `0600`, then configure the Flutter SDK with the server
URL and public signing key.

Use Dokploy for deployments, restarts, and image updates. Do not use the
standalone `patchforge update` or `patchforge restart` commands.

### 4. Secure the Dokploy dashboard

Give the Dokploy panel a different hostname from the PatchForge API, verify its
HTTPS certificate, and only then remove public IP-and-port access:

```bash
docker service update \
  --publish-rm "published=3000,target=3000,mode=host" \
  dokploy
```

Never close port 3000 until the HTTPS dashboard has been tested in a new browser
session. Keep SSH access available for recovery.

## Migrate an existing standalone installation

The migration assumes the existing installation lives in `/opt/patchforge` and
the public hostname already resolves to the VPS, for example
`104-207-92-53.sslip.io`.

### Important limits

Dokploy officially recommends at least 2 GB of RAM and 30 GB of disk. A VPS
with 2 GB of RAM and 25 GB of disk is below the disk recommendation. Use the
prebuilt PatchForge GHCR image, do not build images on this VPS, keep the 2 GB
swap file enabled, and monitor free disk space.

Dokploy needs host ports 80, 443, and 3000. The current Caddy container already
owns 80 and 443, so the controlled cutover below includes a short outage.

### 1. Prepare and back up the existing installation

Run this from a fresh clone of the PatchForge repository on the VPS:

```bash
cd /srv/patchforge
sudo bash scripts/prepare-dokploy-migration.sh
sudo ls -lh /opt/patchforge/backups
```

The helper verifies the existing signing key, storage directory, and PostgreSQL
volume. It creates a database backup and writes the environment values required
by Dokploy to `/opt/patchforge/dokploy.env` with mode `0600`. It does not stop
the working service.

Copy the latest database backup and the entire `/opt/patchforge/secrets`
directory off the VPS before continuing. The signing key is required by devices
that already trust this installation.

### 2. Stop the standalone stack without deleting data

```bash
cd /opt/patchforge
sudo docker compose --env-file config.env -f compose.yaml down --remove-orphans
sudo docker volume inspect patchforge_postgres_data >/dev/null
sudo test -s /opt/patchforge/secrets/signing-private-key
sudo test -d /opt/patchforge/storage/patches
sudo ss -ltnp | grep -E ':(80|443|3000)\b' || true
```

Do not add `-v` to the `docker compose down` command. The external PostgreSQL
volume must remain intact.

### 3. Install Dokploy

Use the current command from Dokploy's official installation page rather than
copying an old command from this repository. After installation, open:

```text
http://104.207.92.53:3000
```

Create the Dokploy administrator immediately. Keep port 3000 restricted to your
own IP in the VPS firewall when possible.

### 4. Create the PatchForge Compose service

In Dokploy:

1. Create a project named `PatchForge`.
2. Create a `Compose` service and select `Docker Compose`.
3. Select the Git provider and repository
   `https://github.com/Kaung-Myat/patchforge`.
4. Select a release tag rather than `main` for production.
5. Set the Compose Path to `./deploy/dokploy.compose.yaml`.
6. Open the Environment tab and paste the contents of
   `/opt/patchforge/dokploy.env`.
7. Save, but do not deploy until the environment is configured.

Never commit `dokploy.env`, the publisher token, the database password, or the
signing key to Git.

### 5. Configure the sslip.io route

Do not create this hostname in Dokploy's Domains tab. Dokploy treats `sslip.io`
entries created through that UI as HTTP-only testing domains. PatchForge needs
HTTPS because publisher credentials and signed update traffic must never be
sent over plaintext HTTP.

The repository Compose file therefore contains explicit Traefik labels for the
HTTP-to-HTTPS redirect, Let's Encrypt certificate, and `server:8080` route. It
also connects only the API server to the external `dokploy-network`. Confirm
that the Environment tab contains:

```text
PATCHFORGE_DOMAIN=104-207-92-53.sslip.io
```

Leave the Domains tab empty and deploy the service. Do not expose PostgreSQL or
port 8080 through the host. If Let's Encrypt has temporarily rate-limited the
shared `sslip.io` zone, the deployment will run but HTTPS certificate issuance
will fail; keep the previous stack stopped and inspect the Traefik logs before
retrying or rolling back.

### 6. Verify the cutover

```bash
curl -fsS https://104-207-92-53.sslip.io/health
```

The expected response is:

```json
{"status":"ok"}
```

Then verify these items in the Dokploy UI:

- `postgres` is healthy.
- `server` is running without signing-key or database errors.
- Only Traefik exposes host ports 80 and 443.

From the secured publisher workstation, list the existing applications with the
same publisher token. Finally, run one real phone update check. Existing apps,
releases, patch records, signing keys, and patch artifacts must still be present.

### Roll back

If Dokploy deployment fails, stop or delete only the failed PatchForge Compose
service in Dokploy. Do not delete its external volume or the files under
`/opt/patchforge`. Make sure Dokploy/Traefik has released ports 80 and 443, then
restore the original stack:

```bash
cd /opt/patchforge
sudo docker compose --env-file config.env -f compose.yaml up -d
sudo patchforge doctor
```

Because both deployments use the same PostgreSQL volume, signing key, and
storage directory, never run the standalone and Dokploy PatchForge stacks at
the same time.

# Deploy PatchForge with Dokploy and sslip.io

This guide migrates an existing one-command PatchForge installation to
Dokploy without changing its database, signing identity, publisher credential,
or patch artifacts. Dokploy's Traefik proxy replaces the PatchForge Caddy
container.

The migration assumes the existing installation lives in `/opt/patchforge` and
the public hostname already resolves to the VPS, for example
`104-207-92-53.sslip.io`.

## Important limits

Dokploy officially recommends at least 2 GB of RAM and 30 GB of disk. A VPS
with 2 GB of RAM and 25 GB of disk is below the disk recommendation. Use the
prebuilt PatchForge GHCR image, do not build images on this VPS, keep the 2 GB
swap file enabled, and monitor free disk space.

Dokploy needs host ports 80, 443, and 3000. The current Caddy container already
owns 80 and 443, so the controlled cutover below includes a short outage.

## 1. Prepare and back up the existing installation

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

## 2. Stop the standalone stack without deleting data

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

## 3. Install Dokploy

Use the current command from Dokploy's official installation page rather than
copying an old command from this repository. After installation, open:

```text
http://104.207.92.53:3000
```

Create the Dokploy administrator immediately. Keep port 3000 restricted to your
own IP in the VPS firewall when possible.

## 4. Create the PatchForge Compose service

In Dokploy:

1. Create a project named `PatchForge`.
2. Create a `Compose` service and select `Docker Compose`.
3. Select the Git provider and repository
   `https://github.com/Kaung-Myat/patchforge`.
4. Select a release tag rather than `main` for production.
5. Set the Compose Path to `./deploy/dokploy.compose.yaml`.
6. Open the Environment tab and paste the contents of
   `/opt/patchforge/dokploy.env`.
7. Save, but do not deploy until the domain is configured.

Never commit `dokploy.env`, the publisher token, the database password, or the
signing key to Git.

## 5. Add the sslip.io domain

In the Compose service's Domains tab, add:

| Field | Value |
| --- | --- |
| Service | `server` |
| Host | `104-207-92-53.sslip.io` |
| Path | `/` |
| Container Port | `8080` |
| HTTPS | Enabled |
| Certificate | `letsencrypt` |

Leave Internal Path empty and Strip Path disabled. Dokploy injects the Traefik
labels and routing network when it deploys the Compose service.

Deploy the service. Do not expose PostgreSQL or port 8080 through the host.

## 6. Verify the cutover

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

## Roll back

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

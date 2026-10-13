# Changelog

## 0.4.0

- Add a self-contained web console at `/console/` for connecting with a
  publisher token, viewing server and storage status, creating apps and
  releases, and inspecting published patch chains.
- Add an authenticated `/api/v1/system` endpoint that reports public signing
  information, capabilities, storage limits, and aggregate deployment stats.
- Add strict browser security headers and validate console-created app and
  release inputs server-side.
- Add `patchforge init` to verify a self-hosted server, create or reuse an app,
  discover its active signing key, and generate a shareable project YAML file.
- Store publisher credentials outside the project under the user's protected
  configuration directory instead of placing raw tokens in project files.
- Add config-driven `patchforge release create`, `patchforge patch create`, and
  `patchforge patch release` workflows while retaining the low-level commands.
- Publish a checksummed Linux amd64 CLI archive with each GitHub release and add
  a standalone CLI installer.

## 0.3.0

- Add authenticated, streamed HTTPS patch uploads with server-side SHA-256 and
  size verification, atomic publication, safe retry handling, and path checks.
- Make HTTPS the default CLI publishing transport so publishers no longer need
  VPS SSH accounts or direct artifact-storage access.

## 0.2.0

- Add production-ready Dokploy deployment with Traefik HTTPS routing.
- Add fresh-install and migration preparation helpers that preserve signing,
  database, publisher, and artifact data.
- Add a Dokploy-aware status, backup, logs, and health-check command.
- Add architecture-specific patch chains and release baselines.
- Add release channels, deterministic staged rollouts, and signed manifest v2.
- Add signing-key registry, rotation, retirement, and revocation support.
- Add authenticated audit events and hardened reverse-proxy handling.
- Add automated remote-VPS publishing and end-to-end artifact verification.

## 0.1.2

- Add one-command standalone VPS installation and GHCR image publishing.
- Add publisher-token authentication and server-side token hashing.

## 0.1.0

- Initial self-hosted Flutter Android OTA proof of concept.

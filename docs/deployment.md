# Server Deployment — VPS + Docker Compose

The `modeler-server` Rust/Axum backend powers all collaboration: auth, projects,
files, Arena, research, and CRDT sync. Tauri desktop clients connect to it over
HTTP/WebSocket. AI provider keys live in each client, **not** on the server.

This guide covers: **VPS + Docker Compose**, **compute via host Docker socket**,
**no TLS** (plain HTTP/WS — internal/testing use).

> ⚠️ No-TLS caveat: the client only upgrades sync to `wss://` when the API URL
> is `https://`. Over plain HTTP, collaborative sync runs as cleartext `ws://`.
> Fine for a LAN or trial; do not put sensitive data on a public cleartext host.

---

## Architecture recap

```
Tauri client A ─┐
                ├─ HTTP/WS ─→  modeler-server (Docker) ─→ SQLite + files (volume)
Tauri client B ─┘                     │
                                      └─ bollard ─→ host Docker daemon
                                                     (runs modeler-python:latest
                                                      for code execution)
```

| Concern | Detail |
|---------|--------|
| DB | SQLite, migrations embedded (`include_str!`), auto-run on startup |
| Persistence | `DATA_DIR=/app/data` → named volume `modeler-data` (DB + uploads) |
| Config | env only: `JWT_SECRET`, `DATABASE_URL`, `DATA_DIR`, `PORT` |
| Port | listens `0.0.0.0:3001` |
| compute | drives host Docker via mounted `/var/run/docker.sock` |

---

## Prerequisites

On the VPS:

```bash
# Docker Engine + Compose v2
curl -fsSL https://get.docker.com | sh
docker compose version   # confirm v2

# Build the compute image ONCE on the host (used by code execution)
git clone <your-repo> mathmodel && cd mathmodel
docker build -t modeler-python:latest server/src/compute/
```

---

## First deploy

```bash
cd mathmodel
cp deploy/.env.example deploy/.env

# Generate a strong JWT secret and put it in deploy/.env
openssl rand -hex 32        # paste into JWT_SECRET=

docker compose -f deploy/docker-compose.yml --env-file deploy/.env up -d --build
docker compose -f deploy/docker-compose.yml logs -f server
```

Healthy log line: `Server running on port 3001`. Test from your machine:

```bash
curl http://<vps-ip>:3001/          # reachable check
```

Open the firewall for the port:

```bash
sudo ufw allow 3001/tcp             # if using ufw
```

---

## Building the Tauri client to point at this server

On your build machine (not the VPS):

```bash
NEXT_PUBLIC_API_URL=http://<vps-ip>:3001 npm run tauri build
```

Distribute the resulting installer. Without `NEXT_PUBLIC_API_URL`, the client
falls back to its embedded/local-server logic.

---

## Updating to a new version

```bash
cd mathmodel
git pull
docker compose -f deploy/docker-compose.yml --env-file deploy/.env up -d --build
```

The `modeler-data` volume persists across rebuilds — DB and uploads are kept.
Schema migrations re-run automatically and are idempotent.

---

## Backup & restore

Everything lives in the `modeler-data` volume.

```bash
# Backup
docker run --rm -v modeler-data:/data -v "$PWD":/backup busybox \
  tar czf /backup/modeler-backup-$(date +%F).tar.gz -C /data .

# Restore (server stopped)
docker compose -f deploy/docker-compose.yml down
docker run --rm -v modeler-data:/data -v "$PWD":/backup busybox \
  sh -c "rm -rf /data/* && tar xzf /backup/modeler-backup-YYYY-MM-DD.tar.gz -C /data"
docker compose -f deploy/docker-compose.yml --env-file deploy/.env up -d
```

---

## Security notes

- **`JWT_SECRET`** — must be overridden; the code default is insecure.
- **`docker.sock` mount** — grants the server (and any executed user code)
  root-equivalent access to the host. Keep on a trusted network; the compute
  executor uses ephemeral containers + resource limits, but this is not a
  substitute for sandboxing untrusted users.
- **No TLS** — put behind a reverse proxy (Caddy/Nginx) before public exposure;
  see `docs/deployment-tls.md` (add when TLS is needed).
- **CORS** is `Any` — acceptable for desktop clients (no browser same-origin).

---

## Troubleshooting

| Symptom | Cause / fix |
|---------|-------------|
| Client "failed to fetch" | Wrong `NEXT_PUBLIC_API_URL` at build time, or port 3001 firewalled |
| Sync/collab not working | Cleartext `ws://` blocked, or client built against `https://` but server is plain HTTP |
| compute returns errors | `modeler-python:latest` not built on host, or docker.sock not mounted |
| Data lost after redeploy | Volume not used — confirm `modeler-data` is mounted, never `down -v` |
| Migration panic on boot | Corrupt/partial DB; restore from backup |

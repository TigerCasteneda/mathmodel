# Server Deployment — TLS via Caddy (automatic HTTPS)

This is an **add-on** to `docs/deployment.md`. It puts a Caddy reverse proxy in
front of `modeler-server` so the public endpoint is `https://` (and WebSocket
collaboration upgrades to secure `wss://`). The server itself is unchanged — it
still listens on plain HTTP `3001`; Caddy terminates TLS.

> This file adds new services only. It does not modify the base
> `deploy/docker-compose.yml`, the server code, or your local dev workflow.
> Local debugging still runs over `http://localhost:3001` exactly as before.

---

## Why you need this for production

The Tauri client upgrades collaboration sockets to `wss://` **only** when its
`NEXT_PUBLIC_API_URL` is `https://`. The two WebSocket routes are:

- `/sync` — CRDT document collaboration
- `/projects/{id}/screen/ws` — screen sharing

Caddy must pass the WebSocket `Upgrade`/`Connection` headers through. The
`reverse_proxy` directive does this automatically — no special config needed.

---

## Prerequisites

- A domain name with an **A record** pointing at the VPS public IP
  (e.g. `api.example.com → 203.0.113.10`).
- Ports **80 and 443** open on the host firewall:

  ```bash
  sudo ufw allow 80/tcp
  sudo ufw allow 443/tcp
  ```

- Caddy auto-provisions Let's Encrypt certs over HTTP-01/TLS-ALPN, so port 80
  must be reachable from the internet for the initial challenge.

---

## Files to add

### `deploy/Caddyfile`

```
# Replace api.example.com with your domain.
api.example.com {
    encode zstd gzip

    # reverse_proxy transparently forwards WebSocket upgrades, so both
    # /sync and /projects/*/screen/ws work over wss:// with no extra config.
    reverse_proxy server:3001
}
```

### `deploy/docker-compose.tls.yml`

A compose **override** that adds Caddy and stops publishing 3001 directly.

```yaml
services:
  server:
    # No longer expose 3001 to the host — only Caddy talks to it over the
    # internal compose network.
    ports: !reset []

  caddy:
    image: caddy:2
    container_name: modeler-caddy
    restart: unless-stopped
    depends_on:
      - server
    ports:
      - "80:80"
      - "443:443"
    volumes:
      - ./Caddyfile:/etc/caddy/Caddyfile:ro
      - caddy-data:/data       # persisted certificates (survives restarts)
      - caddy-config:/config

volumes:
  caddy-data:
  caddy-config:
```

> `ports: !reset []` requires Docker Compose v2.24+. On older versions, instead
> change the base file's mapping to `"127.0.0.1:3001:3001"` so 3001 is only
> reachable locally, not from the public internet.

---

## Deploy with TLS

Layer the override on top of the base compose file:

```bash
cd mathmodel
# edit deploy/Caddyfile with your real domain first

docker compose \
  -f deploy/docker-compose.yml \
  -f deploy/docker-compose.tls.yml \
  --env-file deploy/.env \
  up -d --build

docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.tls.yml logs -f caddy
```

Look for Caddy obtaining a certificate. Then verify:

```bash
curl https://api.example.com/        # valid cert, reachable
```

---

## Build the client for HTTPS

```bash
NEXT_PUBLIC_API_URL=https://api.example.com npm run tauri build
```

Now the client connects over HTTPS and collaboration upgrades to `wss://`
automatically.

---

## Notes

- **Certificate renewal** is automatic; Caddy renews well before expiry. The
  `caddy-data` volume must persist or you'll re-request certs on every restart
  (and can hit Let's Encrypt rate limits).
- **Switching back to no-TLS** for a quick test: just omit the
  `-f deploy/docker-compose.tls.yml` flag — you're back to plain HTTP on 3001.
- **CORS** stays `Any` on the server; fine for desktop clients.
- **Local dev is unaffected** — none of this touches `cargo run` /
  `npm run tauri dev`, which keep using `http://localhost:3001`.

# syntax=docker/dockerfile:1

# ---- Build stage ----
# Debian-based: reqwest uses native-tls (OpenSSL), so we avoid the musl/Alpine
# OpenSSL cross-compile pain by building on glibc.
# Pinned to a recent stable: the committed Cargo.lock was generated with a
# newer toolchain, so an older builder can fail on lockfile format / dep MSRV.
FROM rust:1.84-slim-bookworm AS builder

# OpenSSL + pkg-config are required to compile reqwest's native-tls backend.
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependencies: copy manifests first, build a dummy main, then the real
# sources. Re-running with unchanged Cargo.toml/lock reuses the dep layer.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && echo "" > src/lib.rs \
    && cargo build --release 2>/dev/null || true
RUN rm -rf src

# Real sources + embedded migrations (include_str! reads ../migrations at compile time).
COPY src ./src
COPY migrations ./migrations

# Touch so cargo rebuilds with the real sources after the dummy build.
RUN touch src/main.rs src/lib.rs \
    && cargo build --release --bin modeler-server

# ---- Runtime stage ----
FROM debian:bookworm-slim AS runtime

# ca-certificates: outbound HTTPS (reqwest). libssl3: native-tls runtime.
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /build/target/release/modeler-server /usr/local/bin/modeler-server

# Persistent data (SQLite DB + uploaded files) lives here; mount a volume.
RUN mkdir -p /app/data
ENV DATA_DIR=/app/data
ENV DATABASE_URL=sqlite:/app/data/modeler.db?mode=rwc
ENV PORT=3001

EXPOSE 3001

CMD ["modeler-server"]

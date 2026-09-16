# syntax=docker/dockerfile:1

# ---- Stage 1: build the admin frontend (dist is embedded at compile time) ----
FROM node:22-alpine AS frontend
WORKDIR /app
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

# ---- Stage 2: build the release binary (rust-embed pulls in frontend/dist) ----
FROM rust:1-alpine AS build
# zstd and bundled SQLite compile C code, so a toolchain is required
RUN apk add --no-cache build-base
WORKDIR /app
# Compile the dependency tree against a stub main so this layer is keyed on
# Cargo.toml/Cargo.lock only and the CI layer cache survives src changes.
# The stub's own artifacts are removed so the real build cannot mistake
# them for fresh outputs and ship an empty binary.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && \
    cargo build --release --locked && \
    rm -rf target/release/deps/proxy_converter* \
           target/release/.fingerprint/proxy-converter-* \
           target/release/proxy-converter
COPY src ./src
COPY --from=frontend /app/dist ./frontend/dist
RUN cargo build --release --locked && \
    cp target/release/proxy-converter /usr/local/bin/proxy-converter

# ---- Stage 3: minimal runtime ----
FROM alpine:3
# ca-certificates: reqwest (rustls) needs root CAs to download mrs/geo sources
RUN apk add --no-cache ca-certificates tzdata && mkdir -p /data
COPY --from=build /usr/local/bin/proxy-converter /usr/local/bin/proxy-converter
EXPOSE 8080
ENTRYPOINT ["proxy-converter"]
# Listen on 0.0.0.0: the default 127.0.0.1 is unreachable through port mapping
CMD ["run", "--addr", "0.0.0.0:8080", "--database", "/data/tokens.db"]

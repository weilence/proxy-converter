#!/usr/bin/env bash
# Build the release binary. The admin UI (frontend/dist) is embedded at
# compile time, so the frontend must be built before `cargo build`.
set -euo pipefail
cd "$(dirname "$0")/.."

(
  cd frontend
  npm ci
  npm run build
)

cargo build --release
echo "release binary: target/release/proxy-converter"

#!/usr/bin/env bash
set -euo pipefail

# Build, optimize, and checksum the production Wasm artifact.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

TARGET="wasm32-unknown-unknown"
PACKAGE="wasm_vault"
ARTIFACTS_DIR="${ROOT_DIR}/artifacts"
OPTIMIZED_WASM="${ARTIFACTS_DIR}/${PACKAGE}.wasm"
CHECKSUM_FILE="${ARTIFACTS_DIR}/checksums.txt"
CAPABILITIES="staking,stargate,iterator,cosmwasm_1_1,cosmwasm_1_2,cosmwasm_1_3,cosmwasm_1_4,cosmwasm_2_0,cosmwasm_2_1,cosmwasm_2_2,cosmwasm_3_0,ibc2"
OPT_CACHE_VOLUME="$(basename "$(pwd)")_cache"
REGISTRY_CACHE_VOLUME="registry_cache"
DOCKER_USER="$(id -u):$(id -g)"
DOCKER_PLATFORM="${DOCKER_PLATFORM:-linux/amd64}"

echo "== wasm_vault production build =="

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required tool: $1. $2"
    exit 1
  fi
}

require_command rustup "Install rustup: https://rustup.rs/"
require_command cargo "Install Rust toolchain via rustup."
require_command docker "Docker is required for cosmwasm/optimizer."

echo ">> Ensuring target ${TARGET} is installed"
rustup target add "${TARGET}" >/dev/null

echo ">> Formatting and linting"
cargo fmt --all -- --check
cargo clippy --locked --release --target "${TARGET}" -- -D warnings

echo ">> Running tests"
cargo test --locked

echo ">> Optimizing with cosmwasm/optimizer:0.16.0"
mkdir -p "${ARTIFACTS_DIR}"
# Reset optimizer cache volume to avoid permission leftovers from prior runs.
docker volume rm "${OPT_CACHE_VOLUME}" >/dev/null 2>&1 || true
docker volume create "${OPT_CACHE_VOLUME}" >/dev/null
docker volume create "${REGISTRY_CACHE_VOLUME}" >/dev/null
# Ensure mounted volumes and artifacts dir are writable by the calling user.
docker run --rm --platform "${DOCKER_PLATFORM}" \
  -v "$(pwd)":/code \
  --mount type=volume,source="${OPT_CACHE_VOLUME}",target=/target \
  --mount type=volume,source="${REGISTRY_CACHE_VOLUME}",target=/usr/local/cargo/registry \
  --entrypoint sh \
  cosmwasm/optimizer:0.16.0 \
  -c "chown -R ${DOCKER_USER} /target /usr/local/cargo/registry /code/artifacts"
docker run --rm -v "$(pwd)":/code \
  --platform "${DOCKER_PLATFORM}" \
  --user "${DOCKER_USER}" \
  --mount type=volume,source="${OPT_CACHE_VOLUME}",target=/target \
  --mount type=volume,source="${REGISTRY_CACHE_VOLUME}",target=/usr/local/cargo/registry \
  cosmwasm/optimizer:0.16.0

if [[ ! -f "${OPTIMIZED_WASM}" ]]; then
  echo "Optimized artifact not found at ${OPTIMIZED_WASM}"
  exit 1
fi

echo ">> Writing checksum to ${CHECKSUM_FILE}"
if command -v sha256sum >/dev/null 2>&1; then
  sha256sum "${OPTIMIZED_WASM}" > "${CHECKSUM_FILE}"
else
  shasum -a 256 "${OPTIMIZED_WASM}" > "${CHECKSUM_FILE}"
fi

if command -v cosmwasm-check >/dev/null 2>&1; then
  echo ">> Running cosmwasm-check"
  cosmwasm-check --available-capabilities "${CAPABILITIES}" "${OPTIMIZED_WASM}"
else
  echo "cosmwasm-check not installed; skipping static validation."
fi

echo "Done. Optimized artifact: ${OPTIMIZED_WASM}"

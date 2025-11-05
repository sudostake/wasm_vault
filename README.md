# wasm_vault

Rust smart contract for CosmWasm-based chains. The steps below get you ready to compile, test, and inspect the contract from a clean machine.

## Prerequisites

- Rust toolchain via [rustup](https://rustup.rs/) (1.82 or newer; older toolchains fail because `indexmap` ≥2.12 requires rustc 1.82).
- `wasm32-unknown-unknown` target for Rust.
- [`cargo-run-script`](https://github.com/fornwall/cargo-run-script) (`cargo install cargo-run-script`) for helper aliases like `cargo integration-test` and `cargo run-script optimize`.
- Docker (optional) if you want to run the optimizer script.

## Install & Build Locally

1. Clone the repository and enter it:
   ```sh
   git clone <your-fork-url> wasm_vault
   cd wasm_vault
   ```
2. Install the Wasm compilation target (once per machine):
   ```sh
   rustup target add wasm32-unknown-unknown
   ```
3. Compile the contract to Wasm:
   ```sh
   cargo wasm
   ```
   The resulting artifact lives at `target/wasm32-unknown-unknown/release/wasm_vault.wasm`.

## Run Tests

1. Execute unit tests (runs in native mode):
   ```sh
   cargo unit-test
   ```
2. Run integration tests (files in `tests/` only):
   ```sh
   cargo integration-test
   ```
3. Run the full test matrix if you want everything in one go:
   ```sh
   cargo test
   ```

## Useful Extras

- Generate JSON schemas for messages and responses:
  ```sh
  cargo schema
  ```
  Artifacts are written to `schema/`.
- Produce an optimized Wasm binary (requires Docker):
  ```sh
  cargo run-script optimize
  ```
  The optimized output is placed in `artifacts/`.

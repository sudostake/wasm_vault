# wasm_vault

Rust smart contract for CosmWasm-based chains. The steps below get you ready to compile, test, and inspect the contract from a clean machine.

## Prerequisites

- Rust toolchain via [rustup](https://rustup.rs/). The repository pins `rustc` to 1.86.0 through `rust-toolchain.toml`; `rustup` will auto-install it, or run `rustup toolchain install 1.86.0`.
- `wasm32-unknown-unknown` target for Rust 1.86.0:
  ```sh
  rustup target add wasm32-unknown-unknown --toolchain 1.86.0
  ```
- [`cargo-run-script`](https://github.com/fornwall/cargo-run-script) (`cargo install cargo-run-script`) — required only for `cargo run-script optimize`.
- [`cargo-nextest`](https://nexte.st/) (`cargo install cargo-nextest`) for running the test suite with the multi-threaded runner.
- [`cargo-tarpaulin`](https://github.com/xd009642/tarpaulin) (`cargo install cargo-tarpaulin`) for coverage reports.
- Docker (optional) if you want to run the optimizer script.
- [`cosmwasm-check`](https://github.com/CosmWasm/wasmvm/tree/main/tools/check) for static validation with the same limits the chain enforces.

## Install & Build Locally

1. Clone the repository and enter it:
   ```sh
   git clone <your-fork-url> wasm_vault
   cd wasm_vault
   ```
2. Install the Wasm compilation target (once per machine):
   ```sh
   rustup target add wasm32-unknown-unknown --toolchain 1.86.0
   ```
3. Compile the contract to Wasm:
   ```sh
   cargo wasm
   ```
   The resulting artifact lives at `target/wasm32-unknown-unknown/release/wasm_vault.wasm`.
4. (Optional) Validate the artifact against a CosmWasm 3.0-enabled chain configuration:
   ```sh
   cosmwasm-check --available-capabilities 'staking,stargate,iterator,cosmwasm_1_1,cosmwasm_1_2,cosmwasm_1_3,cosmwasm_1_4,cosmwasm_2_0,cosmwasm_2_1,cosmwasm_2_2,cosmwasm_3_0,ibc2' \
     target/wasm32-unknown-unknown/release/wasm_vault.wasm
   ```
   The contract requires the `cosmwasm_3_0` capability; adjust the list above to match the target network.

## Run Tests

- Run all unit and integration tests with the default runner:
  ```sh
  cargo test
  ```
- Run the cw-multi-test integration suite only (entrypoint `tests/mod.rs`, which re-exports everything under `tests/multitest/`):
  ```sh
  cargo test --test mod
  ```
- Use Nextest for faster, isolated execution when iterating locally:
  ```sh
  cargo nextest run
  ```
- Run only documentation tests (mirrors the `cargo test --doc` CI step):
  ```sh
  cargo test --doc
  ```
- Generate coverage data (uses Tarpaulin):
  ```sh
  cargo tarpaulin
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

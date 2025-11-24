#[cfg(not(target_arch = "wasm32"))]
use cosmwasm_schema::write_api;
#[cfg(not(target_arch = "wasm32"))]
use wasm_vault::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    write_api! {
        instantiate: InstantiateMsg,
        execute: ExecuteMsg,
        query: QueryMsg,
    }
}

// Avoid compiling the schema generator for the Wasm target so production builds succeed.
#[cfg(target_arch = "wasm32")]
fn main() {}

use cosmwasm_std::{Addr, Coin};
use cw_multi_test::{BasicApp, Executor};

use crate::common::{mock_app, store_contract};
use wasm_vault::msg::{ExecuteMsg, InfoResponse, InstantiateMsg, QueryMsg};
use wasm_vault::types::OpenInterest;

fn instantiate_vault() -> (BasicApp, Addr, Addr) {
    let mut app = mock_app();
    let code_id = store_contract(&mut app);
    let owner = app.api().addr_make("creator");

    let contract_addr = app
        .instantiate_contract(
            code_id,
            owner.clone(),
            &InstantiateMsg {
                owner: Some(owner.to_string()),
            },
            &[],
            "vault",
            None,
        )
        .expect("instantiate succeeds");

    (app, contract_addr, owner)
}

#[test]
fn owner_can_open_interest_once() {
    let (mut app, contract_addr, owner) = instantiate_vault();

    let request = OpenInterest {
        liquidity_coin: Coin::new(1_000u128, "uusd"),
        interest_coin: Coin::new(50u128, "ujuno"),
        expiry_duration: 86_400u64,
        collateral: Coin::new(2_000u128, "uatom"),
    };

    let response = app
        .execute_contract(
            owner.clone(),
            contract_addr.clone(),
            &ExecuteMsg::OpenInterest(request.clone()),
            &[],
        )
        .expect("open interest succeeds");

    assert!(response.events.iter().any(|event| {
        event.ty == "wasm"
            && event
                .attributes
                .iter()
                .any(|attr| attr.key == "action" && attr.value == "open_interest")
    }));

    let info: InfoResponse = app
        .wrap()
        .query_wasm_smart(contract_addr.clone(), &QueryMsg::Info)
        .expect("info query succeeds");

    assert_eq!(info.open_interest, Some(request));
}

#[test]
fn cannot_open_interest_twice() {
    let (mut app, contract_addr, owner) = instantiate_vault();

    let msg = ExecuteMsg::OpenInterest(OpenInterest {
        liquidity_coin: Coin::new(500u128, "uusd"),
        interest_coin: Coin::new(10u128, "ujuno"),
        expiry_duration: 100,
        collateral: Coin::new(700u128, "uatom"),
    });

    app.execute_contract(owner.clone(), contract_addr.clone(), &msg, &[])
        .expect("first open interest succeeds");

    let err = app
        .execute_contract(owner.clone(), contract_addr.clone(), &msg, &[])
        .unwrap_err();

    assert!(
        err.to_string().contains("already active"),
        "unexpected error: {err}"
    );
}

#[test]
fn rejects_invalid_inputs() {
    let (mut app, contract_addr, owner) = instantiate_vault();

    let err = app
        .execute_contract(
            owner.clone(),
            contract_addr.clone(),
            &ExecuteMsg::OpenInterest(OpenInterest {
                liquidity_coin: Coin::new(0u128, "uusd"),
                interest_coin: Coin::new(10u128, "ujuno"),
                expiry_duration: 0,
                collateral: Coin::new(700u128, "uatom"),
            }),
            &[],
        )
        .unwrap_err();

    assert!(
        err.to_string().contains("amount must be greater than zero")
            || err.to_string().contains("Expiry duration"),
        "unexpected error: {err}"
    );
}

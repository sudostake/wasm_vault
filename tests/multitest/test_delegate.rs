use cosmwasm_std::{coins, BankMsg, Uint128, Uint256};
use cw_multi_test::Executor;

use crate::common::{mock_app, store_contract, DENOM};

use wasm_vault::msg::{ExecuteMsg, InstantiateMsg};

#[test]
fn owner_can_delegate_existing_vault_funds() {
    let mut app = mock_app();
    let code_id = store_contract(&mut app);

    let owner = app.api().addr_make("creator");
    let contract_addr = app
        .instantiate_contract(
            code_id,
            owner.clone(),
            &InstantiateMsg {
                owner: Some(owner.to_string()),
                liquidation_unbonding_duration: None,
            },
            &[],
            "vault",
            None,
        )
        .expect("instantiate succeeds");

    app.execute(
        owner.clone(),
        BankMsg::Send {
            to_address: contract_addr.to_string(),
            amount: coins(500, DENOM),
        }
        .into(),
    )
    .expect("funding succeeds");

    let validator = app.api().addr_make("validator").into_string();
    let amount = Uint128::new(400);

    let response = app
        .execute_contract(
            owner.clone(),
            contract_addr.clone(),
            &ExecuteMsg::Delegate {
                validator: validator.clone(),
                amount,
            },
            &[],
        )
        .expect("delegate should succeed");

    assert!(response.events.iter().any(|event| {
        event.ty == "wasm"
            && event
                .attributes
                .iter()
                .any(|attr| attr.key == "action" && attr.value == "delegate")
    }));

    let delegation = app
        .wrap()
        .query_delegation(contract_addr.clone(), validator.clone())
        .expect("delegation query should succeed")
        .expect("delegation should exist");

    assert_eq!(delegation.amount.denom, DENOM);
    assert_eq!(delegation.amount.amount, Uint256::from(amount));

    let balance = app
        .wrap()
        .query_balance(contract_addr.clone(), DENOM)
        .expect("balance query should succeed");
    assert_eq!(balance.amount, Uint256::from(500u128 - amount.u128()));
}

#[test]
fn non_owner_cannot_delegate() {
    let mut app = mock_app();
    let code_id = store_contract(&mut app);

    let owner = app.api().addr_make("creator");
    let other = app.api().addr_make("other");

    let contract_addr = app
        .instantiate_contract(
            code_id,
            owner.clone(),
            &InstantiateMsg {
                owner: Some(owner.to_string()),
                liquidation_unbonding_duration: None,
            },
            &[],
            "vault",
            None,
        )
        .expect("instantiate succeeds");

    let err = app
        .execute_contract(
            other.clone(),
            contract_addr,
            &ExecuteMsg::Delegate {
                validator: app.api().addr_make("validator").into_string(),
                amount: Uint128::new(100),
            },
            &[],
        )
        .unwrap_err();

    assert!(err.to_string().contains("Unauthorized"));
}

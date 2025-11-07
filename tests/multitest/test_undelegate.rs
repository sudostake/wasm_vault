use cosmwasm_std::{coins, BankMsg, Uint128, Uint256};
use cw_multi_test::Executor;

use crate::common::{mock_app, store_contract, DENOM};

use wasm_vault::msg::{ExecuteMsg, InstantiateMsg};

#[test]
fn owner_can_undelegate_staked_funds() {
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

    app.execute(
        owner.clone(),
        BankMsg::Send {
            to_address: contract_addr.to_string(),
            amount: coins(800, DENOM),
        }
        .into(),
    )
    .expect("funding succeeds");

    let validator = app.api().addr_make("validator").into_string();
    let delegate_amount = Uint128::new(500);

    app.execute_contract(
        owner.clone(),
        contract_addr.clone(),
        &ExecuteMsg::Delegate {
            validator: validator.clone(),
            amount: delegate_amount,
        },
        &[],
    )
    .expect("delegate succeeds");

    let delegation = app
        .wrap()
        .query_delegation(contract_addr.clone(), validator.clone())
        .expect("delegation query succeeds")
        .expect("delegation exists");
    assert_eq!(delegation.amount.amount, Uint256::from(delegate_amount));

    let undelegate_amount = Uint128::new(200);
    let response = app
        .execute_contract(
            owner.clone(),
            contract_addr.clone(),
            &ExecuteMsg::Undelegate {
                validator: validator.clone(),
                amount: undelegate_amount,
            },
            &[],
        )
        .expect("undelegate succeeds");

    assert!(response.events.iter().any(|event| {
        event.ty == "wasm"
            && event
                .attributes
                .iter()
                .any(|attr| attr.key == "action" && attr.value == "undelegate")
    }));

    let updated_delegation = app
        .wrap()
        .query_delegation(contract_addr.clone(), validator.clone())
        .expect("delegation query succeeds")
        .expect("delegation should remain");

    assert_eq!(
        updated_delegation.amount.amount,
        Uint256::from(delegate_amount.u128() - undelegate_amount.u128())
    );
}

#[test]
fn non_owner_cannot_undelegate() {
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
            &ExecuteMsg::Undelegate {
                validator: app.api().addr_make("validator").into_string(),
                amount: Uint128::new(50),
            },
            &[],
        )
        .unwrap_err();

    assert!(err.to_string().contains("Unauthorized"));
}

use cosmwasm_std::{coins, BankMsg, Uint128, Uint256};
use cw_multi_test::Executor;

use crate::common::{mock_app, store_contract, DENOM};

use wasm_vault::msg::{ExecuteMsg, InstantiateMsg};

#[test]
fn owner_can_redelegate_between_validators() {
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
            amount: coins(900, DENOM),
        }
        .into(),
    )
    .expect("funding succeeds");

    let src_validator = app.api().addr_make("validator").into_string();
    let dst_validator = app.api().addr_make("validator-two").into_string();

    app.execute_contract(
        owner.clone(),
        contract_addr.clone(),
        &ExecuteMsg::Delegate {
            validator: src_validator.clone(),
            amount: Uint128::new(600),
        },
        &[],
    )
    .expect("delegate succeeds");

    let redelegate_amount = Uint128::new(250);
    let response = app
        .execute_contract(
            owner.clone(),
            contract_addr.clone(),
            &ExecuteMsg::Redelegate {
                src_validator: src_validator.clone(),
                dst_validator: dst_validator.clone(),
                amount: redelegate_amount,
            },
            &[],
        )
        .expect("redelegate succeeds");

    assert!(response.events.iter().any(|event| {
        event.ty == "wasm"
            && event
                .attributes
                .iter()
                .any(|attr| attr.key == "action" && attr.value == "redelegate")
    }));

    let source_delegation = app
        .wrap()
        .query_delegation(contract_addr.clone(), src_validator.clone())
        .expect("delegation query succeeds")
        .expect("source delegation exists");
    assert_eq!(
        source_delegation.amount.amount,
        Uint256::from(600u128 - redelegate_amount.u128())
    );

    let destination_delegation = app
        .wrap()
        .query_delegation(contract_addr.clone(), dst_validator.clone())
        .expect("delegation query succeeds")
        .expect("destination delegation exists");
    assert_eq!(
        destination_delegation.amount.amount,
        Uint256::from(redelegate_amount)
    );
}

#[test]
fn non_owner_cannot_redelegate() {
    let mut app = mock_app();
    let code_id = store_contract(&mut app);

    let owner = app.api().addr_make("creator");
    let intruder = app.api().addr_make("other");

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
            intruder,
            contract_addr,
            &ExecuteMsg::Redelegate {
                src_validator: app.api().addr_make("validator").into_string(),
                dst_validator: app.api().addr_make("validator-two").into_string(),
                amount: Uint128::new(100),
            },
            &[],
        )
        .unwrap_err();

    assert!(err.to_string().contains("Unauthorized"));
}

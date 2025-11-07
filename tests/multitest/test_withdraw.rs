use cosmwasm_std::{coins, BankMsg, Uint128, Uint256};
use cw_multi_test::Executor;

use crate::common::{mock_app, store_contract, DENOM};

use wasm_vault::msg::{ExecuteMsg, InstantiateMsg};

#[test]
fn owner_can_withdraw_to_self() {
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

    let funding_amount = 500u128;
    app.execute(
        owner.clone(),
        BankMsg::Send {
            to_address: contract_addr.to_string(),
            amount: coins(funding_amount, DENOM),
        }
        .into(),
    )
    .expect("funding succeeds");

    let owner_balance_before = app
        .wrap()
        .query_balance(owner.clone(), DENOM)
        .expect("owner balance available")
        .amount;

    let withdraw_amount = Uint128::new(200);
    let response = app
        .execute_contract(
            owner.clone(),
            contract_addr.clone(),
            &ExecuteMsg::Withdraw {
                denom: DENOM.to_string(),
                amount: withdraw_amount,
                recipient: None,
            },
            &[],
        )
        .expect("withdraw succeeds");

    assert!(response.events.iter().any(|event| {
        event.ty == "wasm"
            && event
                .attributes
                .iter()
                .any(|attr| attr.key == "action" && attr.value == "withdraw")
    }));

    let owner_balance_after = app
        .wrap()
        .query_balance(owner.clone(), DENOM)
        .expect("owner balance available")
        .amount;
    assert_eq!(
        owner_balance_after,
        owner_balance_before + Uint256::from(withdraw_amount)
    );

    let contract_balance = app
        .wrap()
        .query_balance(contract_addr.clone(), DENOM)
        .expect("contract balance available")
        .amount;
    assert_eq!(
        contract_balance,
        Uint256::from(funding_amount - withdraw_amount.u128())
    );
}

#[test]
fn owner_can_withdraw_to_custom_recipient() {
    let mut app = mock_app();
    let code_id = store_contract(&mut app);

    let owner = app.api().addr_make("creator");
    let recipient = app.api().addr_make("friend");
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
            amount: coins(750, DENOM),
        }
        .into(),
    )
    .expect("funding succeeds");

    let recipient_balance_before = app
        .wrap()
        .query_balance(recipient.clone(), DENOM)
        .expect("recipient balance available")
        .amount;

    let withdraw_amount = Uint128::new(320);
    app.execute_contract(
        owner.clone(),
        contract_addr.clone(),
        &ExecuteMsg::Withdraw {
            denom: DENOM.to_string(),
            amount: withdraw_amount,
            recipient: Some(recipient.to_string()),
        },
        &[],
    )
    .expect("withdraw succeeds");

    let recipient_balance_after = app
        .wrap()
        .query_balance(recipient.clone(), DENOM)
        .expect("recipient balance available")
        .amount;
    assert_eq!(
        recipient_balance_after,
        recipient_balance_before + Uint256::from(withdraw_amount)
    );

    let contract_balance = app
        .wrap()
        .query_balance(contract_addr.clone(), DENOM)
        .expect("contract balance available")
        .amount;
    assert_eq!(
        contract_balance,
        Uint256::from(750u128 - withdraw_amount.u128())
    );
}

#[test]
fn non_owner_cannot_withdraw() {
    let mut app = mock_app();
    let code_id = store_contract(&mut app);

    let owner = app.api().addr_make("creator");
    let intruder = app.api().addr_make("intruder");
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
            intruder.clone(),
            contract_addr,
            &ExecuteMsg::Withdraw {
                denom: DENOM.to_string(),
                amount: Uint128::new(50),
                recipient: None,
            },
            &[],
        )
        .unwrap_err();

    assert!(err.to_string().contains("Unauthorized"));
}

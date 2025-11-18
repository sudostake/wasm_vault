use cosmwasm_std::{coins, BankMsg, Uint128};
use cw_multi_test::Executor;

use crate::common::{mock_app, store_contract, DENOM};

use wasm_vault::msg::{ExecuteMsg, InstantiateMsg};

#[test]
fn owner_can_claim_rewards_from_all_validators() {
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
            amount: coins(1_000, DENOM),
        }
        .into(),
    )
    .expect("funding succeeds");

    let validator_one = app.api().addr_make("validator").into_string();
    app.execute_contract(
        owner.clone(),
        contract_addr.clone(),
        &ExecuteMsg::Delegate {
            validator: validator_one,
            amount: Uint128::new(600),
        },
        &[],
    )
    .expect("delegate succeeds");

    let validator_two = app.api().addr_make("validator-two").into_string();
    app.execute_contract(
        owner.clone(),
        contract_addr.clone(),
        &ExecuteMsg::Delegate {
            validator: validator_two,
            amount: Uint128::new(300),
        },
        &[],
    )
    .expect("delegate succeeds");

    // Advance time so rewards accrue
    app.update_block(|block| {
        block.height += 1_000;
        block.time = block.time.plus_seconds(365 * 24 * 60 * 60);
    });

    let balance_before = app
        .wrap()
        .query_balance(contract_addr.clone(), DENOM)
        .expect("balance query succeeds")
        .amount;

    let response = app
        .execute_contract(
            owner.clone(),
            contract_addr.clone(),
            &ExecuteMsg::ClaimDelegatorRewards {},
            &[],
        )
        .expect("claim rewards succeeds");

    assert!(response.events.iter().any(|event| {
        event.ty == "wasm"
            && event
                .attributes
                .iter()
                .any(|attr| attr.key == "action" && attr.value == "claim_delegator_rewards")
    }));

    let balance_after = app
        .wrap()
        .query_balance(contract_addr.clone(), DENOM)
        .expect("balance query succeeds")
        .amount;

    assert!(
        balance_after > balance_before,
        "expected contract balance to increase after claiming rewards"
    );
}

#[test]
fn non_owner_cannot_claim_rewards() {
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
                liquidation_unbonding_duration: None,
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
            &ExecuteMsg::ClaimDelegatorRewards {},
            &[],
        )
        .unwrap_err();

    assert!(err.to_string().contains("Unauthorized"));
}

use cosmwasm_std::{Decimal, VoteOption, WeightedVoteOption};
use cw_multi_test::Executor;

use crate::common::{mock_app, mock_app_with_gov_accepting, store_contract};
use wasm_vault::msg::{ExecuteMsg, InstantiateMsg};

#[test]
fn owner_can_cast_standard_vote_when_gov_accepts() {
    let mut app = mock_app_with_gov_accepting();
    let code_id = store_contract(&mut app);

    let owner = app.api().addr_make("owner");
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

    let response = app
        .execute_contract(
            owner.clone(),
            contract_addr,
            &ExecuteMsg::Vote {
                proposal_id: 7,
                option: VoteOption::Yes,
            },
            &[],
        )
        .expect("vote should succeed when gov accepts");

    assert!(response.events.iter().any(|event| {
        event.ty == "wasm"
            && event
                .attributes
                .iter()
                .any(|attr| attr.key == "action" && attr.value == "vote")
    }));
}

#[test]
fn owner_can_cast_weighted_vote_when_gov_accepts() {
    let mut app = mock_app_with_gov_accepting();
    let code_id = store_contract(&mut app);

    let owner = app.api().addr_make("weighted-owner");
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

    let response = app
        .execute_contract(
            owner.clone(),
            contract_addr,
            &ExecuteMsg::VoteWeighted {
                proposal_id: 42,
                options: vec![
                    WeightedVoteOption {
                        option: VoteOption::Yes,
                        weight: Decimal::percent(70),
                    },
                    WeightedVoteOption {
                        option: VoteOption::No,
                        weight: Decimal::percent(30),
                    },
                ],
            },
            &[],
        )
        .expect("weighted vote should succeed when gov accepts");

    assert!(response.events.iter().any(|event| {
        event.ty == "wasm"
            && event.attributes.iter().any(|attr| {
                (attr.key == "vote_type" && attr.value == "weighted")
                    || (attr.key == "option_count" && attr.value == "2")
            })
    }));
}

#[test]
fn vote_fails_when_gov_module_rejects() {
    let mut app = mock_app();
    let code_id = store_contract(&mut app);

    let owner = app.api().addr_make("failing-owner");
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
            owner.clone(),
            contract_addr,
            &ExecuteMsg::Vote {
                proposal_id: 99,
                option: VoteOption::No,
            },
            &[],
        )
        .unwrap_err();

    assert!(
        err.to_string().contains("Unexpected exec msg"),
        "expected failing gov module error, got {err}"
    );
}

#[test]
fn non_owner_cannot_vote_even_when_gov_accepts() {
    let mut app = mock_app_with_gov_accepting();
    let code_id = store_contract(&mut app);

    let owner = app.api().addr_make("gov-owner");
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

    let intruder = app.api().addr_make("intruder");
    let err = app
        .execute_contract(
            intruder,
            contract_addr,
            &ExecuteMsg::Vote {
                proposal_id: 13,
                option: VoteOption::Abstain,
            },
            &[],
        )
        .unwrap_err();

    assert!(
        err.to_string().contains("Unauthorized"),
        "expected unauthorized error, got {err}"
    );
}

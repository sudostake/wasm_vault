use cosmwasm_std::{coins, Addr, Coin, Uint256};
use cw_multi_test::{BasicApp, Executor};

use crate::common::{mock_app, store_contract, DENOM};
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

#[test]
fn owner_can_close_pending_open_interest() {
    let (mut app, contract_addr, owner) = instantiate_vault();

    let msg = ExecuteMsg::OpenInterest(OpenInterest {
        liquidity_coin: Coin::new(1_000u128, "uusd"),
        interest_coin: Coin::new(50u128, "ujuno"),
        expiry_duration: 86_400u64,
        collateral: Coin::new(2_000u128, "uatom"),
    });

    app.execute_contract(owner.clone(), contract_addr.clone(), &msg, &[])
        .expect("open interest succeeds");

    let response = app
        .execute_contract(
            owner.clone(),
            contract_addr.clone(),
            &ExecuteMsg::CloseOpenInterest {},
            &[],
        )
        .expect("close succeeds");

    assert!(response.events.iter().any(|event| {
        event.ty == "wasm"
            && event
                .attributes
                .iter()
                .any(|attr| attr.key == "action" && attr.value == "close_open_interest")
    }));

    let info: InfoResponse = app
        .wrap()
        .query_wasm_smart(contract_addr.clone(), &QueryMsg::Info)
        .expect("info query succeeds");

    assert!(info.open_interest.is_none());
}

#[test]
fn cannot_close_without_active_open_interest() {
    let (mut app, contract_addr, owner) = instantiate_vault();

    let err = app
        .execute_contract(
            owner.clone(),
            contract_addr.clone(),
            &ExecuteMsg::CloseOpenInterest {},
            &[],
        )
        .unwrap_err();

    assert!(err.to_string().contains("No open interest"));
}

#[test]
fn lender_can_fund_open_interest_and_refund_counter_offers() {
    let (mut app, contract_addr, owner) = instantiate_vault();

    let open_interest = OpenInterest {
        liquidity_coin: Coin::new(1_000u128, DENOM),
        interest_coin: Coin::new(50u128, "uinterest"),
        expiry_duration: 86_400u64,
        collateral: Coin::new(2_000u128, "ucollateral"),
    };

    app.execute_contract(
        owner.clone(),
        contract_addr.clone(),
        &ExecuteMsg::OpenInterest(open_interest.clone()),
        &[],
    )
    .expect("open interest set");

    let proposer_a = app.api().addr_make("bidder-a");
    let proposer_b = app.api().addr_make("bidder-b");

    app.send_tokens(owner.clone(), proposer_a.clone(), &coins(5_000, DENOM))
        .expect("fund proposer a");
    app.send_tokens(owner.clone(), proposer_b.clone(), &coins(5_000, DENOM))
        .expect("fund proposer b");

    let proposer_a_balance_before = app
        .wrap()
        .query_balance(proposer_a.to_string(), DENOM)
        .expect("balance query");
    let proposer_b_balance_before = app
        .wrap()
        .query_balance(proposer_b.to_string(), DENOM)
        .expect("balance query");

    let mut offer_a = open_interest.clone();
    offer_a.liquidity_coin.amount = offer_a
        .liquidity_coin
        .amount
        .checked_sub(Uint256::from(100u128))
        .expect("amount stays positive");
    let mut offer_b = open_interest.clone();
    offer_b.liquidity_coin.amount = offer_b
        .liquidity_coin
        .amount
        .checked_sub(Uint256::from(200u128))
        .expect("amount stays positive");

    app.execute_contract(
        proposer_a.clone(),
        contract_addr.clone(),
        &ExecuteMsg::ProposeCounterOffer(offer_a.clone()),
        &[offer_a.liquidity_coin.clone()],
    )
    .expect("offer a stored");

    app.execute_contract(
        proposer_b.clone(),
        contract_addr.clone(),
        &ExecuteMsg::ProposeCounterOffer(offer_b.clone()),
        &[offer_b.liquidity_coin.clone()],
    )
    .expect("offer b stored");

    let lender = app.api().addr_make("direct-lender");
    app.send_tokens(owner.clone(), lender.clone(), &coins(5_000, DENOM))
        .expect("fund lender");

    let response = app
        .execute_contract(
            lender.clone(),
            contract_addr.clone(),
            &ExecuteMsg::FundOpenInterest {},
            &[open_interest.liquidity_coin.clone()],
        )
        .expect("funding succeeds");

    assert!(response.events.iter().any(|event| event
        .attributes
        .iter()
        .any(|attr| attr.value == "fund_open_interest")));

    let info: InfoResponse = app
        .wrap()
        .query_wasm_smart(contract_addr.clone(), &QueryMsg::Info)
        .expect("info query succeeds");

    assert_eq!(info.lender, Some(lender.to_string()));
    assert_eq!(info.open_interest, Some(open_interest.clone()));
    assert!(info.counter_offers.is_none());

    let proposer_a_balance_after = app
        .wrap()
        .query_balance(proposer_a.to_string(), DENOM)
        .expect("balance query");
    let proposer_b_balance_after = app
        .wrap()
        .query_balance(proposer_b.to_string(), DENOM)
        .expect("balance query");

    assert_eq!(
        proposer_a_balance_after.amount,
        proposer_a_balance_before.amount
    );
    assert_eq!(
        proposer_b_balance_after.amount,
        proposer_b_balance_before.amount
    );
}

use cosmwasm_std::{coins, Addr, Coin, Uint128, Uint256};
use cw_multi_test::{BasicApp, Executor};
use std::convert::TryFrom;

use crate::common::{mint_contract_collateral, mock_app, store_contract, DENOM};
use wasm_vault::msg::{ExecuteMsg, InfoResponse, InstantiateMsg, QueryMsg};
use wasm_vault::types::OpenInterest;

fn reduce_liquidity_amount(base_offer: &OpenInterest, reduction: Uint256) -> OpenInterest {
    let mut offer = base_offer.clone();
    offer.liquidity_coin.amount = offer
        .liquidity_coin
        .amount
        .checked_sub(reduction)
        .expect("amount stays positive");
    offer
}

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

    mint_contract_collateral(&mut app, &contract_addr, &request.collateral);

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

    let open_interest = OpenInterest {
        liquidity_coin: Coin::new(500u128, "uusd"),
        interest_coin: Coin::new(10u128, "ujuno"),
        expiry_duration: 100,
        collateral: Coin::new(700u128, "uatom"),
    };
    mint_contract_collateral(&mut app, &contract_addr, &open_interest.collateral);

    let msg = ExecuteMsg::OpenInterest(open_interest.clone());

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

    let invalid_request = OpenInterest {
        liquidity_coin: Coin::new(0u128, "uusd"),
        interest_coin: Coin::new(10u128, "ujuno"),
        expiry_duration: 0,
        collateral: Coin::new(700u128, "uatom"),
    };
    mint_contract_collateral(&mut app, &contract_addr, &invalid_request.collateral);

    let err = app
        .execute_contract(
            owner.clone(),
            contract_addr.clone(),
            &ExecuteMsg::OpenInterest(invalid_request.clone()),
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

    let open_interest = OpenInterest {
        liquidity_coin: Coin::new(1_000u128, "uusd"),
        interest_coin: Coin::new(50u128, "ujuno"),
        expiry_duration: 86_400u64,
        collateral: Coin::new(2_000u128, "uatom"),
    };
    mint_contract_collateral(&mut app, &contract_addr, &open_interest.collateral);

    let msg = ExecuteMsg::OpenInterest(open_interest.clone());

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

    mint_contract_collateral(&mut app, &contract_addr, &open_interest.collateral);

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

    let offer_a = reduce_liquidity_amount(&open_interest, Uint256::from(100u128));
    let offer_b = reduce_liquidity_amount(&open_interest, Uint256::from(200u128));

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
            &ExecuteMsg::FundOpenInterest(open_interest.clone()),
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

#[test]
fn owner_can_repay_funded_open_interest() {
    let (mut app, contract_addr, owner) = instantiate_vault();

    let open_interest = OpenInterest {
        liquidity_coin: Coin::new(1_000u128, DENOM),
        interest_coin: Coin::new(50u128, DENOM),
        expiry_duration: 86_400u64,
        collateral: Coin::new(2_000u128, "ucollateral"),
    };

    mint_contract_collateral(&mut app, &contract_addr, &open_interest.collateral);

    app.execute_contract(
        owner.clone(),
        contract_addr.clone(),
        &ExecuteMsg::OpenInterest(open_interest.clone()),
        &[],
    )
    .expect("open interest set");

    let lender = app.api().addr_make("lender");
    app.send_tokens(owner.clone(), lender.clone(), &coins(5_000, DENOM))
        .expect("fund lender");

    app.execute_contract(
        lender.clone(),
        contract_addr.clone(),
        &ExecuteMsg::FundOpenInterest(open_interest.clone()),
        &[open_interest.liquidity_coin.clone()],
    )
    .expect("funding succeeds");

    let interest_amount = Uint128::try_from(open_interest.interest_coin.amount)
        .expect("interest amount fits in Uint128");
    app.send_tokens(
        owner.clone(),
        contract_addr.clone(),
        &coins(interest_amount.u128(), DENOM),
    )
    .expect("deposit interest");

    let lender_balance_before = app
        .wrap()
        .query_balance(lender.to_string(), DENOM)
        .expect("lender balance before repay");

    let response = app
        .execute_contract(
            owner.clone(),
            contract_addr.clone(),
            &ExecuteMsg::RepayOpenInterest {},
            &[],
        )
        .expect("repay succeeds");

    assert!(response.events.iter().any(|event| {
        event.ty == "wasm"
            && event
                .attributes
                .iter()
                .any(|attr| attr.key == "action" && attr.value == "repay_open_interest")
    }));

    let expected_total = open_interest
        .liquidity_coin
        .amount
        .checked_add(open_interest.interest_coin.amount)
        .expect("sum fits");

    let lender_balance_after = app
        .wrap()
        .query_balance(lender.to_string(), DENOM)
        .expect("lender balance after repay");

    assert_eq!(
        lender_balance_after.amount,
        lender_balance_before
            .amount
            .checked_add(expected_total)
            .expect("sum fits")
    );

    let info: InfoResponse = app
        .wrap()
        .query_wasm_smart(contract_addr.clone(), &QueryMsg::Info)
        .expect("info query succeeds");

    assert!(info.open_interest.is_none());
    assert!(info.lender.is_none());

    let balance = app
        .wrap()
        .query_balance(contract_addr.clone(), DENOM)
        .expect("balance query");
    let balance_amount =
        Uint128::try_from(balance.amount).expect("contract balance fits into Uint128");
    assert_eq!(balance_amount.u128(), 0);
}

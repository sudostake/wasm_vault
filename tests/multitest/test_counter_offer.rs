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
fn owner_accepts_counter_offer_and_refunds_others() {
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

    let proposer_a = app.api().addr_make("user");
    let proposer_b = app.api().addr_make("lender-two");

    // Fund the second proposer from the owner so both can submit offers.
    app.send_tokens(owner.clone(), proposer_b.clone(), &coins(50_000, DENOM))
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
        .checked_sub(Uint256::from(25u128))
        .expect("amount stays positive");

    let mut offer_b = open_interest.clone();
    offer_b.liquidity_coin.amount = offer_b
        .liquidity_coin
        .amount
        .checked_sub(Uint256::from(75u128))
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

    let owner_balance_before = app
        .wrap()
        .query_balance(owner.to_string(), DENOM)
        .expect("balance query");

    app.execute_contract(
        owner.clone(),
        contract_addr.clone(),
        &ExecuteMsg::AcceptCounterOffer {
            proposer: proposer_a.to_string(),
            open_interest: offer_a.clone(),
        },
        &[],
    )
    .expect("accept succeeds");

    let owner_balance_after = app
        .wrap()
        .query_balance(owner.to_string(), DENOM)
        .expect("balance query");

    let owner_amount_after = owner_balance_after.amount;
    let owner_amount_before = owner_balance_before.amount;
    let gained = owner_amount_after
        .checked_sub(owner_amount_before)
        .expect("gain computed");
    assert_eq!(gained, Uint256::zero(), "owner balance unaffected");

    let proposer_b_balance_after = app
        .wrap()
        .query_balance(proposer_b.to_string(), DENOM)
        .expect("balance query");
    assert_eq!(proposer_b_balance_after, proposer_b_balance_before);

    let proposer_a_balance_after = app
        .wrap()
        .query_balance(proposer_a.to_string(), DENOM)
        .expect("balance query");
    let proposer_a_amount_before = proposer_a_balance_before.amount;
    let proposer_a_amount_after = proposer_a_balance_after.amount;
    let lost = proposer_a_amount_before
        .checked_sub(proposer_a_amount_after)
        .expect("loss computed");
    assert_eq!(lost, offer_a.liquidity_coin.amount);

    let info: InfoResponse = app
        .wrap()
        .query_wasm_smart(contract_addr.clone(), &QueryMsg::Info)
        .expect("info query succeeds");

    assert_eq!(info.lender, Some(proposer_a.to_string()));
    assert!(info.counter_offers.is_none());
    assert_eq!(info.open_interest, Some(offer_a));
}

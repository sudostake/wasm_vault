use cosmwasm_std::{attr, DepsMut, Env, MessageInfo, Response};

use crate::{
    state::{LENDER, OPEN_INTEREST},
    types::OpenInterest,
    ContractError,
};

use super::helpers::{
    open_interest_attributes, refund_counter_offer_escrow, set_active_lender,
    validate_liquidity_funding,
};

pub fn fund(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    expected_interest: OpenInterest,
) -> Result<Response, ContractError> {
    let open_interest = OPEN_INTEREST
        .load(deps.storage)?
        .ok_or(ContractError::NoOpenInterest {})?;

    if LENDER.load(deps.storage)?.is_some() {
        return Err(ContractError::LenderAlreadySet {});
    }

    if open_interest != expected_interest {
        return Err(ContractError::OpenInterestMismatch {});
    }

    validate_liquidity_funding(&info, &open_interest.liquidity_coin)?;

    let lender = info.sender;
    let expiry = env.block.time.plus_seconds(open_interest.expiry_duration);
    set_active_lender(deps.storage, lender.clone(), expiry)?;

    let refund_msgs = refund_counter_offer_escrow(deps.storage)?;
    let refund_count = refund_msgs.len();

    let mut attrs = open_interest_attributes("fund_open_interest", &open_interest);
    attrs.push(attr("lender", lender.as_str()));
    attrs.push(attr("refunded_offers", refund_count.to_string()));

    Ok(Response::new()
        .add_messages(refund_msgs)
        .add_attributes(attrs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contract::open_interest::test_helpers::{build_open_interest, sample_coin, setup},
        state::{COUNTER_OFFERS, LENDER, OPEN_INTEREST, OPEN_INTEREST_EXPIRY, OUTSTANDING_DEBT},
        ContractError,
    };
    use cosmwasm_std::coins;
    use cosmwasm_std::{
        attr,
        testing::{message_info, mock_dependencies, mock_env},
        BankMsg, Coin, Order, Uint256,
    };

    #[test]
    fn fund_requires_active_open_interest() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut().storage, &owner);

        let lender = deps.api.addr_make("lender");
        let expected_interest = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );
        let err = fund(
            deps.as_mut(),
            mock_env(),
            message_info(&lender, &[Coin::new(100u128, "uusd")]),
            expected_interest.clone(),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::NoOpenInterest {}));
    }

    #[test]
    fn fund_rejects_when_lender_already_present() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut().storage, &owner);

        let request = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );
        OPEN_INTEREST
            .save(deps.as_mut().storage, &Some(request.clone()))
            .expect("open interest stored");
        let existing_lender = deps.api.addr_make("existing");
        LENDER
            .save(deps.as_mut().storage, &Some(existing_lender))
            .expect("lender stored");

        let new_lender = deps.api.addr_make("new");
        let err = fund(
            deps.as_mut(),
            mock_env(),
            message_info(&new_lender, &[Coin::new(100u128, "uusd")]),
            request.clone(),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::LenderAlreadySet {}));
    }

    #[test]
    fn fund_validates_exact_liquidity_amount() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut().storage, &owner);

        let request = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );
        OPEN_INTEREST
            .save(deps.as_mut().storage, &Some(request.clone()))
            .expect("open interest stored");

        let lender = deps.api.addr_make("lender");
        let err = fund(
            deps.as_mut(),
            mock_env(),
            message_info(
                &lender,
                &[Coin::new(
                    request
                        .liquidity_coin
                        .amount
                        .checked_sub(Uint256::from(1u128))
                        .unwrap(),
                    &request.liquidity_coin.denom,
                )],
            ),
            request.clone(),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::OpenInterestFundingMismatch { .. }
        ));
    }

    #[test]
    fn fund_rejects_mismatched_open_interest() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut().storage, &owner);

        let request = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );
        OPEN_INTEREST
            .save(deps.as_mut().storage, &Some(request.clone()))
            .expect("open interest stored");

        let mut mismatched_interest = request.clone();
        mismatched_interest.liquidity_coin.amount = mismatched_interest
            .liquidity_coin
            .amount
            .checked_sub(Uint256::from(1u128))
            .expect("amount stays positive");

        let lender = deps.api.addr_make("lender");
        let err = fund(
            deps.as_mut(),
            mock_env(),
            message_info(&lender, &[request.liquidity_coin.clone()]),
            mismatched_interest,
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::OpenInterestMismatch {}));
    }

    #[test]
    fn fund_sets_lender_and_refunds_counter_offers() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut().storage, &owner);

        let request = build_open_interest(
            sample_coin(1_000, "uusd"),
            sample_coin(50, "ujuno"),
            86_400,
            sample_coin(2_000, "uatom"),
        );
        OPEN_INTEREST
            .save(deps.as_mut().storage, &Some(request.clone()))
            .expect("open interest stored");

        let proposer_a = deps.api.addr_make("alice");
        let proposer_b = deps.api.addr_make("bob");

        let mut offer_a = request.clone();
        offer_a.liquidity_coin.amount = offer_a
            .liquidity_coin
            .amount
            .checked_sub(Uint256::from(100u128))
            .expect("amount stays positive");
        let mut offer_b = request.clone();
        offer_b.liquidity_coin.amount = offer_b
            .liquidity_coin
            .amount
            .checked_sub(Uint256::from(200u128))
            .expect("amount stays positive");

        COUNTER_OFFERS
            .save(deps.as_mut().storage, &proposer_a, &offer_a.clone())
            .expect("offer stored");
        COUNTER_OFFERS
            .save(deps.as_mut().storage, &proposer_b, &offer_b.clone())
            .expect("offer stored");
        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &Some(request.liquidity_coin.clone()))
            .expect("debt stored");

        let lender = deps.api.addr_make("lender");
        let response = fund(
            deps.as_mut(),
            mock_env(),
            message_info(&lender, &[request.liquidity_coin.clone()]),
            request.clone(),
        )
        .expect("fund succeeds");

        assert_eq!(response.attributes[0], attr("action", "fund_open_interest"));
        assert_eq!(response.messages.len(), 2);
        for msg in &response.messages {
            match &msg.msg {
                cosmwasm_std::CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                    let expected = if to_address == proposer_a.as_str() {
                        offer_a.liquidity_coin.clone()
                    } else {
                        assert_eq!(to_address, proposer_b.as_str());
                        offer_b.liquidity_coin.clone()
                    };
                    assert_eq!(amount.as_slice(), &[expected]);
                }
                other => panic!("unexpected message: {:?}", other),
            }
        }

        let stored_lender = LENDER
            .load(deps.as_ref().storage)
            .expect("lender query succeeds");
        assert_eq!(stored_lender, Some(lender));

        let mut offers = COUNTER_OFFERS.range(deps.as_ref().storage, None, None, Order::Ascending);
        assert!(offers.next().is_none());

        let debt = OUTSTANDING_DEBT
            .load(deps.as_ref().storage)
            .expect("debt query succeeds");
        assert!(debt.is_none());
    }

    #[test]
    fn fund_records_expiry_timestamp() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut().storage, &owner);

        let request = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "uinterest"),
            1_000,
            sample_coin(200, "uatom"),
        );
        OPEN_INTEREST
            .save(deps.as_mut().storage, &Some(request.clone()))
            .expect("open interest stored");

        let env = mock_env();
        deps.querier
            .bank
            .update_balance(env.contract.address.as_str(), coins(100, "uusd"));

        let lender_addr = deps.api.addr_make("lender");
        fund(
            deps.as_mut(),
            env.clone(),
            message_info(&lender_addr, &[request.liquidity_coin.clone()]),
            request.clone(),
        )
        .expect("fund succeeds");

        let stored_expiry = OPEN_INTEREST_EXPIRY
            .load(deps.as_ref().storage)
            .expect("expiry loaded")
            .expect("expiry set");
        let expected = env.block.time.plus_seconds(request.expiry_duration);
        assert_eq!(stored_expiry, expected);
    }
}

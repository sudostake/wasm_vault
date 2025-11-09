use cosmwasm_std::{
    attr, Addr, BankMsg, Coin, DepsMut, Env, MessageInfo, Order, Response, StdResult, Storage,
    Uint256,
};

use crate::{
    state::{COUNTER_OFFERS, LENDER, OPEN_INTEREST, OUTSTANDING_DEBT, OWNER},
    types::OpenInterest,
    ContractError,
};

pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    open_interest: OpenInterest,
) -> Result<Response, ContractError> {
    let owner = OWNER.load(deps.storage)?;
    if info.sender != owner {
        return Err(ContractError::Unauthorized {});
    }

    if OPEN_INTEREST.load(deps.storage)?.is_some() {
        return Err(ContractError::OpenInterestAlreadyExists {});
    }

    validate_open_interest(&open_interest)?;

    OPEN_INTEREST.save(deps.storage, &Some(open_interest.clone()))?;
    COUNTER_OFFERS.clear(deps.storage);

    Ok(Response::new().add_attributes([
        attr("action", "open_interest"),
        attr(
            "liquidity_denom",
            open_interest.liquidity_coin.denom.clone(),
        ),
        attr(
            "liquidity_amount",
            open_interest.liquidity_coin.amount.to_string(),
        ),
        attr("interest_denom", open_interest.interest_coin.denom.clone()),
        attr(
            "interest_amount",
            open_interest.interest_coin.amount.to_string(),
        ),
        attr("collateral_denom", open_interest.collateral.denom.clone()),
        attr(
            "collateral_amount",
            open_interest.collateral.amount.to_string(),
        ),
        attr("expiry_duration", open_interest.expiry_duration.to_string()),
    ]))
}

pub fn close(deps: DepsMut, info: MessageInfo) -> Result<Response, ContractError> {
    let owner = OWNER.load(deps.storage)?;
    if info.sender != owner {
        return Err(ContractError::Unauthorized {});
    }

    if LENDER.load(deps.storage)?.is_some() {
        return Err(ContractError::LenderAlreadySet {});
    }

    let open_interest = OPEN_INTEREST
        .load(deps.storage)?
        .ok_or(ContractError::NoOpenInterest {})?;

    OPEN_INTEREST.save(deps.storage, &None)?;
    let refund_msgs = refund_counter_offer_escrow(deps.storage)?;

    let response = Response::new().add_attributes([
        attr("action", "close_open_interest"),
        attr(
            "liquidity_denom",
            open_interest.liquidity_coin.denom.clone(),
        ),
        attr(
            "liquidity_amount",
            open_interest.liquidity_coin.amount.to_string(),
        ),
        attr("interest_denom", open_interest.interest_coin.denom.clone()),
        attr(
            "interest_amount",
            open_interest.interest_coin.amount.to_string(),
        ),
        attr("collateral_denom", open_interest.collateral.denom.clone()),
        attr(
            "collateral_amount",
            open_interest.collateral.amount.to_string(),
        ),
    ]);

    Ok(response.add_messages(refund_msgs))
}

pub fn fund(
    deps: DepsMut,
    _env: Env,
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
    LENDER.save(deps.storage, &Some(lender.clone()))?;

    let refund_msgs = refund_counter_offer_escrow(deps.storage)?;
    let refund_count = refund_msgs.len();

    Ok(Response::new().add_messages(refund_msgs).add_attributes([
        attr("action", "fund_open_interest"),
        attr("lender", lender.as_str()),
        attr(
            "liquidity_denom",
            open_interest.liquidity_coin.denom.clone(),
        ),
        attr(
            "liquidity_amount",
            open_interest.liquidity_coin.amount.to_string(),
        ),
        attr("refunded_offers", refund_count.to_string()),
    ]))
}

fn validate_open_interest(open_interest: &OpenInterest) -> Result<(), ContractError> {
    validate_coin(&open_interest.liquidity_coin, "liquidity_coin")?;
    validate_coin(&open_interest.interest_coin, "interest_coin")?;
    validate_coin(&open_interest.collateral, "collateral")?;

    if open_interest.expiry_duration == 0 {
        return Err(ContractError::InvalidExpiryDuration {});
    }

    Ok(())
}

fn validate_coin(coin: &Coin, field: &'static str) -> Result<(), ContractError> {
    if coin.amount.is_zero() {
        return Err(ContractError::InvalidCoinAmount { field });
    }

    if coin.denom.is_empty() {
        return Err(ContractError::InvalidCoinDenom { field });
    }

    Ok(())
}

fn validate_liquidity_funding(
    info: &MessageInfo,
    liquidity_coin: &Coin,
) -> Result<(), ContractError> {
    let denom = &liquidity_coin.denom;
    let expected = liquidity_coin.amount;
    let received = info
        .funds
        .iter()
        .filter(|coin| coin.denom == *denom)
        .fold(Uint256::zero(), |acc, coin| acc + coin.amount);

    if received != expected {
        return Err(ContractError::OpenInterestFundingMismatch {
            denom: denom.clone(),
            expected,
            received,
        });
    }

    Ok(())
}

fn refund_counter_offer_escrow(storage: &mut dyn Storage) -> StdResult<Vec<BankMsg>> {
    let offers = COUNTER_OFFERS
        .range(storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<(Addr, OpenInterest)>>>()?;

    let mut refunds = Vec::with_capacity(offers.len());

    for (addr, offer) in &offers {
        refunds.push(BankMsg::Send {
            to_address: addr.to_string(),
            amount: vec![offer.liquidity_coin.clone()],
        });
    }

    COUNTER_OFFERS.clear(storage);
    OUTSTANDING_DEBT.save(storage, &None)?;

    Ok(refunds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::{
        attr,
        testing::{message_info, mock_dependencies, mock_env},
        Addr, BankMsg, Order, Uint256,
    };

    fn setup(deps: DepsMut, owner: &Addr) {
        OWNER.save(deps.storage, owner).expect("owner stored");
        LENDER.save(deps.storage, &None).expect("lender cleared");
        OUTSTANDING_DEBT
            .save(deps.storage, &None)
            .expect("debt cleared");
        OPEN_INTEREST
            .save(deps.storage, &None)
            .expect("open interest cleared");
    }

    fn sample_coin(amount: u128, denom: &str) -> Coin {
        Coin::new(amount, denom)
    }

    fn build_open_interest(
        liquidity_coin: Coin,
        interest_coin: Coin,
        expiry_duration: u64,
        collateral: Coin,
    ) -> OpenInterest {
        OpenInterest {
            liquidity_coin,
            interest_coin,
            expiry_duration,
            collateral,
        }
    }

    #[test]
    fn rejects_non_owner_senders() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut(), &owner);
        let intruder = deps.api.addr_make("intruder");
        let request = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );

        let response = execute(
            deps.as_mut(),
            mock_env(),
            message_info(&intruder, &[]),
            request,
        );

        assert!(matches!(response, Err(ContractError::Unauthorized {})));
    }

    #[test]
    fn rejects_when_interest_already_open() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut(), &owner);

        let request = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );

        OPEN_INTEREST
            .save(deps.as_mut().storage, &Some(request.clone()))
            .expect("interest stored");

        let err = execute(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            request,
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::OpenInterestAlreadyExists {}));
    }

    #[test]
    fn rejects_zero_coin_amounts() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut(), &owner);
        let request = build_open_interest(
            Coin::new(0u128, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );

        let err = execute(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            request,
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::InvalidCoinAmount {
                field: "liquidity_coin"
            }
        ));
    }

    #[test]
    fn rejects_empty_denoms() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut(), &owner);
        let request = build_open_interest(
            sample_coin(100, ""),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );

        let err = execute(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            request,
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::InvalidCoinDenom {
                field: "liquidity_coin"
            }
        ));
    }

    #[test]
    fn rejects_zero_expiry_duration() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut(), &owner);
        let request = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            0,
            sample_coin(200, "uatom"),
        );

        let err = execute(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            request,
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::InvalidExpiryDuration {}));
    }

    #[test]
    fn stores_open_interest_when_inputs_valid() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut(), &owner);
        let request = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );

        let response = execute(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            request.clone(),
        )
        .expect("open interest succeeds");

        assert!(response.messages.is_empty());
        assert_eq!(response.attributes.len(), 8);

        let stored = OPEN_INTEREST
            .load(deps.as_ref().storage)
            .expect("interest fetched");

        assert_eq!(stored, Some(request));
    }

    #[test]
    fn close_rejects_non_owner_senders() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut(), &owner);
        let intruder = deps.api.addr_make("intruder");

        let err = close(deps.as_mut(), message_info(&intruder, &[])).unwrap_err();

        assert!(matches!(err, ContractError::Unauthorized {}));
    }

    #[test]
    fn close_requires_active_open_interest() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut(), &owner);

        let err = close(deps.as_mut(), message_info(&owner, &[])).unwrap_err();

        assert!(matches!(err, ContractError::NoOpenInterest {}));
    }

    #[test]
    fn close_rejects_when_lender_present() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut(), &owner);

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
        LENDER
            .save(deps.as_mut().storage, &Some(lender))
            .expect("lender stored");

        let err = close(deps.as_mut(), message_info(&owner, &[])).unwrap_err();

        assert!(matches!(err, ContractError::LenderAlreadySet {}));
    }

    #[test]
    fn close_clears_open_interest_and_emits_attributes() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut(), &owner);

        let request = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );

        OPEN_INTEREST
            .save(deps.as_mut().storage, &Some(request.clone()))
            .expect("open interest stored");

        let response = close(deps.as_mut(), message_info(&owner, &[])).expect("close succeeds");

        assert!(response.messages.is_empty());
        assert_eq!(
            response.attributes[0],
            attr("action", "close_open_interest")
        );

        let stored = OPEN_INTEREST
            .load(deps.as_ref().storage)
            .expect("open interest fetched");

        assert!(stored.is_none());
    }

    #[test]
    fn close_clears_counter_offers() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut(), &owner);

        let request = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );

        OPEN_INTEREST
            .save(deps.as_mut().storage, &Some(request.clone()))
            .expect("open interest stored");

        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &Some(request.liquidity_coin.clone()))
            .expect("debt stored");

        let proposer = deps.api.addr_make("proposer");
        COUNTER_OFFERS
            .save(deps.as_mut().storage, &proposer, &request)
            .expect("counter offer stored");

        let response = close(deps.as_mut(), message_info(&owner, &[])).expect("close succeeds");

        assert_eq!(response.messages.len(), 1);
        let message = &response.messages[0];
        match &message.msg {
            cosmwasm_std::CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                assert_eq!(to_address, proposer.as_str());
                assert_eq!(amount.as_slice(), &[request.liquidity_coin.clone()]);
            }
            msg => panic!("unexpected refund message: {msg:?}"),
        }

        let mut offers = COUNTER_OFFERS.range(deps.as_ref().storage, None, None, Order::Ascending);
        assert!(offers.next().is_none());

        let debt = OUTSTANDING_DEBT
            .load(deps.as_ref().storage)
            .expect("debt queried");
        assert!(debt.is_none());
    }

    #[test]
    fn owner_can_reopen_interest_after_closing_offers() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut(), &owner);

        let initial_request = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );

        execute(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            initial_request.clone(),
        )
        .expect("initial open interest succeeds");

        let proposer = deps.api.addr_make("stale");
        let offer = build_open_interest(
            sample_coin(90, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );
        COUNTER_OFFERS
            .save(deps.as_mut().storage, &proposer, &offer)
            .expect("counter offer stored");
        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &Some(offer.liquidity_coin.clone()))
            .expect("debt stored");

        close(deps.as_mut(), message_info(&owner, &[])).expect("close succeeds");

        let reopened_request = build_open_interest(
            sample_coin(200, "uusd"),
            sample_coin(10, "ujuno"),
            172_800,
            sample_coin(300, "uatom"),
        );

        let response = execute(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            reopened_request.clone(),
        )
        .expect("reopen succeeds");

        assert!(response.messages.is_empty());

        let stored = OPEN_INTEREST
            .load(deps.as_ref().storage)
            .expect("open interest fetched");
        assert_eq!(stored, Some(reopened_request));

        let debt = OUTSTANDING_DEBT
            .load(deps.as_ref().storage)
            .expect("debt fetched");
        assert!(debt.is_none());

        let mut offers = COUNTER_OFFERS.range(deps.as_ref().storage, None, None, Order::Ascending);
        assert!(offers.next().is_none());
    }

    #[test]
    fn close_refunds_multiple_offers_and_clears_debt() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut(), &owner);

        let request = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );

        OPEN_INTEREST
            .save(deps.as_mut().storage, &Some(request.clone()))
            .expect("open interest stored");

        let proposer_a = deps.api.addr_make("proposer-a");
        let proposer_b = deps.api.addr_make("proposer-b");

        let offer_a = build_open_interest(
            sample_coin(90, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );
        let offer_b = build_open_interest(
            sample_coin(80, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );

        COUNTER_OFFERS
            .save(deps.as_mut().storage, &proposer_a, &offer_a)
            .expect("offer A stored");
        COUNTER_OFFERS
            .save(deps.as_mut().storage, &proposer_b, &offer_b)
            .expect("offer B stored");

        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &Some(Coin::new(170u128, "uusd")))
            .expect("debt stored");

        let response = close(deps.as_mut(), message_info(&owner, &[])).expect("close succeeds");

        assert_eq!(response.messages.len(), 2);
        let mut recipients = response
            .messages
            .iter()
            .map(|msg| match &msg.msg {
                cosmwasm_std::CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                    (to_address.as_str(), amount.as_slice())
                }
                msg => panic!("unexpected message: {msg:?}"),
            })
            .collect::<Vec<_>>();

        recipients.sort_by(|a, b| a.0.cmp(b.0));

        let mut expected = vec![
            (
                proposer_a.as_str(),
                std::slice::from_ref(&offer_a.liquidity_coin),
            ),
            (
                proposer_b.as_str(),
                std::slice::from_ref(&offer_b.liquidity_coin),
            ),
        ];
        expected.sort_by(|a, b| a.0.cmp(b.0));

        assert_eq!(recipients, expected);

        let debt = OUTSTANDING_DEBT
            .load(deps.as_ref().storage)
            .expect("debt queried");
        assert!(debt.is_none());
    }

    #[test]
    fn fund_requires_active_open_interest() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut(), &owner);

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
        setup(deps.as_mut(), &owner);

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
        setup(deps.as_mut(), &owner);

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
        setup(deps.as_mut(), &owner);

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
        setup(deps.as_mut(), &owner);

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
}

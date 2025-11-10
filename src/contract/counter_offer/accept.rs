use cosmwasm_std::{
    attr, Addr, BankMsg, Coin, DepsMut, Env, MessageInfo, Order, Response, StdResult,
};

use crate::{
    error::ContractError,
    state::{COUNTER_OFFERS, LENDER, OPEN_INTEREST, OUTSTANDING_DEBT, OWNER},
    types::OpenInterest,
};

pub fn accept(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    proposer: String,
    expected_interest: OpenInterest,
) -> Result<Response, ContractError> {
    let owner = OWNER.load(deps.storage)?;
    if info.sender != owner {
        return Err(ContractError::Unauthorized {});
    }

    OPEN_INTEREST
        .load(deps.storage)?
        .ok_or(ContractError::NoOpenInterest {})?;

    if LENDER.load(deps.storage)?.is_some() {
        return Err(ContractError::LenderAlreadySet {});
    }

    let lender_addr = deps.api.addr_validate(&proposer)?;
    let accepted_offer = COUNTER_OFFERS
        .may_load(deps.storage, &lender_addr)?
        .ok_or_else(|| ContractError::CounterOfferNotFound {
            proposer: proposer.clone(),
        })?;

    if accepted_offer != expected_interest {
        return Err(ContractError::CounterOfferMismatch { proposer });
    }

    let offers = COUNTER_OFFERS
        .range(deps.storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<(Addr, OpenInterest)>>>()?;

    let refunds: Vec<(Addr, Coin)> = offers
        .into_iter()
        .filter_map(|(addr, offer)| {
            if addr == lender_addr {
                None
            } else {
                Some((addr, offer.liquidity_coin))
            }
        })
        .collect();

    COUNTER_OFFERS.clear(deps.storage);

    LENDER.save(deps.storage, &Some(lender_addr.clone()))?;
    OPEN_INTEREST.save(deps.storage, &Some(accepted_offer.clone()))?;
    OUTSTANDING_DEBT.save(deps.storage, &None)?;

    let mut response = Response::new().add_attributes([
        attr("action", "accept_counter_offer"),
        attr("lender", lender_addr.as_str()),
        attr(
            "liquidity_amount",
            accepted_offer.liquidity_coin.amount.to_string(),
        ),
        attr("refunded_offers", refunds.len().to_string()),
    ]);

    for (addr, coin) in refunds {
        response = response.add_message(BankMsg::Send {
            to_address: addr.into_string(),
            amount: vec![coin],
        });
    }

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::counter_offer::propose;
    use crate::contract::counter_offer::test_helpers::setup_open_interest;
    use crate::error::ContractError;
    use crate::state::{COUNTER_OFFERS, LENDER, OPEN_INTEREST, OUTSTANDING_DEBT};
    use crate::types::OpenInterest;
    use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env};
    use cosmwasm_std::{attr, BankMsg, Coin, CosmosMsg, Order, Uint256};

    #[test]
    fn owner_can_accept_counter_offer() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let active = setup_open_interest(deps.as_mut(), &owner);

        let accepted = deps.api.addr_make("accepted");
        let mut accepted_offer = active.clone();
        accepted_offer.liquidity_coin.amount = accepted_offer
            .liquidity_coin
            .amount
            .checked_sub(Uint256::from(50u128))
            .expect("amount stays positive");

        let rival = deps.api.addr_make("rival");
        let mut rival_offer = active.clone();
        rival_offer.liquidity_coin.amount = rival_offer
            .liquidity_coin
            .amount
            .checked_sub(Uint256::from(100u128))
            .expect("amount stays positive");

        propose(
            deps.as_mut(),
            mock_env(),
            message_info(&accepted, &[accepted_offer.liquidity_coin.clone()]),
            accepted_offer.clone(),
        )
        .expect("accepted proposer funds escrow");

        propose(
            deps.as_mut(),
            mock_env(),
            message_info(&rival, &[rival_offer.liquidity_coin.clone()]),
            rival_offer.clone(),
        )
        .expect("rival funds escrow");

        let response = accept(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            accepted.to_string(),
            accepted_offer.clone(),
        )
        .expect("owner accepts offer");

        assert_eq!(
            response.attributes[0],
            attr("action", "accept_counter_offer")
        );

        assert_eq!(response.messages.len(), 1);
        let mut payouts: Vec<(String, Coin)> = response
            .messages
            .into_iter()
            .map(|msg| match msg.msg {
                CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                    assert_eq!(amount.len(), 1);
                    (to_address, amount[0].clone())
                }
                other => panic!("unexpected message: {:?}", other),
            })
            .collect();
        payouts.sort_by(|(addr_a, _), (addr_b, _)| addr_a.cmp(addr_b));

        let rival_str = rival.to_string();

        assert_eq!(
            payouts,
            vec![(rival_str, rival_offer.liquidity_coin.clone())]
        );

        let lender = LENDER.load(deps.as_ref().storage).expect("lender stored");
        assert_eq!(lender, Some(accepted.clone()));

        let stored_interest = OPEN_INTEREST
            .load(deps.as_ref().storage)
            .expect("open interest stored")
            .expect("open interest active");
        assert_eq!(stored_interest, accepted_offer);

        let debt = OUTSTANDING_DEBT
            .load(deps.as_ref().storage)
            .expect("debt stored");
        assert!(debt.is_none());

        let mut remaining =
            COUNTER_OFFERS.range(deps.as_ref().storage, None, None, Order::Ascending);
        assert!(remaining.next().is_none());
    }

    #[test]
    fn accepting_solo_offer_clears_debt_without_refunds() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let active = setup_open_interest(deps.as_mut(), &owner);

        let accepted = deps.api.addr_make("accepted");
        let mut accepted_offer = active.clone();
        accepted_offer.liquidity_coin.amount = accepted_offer
            .liquidity_coin
            .amount
            .checked_sub(Uint256::from(10u128))
            .expect("amount stays positive");

        propose(
            deps.as_mut(),
            mock_env(),
            message_info(&accepted, &[accepted_offer.liquidity_coin.clone()]),
            accepted_offer.clone(),
        )
        .expect("accepted proposer funds escrow");

        let response = accept(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            accepted.to_string(),
            accepted_offer.clone(),
        )
        .expect("owner accepts offer");

        assert_eq!(response.messages.len(), 0, "no refunds expected");

        let lender = LENDER.load(deps.as_ref().storage).expect("lender stored");
        assert_eq!(lender, Some(accepted));

        let stored_interest = OPEN_INTEREST
            .load(deps.as_ref().storage)
            .expect("open interest stored")
            .expect("open interest active");
        assert_eq!(stored_interest, accepted_offer);

        let debt = OUTSTANDING_DEBT
            .load(deps.as_ref().storage)
            .expect("debt stored");
        assert!(debt.is_none(), "debt cleared after acceptance");
    }

    #[test]
    fn accept_requires_owner() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let active = setup_open_interest(deps.as_mut(), &owner);
        let proposer = deps.api.addr_make("proposer");
        let mut offer = active.clone();
        offer.liquidity_coin.amount = offer
            .liquidity_coin
            .amount
            .checked_sub(Uint256::from(25u128))
            .expect("amount stays positive");

        propose(
            deps.as_mut(),
            mock_env(),
            message_info(&proposer, &[offer.liquidity_coin.clone()]),
            offer.clone(),
        )
        .expect("proposal stored");

        let intruder = deps.api.addr_make("intruder");
        let err = accept(
            deps.as_mut(),
            mock_env(),
            message_info(&intruder, &[]),
            proposer.to_string(),
            offer,
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::Unauthorized {}));
    }

    #[test]
    fn accept_rejects_when_offer_missing() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let active = setup_open_interest(deps.as_mut(), &owner);
        let proposer = deps.api.addr_make("proposer");
        let mut offer = active.clone();
        offer.liquidity_coin.amount = offer
            .liquidity_coin
            .amount
            .checked_sub(Uint256::from(10u128))
            .expect("amount stays positive");

        propose(
            deps.as_mut(),
            mock_env(),
            message_info(&proposer, &[offer.liquidity_coin.clone()]),
            offer.clone(),
        )
        .expect("proposal stored");

        let missing = deps.api.addr_make("missing");
        let err = accept(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            missing.to_string(),
            offer,
        )
        .unwrap_err();

        match err {
            ContractError::CounterOfferNotFound { proposer } => {
                assert_eq!(proposer, missing.to_string());
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn accept_rejects_without_open_interest() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let _active = setup_open_interest(deps.as_mut(), &owner);
        let proposer = deps.api.addr_make("proposer");
        let offer = OpenInterest {
            liquidity_coin: Coin::new(900u128, "uusd"),
            interest_coin: Coin::new(50u128, "ujuno"),
            expiry_duration: 86_400u64,
            collateral: Coin::new(2_000u128, "uatom"),
        };

        propose(
            deps.as_mut(),
            mock_env(),
            message_info(&proposer, &[offer.liquidity_coin.clone()]),
            offer.clone(),
        )
        .expect("proposal stored");

        OPEN_INTEREST
            .save(deps.as_mut().storage, &None)
            .expect("clear open interest");

        let err = accept(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            proposer.to_string(),
            offer,
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::NoOpenInterest {}));
    }

    #[test]
    fn accept_rejects_when_lender_already_set() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let active = setup_open_interest(deps.as_mut(), &owner);
        let proposer = deps.api.addr_make("proposer");
        let mut offer = active.clone();
        offer.liquidity_coin.amount = offer
            .liquidity_coin
            .amount
            .checked_sub(Uint256::from(15u128))
            .expect("amount stays positive");

        propose(
            deps.as_mut(),
            mock_env(),
            message_info(&proposer, &[offer.liquidity_coin.clone()]),
            offer.clone(),
        )
        .expect("proposal stored");

        LENDER
            .save(deps.as_mut().storage, &Some(proposer.clone()))
            .expect("preset lender");

        let err = accept(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            proposer.to_string(),
            offer.clone(),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::LenderAlreadySet {}));
    }

    #[test]
    fn accept_rejects_mismatched_payload() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let active = setup_open_interest(deps.as_mut(), &owner);
        let proposer = deps.api.addr_make("proposer");
        let mut offer = active.clone();
        offer.liquidity_coin.amount = offer
            .liquidity_coin
            .amount
            .checked_sub(Uint256::from(20u128))
            .expect("amount stays positive");

        propose(
            deps.as_mut(),
            mock_env(),
            message_info(&proposer, &[offer.liquidity_coin.clone()]),
            offer.clone(),
        )
        .expect("proposal stored");

        let mut tampered = offer.clone();
        tampered.liquidity_coin.amount = tampered
            .liquidity_coin
            .amount
            .checked_sub(Uint256::from(1u128))
            .expect("positive amount");

        let err = accept(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            proposer.to_string(),
            tampered,
        )
        .unwrap_err();

        match err {
            ContractError::CounterOfferMismatch { proposer: culprit } => {
                assert_eq!(culprit, proposer.to_string());
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }
}

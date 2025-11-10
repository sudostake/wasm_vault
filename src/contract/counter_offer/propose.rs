use cosmwasm_std::{attr, BankMsg, DepsMut, Env, MessageInfo, Response};

use crate::{
    error::ContractError,
    state::{COUNTER_OFFERS, LENDER, OPEN_INTEREST},
    types::OpenInterest,
};

use super::helpers::{
    add_outstanding_debt, determine_eviction_candidate, release_outstanding_debt,
    validate_counter_offer, validate_counter_offer_escrow,
};

pub fn propose(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    proposed_interest: OpenInterest,
) -> Result<Response, ContractError> {
    let active_interest = OPEN_INTEREST
        .load(deps.storage)?
        .ok_or(ContractError::NoOpenInterest {})?;

    if LENDER.load(deps.storage)?.is_some() {
        return Err(ContractError::LenderAlreadySet {});
    }

    validate_counter_offer(&active_interest, &proposed_interest)?;
    validate_counter_offer_escrow(&info, &proposed_interest)?;

    if COUNTER_OFFERS
        .may_load(deps.storage, &info.sender)?
        .is_some()
    {
        return Err(ContractError::CounterOfferAlreadyExists {});
    }

    let eviction_candidate = determine_eviction_candidate(deps.storage, &proposed_interest)?;

    if let Some((addr, offer)) = &eviction_candidate {
        COUNTER_OFFERS.remove(deps.storage, addr);
        release_outstanding_debt(deps.storage, &offer.liquidity_coin)?;
    }

    add_outstanding_debt(deps.storage, &proposed_interest.liquidity_coin)?;
    COUNTER_OFFERS.save(deps.storage, &info.sender, &proposed_interest)?;

    let mut response = Response::new().add_attributes([
        attr("action", "propose_counter_offer"),
        attr("proposer", info.sender.as_str()),
        attr(
            "liquidity_amount",
            proposed_interest.liquidity_coin.amount.to_string(),
        ),
    ]);

    if let Some((addr, offer)) = eviction_candidate {
        response = response
            .add_attribute("evicted_proposer", addr.as_str())
            .add_message(BankMsg::Send {
                to_address: addr.to_string(),
                amount: vec![offer.liquidity_coin.clone()],
            });
    }

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::counter_offer::test_helpers::setup_open_interest;
    use crate::error::ContractError;
    use crate::state::{
        COUNTER_OFFERS, LENDER, MAX_COUNTER_OFFERS, OPEN_INTEREST, OUTSTANDING_DEBT,
    };
    use crate::types::OpenInterest;
    use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env};
    use cosmwasm_std::{attr, Addr, BankMsg, Coin, CosmosMsg, Uint256};

    #[test]
    fn rejects_without_active_open_interest() {
        let mut deps = mock_dependencies();
        let proposer = deps.api.addr_make("proposer");
        OPEN_INTEREST
            .save(deps.as_mut().storage, &None)
            .expect("open interest initialized");

        let err = propose(
            deps.as_mut(),
            mock_env(),
            message_info(&proposer, &[]),
            OpenInterest {
                liquidity_coin: Coin::new(900u128, "uusd"),
                interest_coin: Coin::new(50u128, "ujuno"),
                expiry_duration: 86_400u64,
                collateral: Coin::new(2_000u128, "uatom"),
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::NoOpenInterest {}));
    }

    #[test]
    fn rejects_when_lender_present() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let active = setup_open_interest(deps.as_mut(), &owner);
        let lender = deps.api.addr_make("lender");
        LENDER.save(deps.as_mut().storage, &Some(lender)).unwrap();

        let proposer = deps.api.addr_make("proposer");
        let err = propose(
            deps.as_mut(),
            mock_env(),
            message_info(&proposer, &[]),
            OpenInterest {
                liquidity_coin: {
                    let mut coin = active.liquidity_coin.clone();
                    coin.amount = coin
                        .amount
                        .checked_sub(Uint256::from(10u128))
                        .expect("amount remains positive");
                    coin
                },
                interest_coin: active.interest_coin.clone(),
                expiry_duration: active.expiry_duration,
                collateral: active.collateral.clone(),
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::LenderAlreadySet {}));
    }

    #[test]
    fn rejects_mismatched_terms() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let active = setup_open_interest(deps.as_mut(), &owner);
        let proposer = deps.api.addr_make("proposer");

        let err = propose(
            deps.as_mut(),
            mock_env(),
            message_info(&proposer, &[]),
            OpenInterest {
                liquidity_coin: Coin::new(900u128, "uusd"),
                interest_coin: Coin::new(55u128, "ujuno"),
                expiry_duration: active.expiry_duration,
                collateral: active.collateral.clone(),
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::CounterOfferTermsMismatch {}));
    }

    #[test]
    fn rejects_non_lower_amounts() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let active = setup_open_interest(deps.as_mut(), &owner);
        let proposer = deps.api.addr_make("proposer");

        let err = propose(
            deps.as_mut(),
            mock_env(),
            message_info(&proposer, &[]),
            OpenInterest {
                liquidity_coin: active.liquidity_coin.clone(),
                interest_coin: active.interest_coin.clone(),
                expiry_duration: active.expiry_duration,
                collateral: active.collateral.clone(),
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::CounterOfferNotSmaller {}));
    }

    #[test]
    fn rejects_missing_escrow_deposit() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let active = setup_open_interest(deps.as_mut(), &owner);
        let proposer = deps.api.addr_make("proposer");
        let offer = OpenInterest {
            liquidity_coin: {
                let mut coin = active.liquidity_coin.clone();
                coin.amount = coin
                    .amount
                    .checked_sub(Uint256::from(10u128))
                    .expect("amount remains positive");
                coin
            },
            interest_coin: active.interest_coin.clone(),
            expiry_duration: active.expiry_duration,
            collateral: active.collateral.clone(),
        };

        let err = propose(
            deps.as_mut(),
            mock_env(),
            message_info(&proposer, &[]),
            offer,
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::CounterOfferEscrowMismatch { .. }
        ));
    }

    #[test]
    fn rejects_incorrect_escrow_amount() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let active = setup_open_interest(deps.as_mut(), &owner);
        let proposer = deps.api.addr_make("proposer");
        let offer = OpenInterest {
            liquidity_coin: {
                let mut coin = active.liquidity_coin.clone();
                coin.amount = coin
                    .amount
                    .checked_sub(Uint256::from(10u128))
                    .expect("amount remains positive");
                coin
            },
            interest_coin: active.interest_coin.clone(),
            expiry_duration: active.expiry_duration,
            collateral: active.collateral.clone(),
        };

        let smaller_amount = offer
            .liquidity_coin
            .amount
            .checked_sub(Uint256::from(1u128))
            .expect("amount remains positive");
        let mut insufficient_deposit = offer.liquidity_coin.clone();
        insufficient_deposit.amount = smaller_amount;
        let funds = vec![insufficient_deposit];

        let err = propose(
            deps.as_mut(),
            mock_env(),
            message_info(&proposer, &funds),
            offer,
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::CounterOfferEscrowMismatch { .. }
        ));
    }

    #[test]
    fn rejects_duplicate_counter_offers_from_same_proposer() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let active = setup_open_interest(deps.as_mut(), &owner);
        let proposer = deps.api.addr_make("proposer");
        let offer = OpenInterest {
            liquidity_coin: {
                let mut coin = active.liquidity_coin.clone();
                coin.amount = coin
                    .amount
                    .checked_sub(Uint256::from(25u128))
                    .expect("amount remains positive");
                coin
            },
            interest_coin: active.interest_coin.clone(),
            expiry_duration: active.expiry_duration,
            collateral: active.collateral.clone(),
        };

        let funds = vec![offer.liquidity_coin.clone()];

        propose(
            deps.as_mut(),
            mock_env(),
            message_info(&proposer, &funds),
            offer.clone(),
        )
        .expect("first proposal succeeds");

        let err = propose(
            deps.as_mut(),
            mock_env(),
            message_info(&proposer, &funds),
            offer,
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::CounterOfferAlreadyExists {}));
    }

    #[test]
    fn accrues_outstanding_debt_for_each_offer() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let active = setup_open_interest(deps.as_mut(), &owner);

        let proposer_a = deps.api.addr_make("proposer-a");
        let offer_a = OpenInterest {
            liquidity_coin: {
                let mut coin = active.liquidity_coin.clone();
                coin.amount = coin
                    .amount
                    .checked_sub(Uint256::from(10u128))
                    .expect("amount remains positive");
                coin
            },
            interest_coin: active.interest_coin.clone(),
            expiry_duration: active.expiry_duration,
            collateral: active.collateral.clone(),
        };

        propose(
            deps.as_mut(),
            mock_env(),
            message_info(&proposer_a, &[offer_a.liquidity_coin.clone()]),
            offer_a.clone(),
        )
        .expect("first offer succeeds");

        let debt = OUTSTANDING_DEBT
            .load(deps.as_ref().storage)
            .expect("load debt")
            .expect("debt present");
        assert_eq!(debt.amount, offer_a.liquidity_coin.amount);
        assert_eq!(debt.denom, offer_a.liquidity_coin.denom);

        let proposer_b = deps.api.addr_make("proposer-b");
        let offer_b = OpenInterest {
            liquidity_coin: {
                let mut coin = active.liquidity_coin.clone();
                coin.amount = coin
                    .amount
                    .checked_sub(Uint256::from(25u128))
                    .expect("amount remains positive");
                coin
            },
            interest_coin: active.interest_coin.clone(),
            expiry_duration: active.expiry_duration,
            collateral: active.collateral.clone(),
        };

        propose(
            deps.as_mut(),
            mock_env(),
            message_info(&proposer_b, &[offer_b.liquidity_coin.clone()]),
            offer_b.clone(),
        )
        .expect("second offer succeeds");

        let debt = OUTSTANDING_DEBT
            .load(deps.as_ref().storage)
            .expect("load debt")
            .expect("debt present");
        let expected_amount = offer_a
            .liquidity_coin
            .amount
            .checked_add(offer_b.liquidity_coin.amount)
            .expect("sum fits");
        assert_eq!(debt.amount, expected_amount);
        assert_eq!(debt.denom, offer_b.liquidity_coin.denom);
    }

    #[test]
    fn stores_offer_and_evicts_smallest_when_full() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let active = setup_open_interest(deps.as_mut(), &owner);
        let mut expected_debt = Uint256::zero();
        let mut lowest_offer: Option<(Addr, Coin)> = None;

        for i in 0..MAX_COUNTER_OFFERS {
            let proposer = deps.api.addr_make(&format!("proposer{i}"));
            let decrement = Uint256::from(10u128 + i as u128);
            let amount = active
                .liquidity_coin
                .amount
                .checked_sub(decrement)
                .expect("amount stays positive");
            let offer = OpenInterest {
                liquidity_coin: Coin::new(amount, "uusd"),
                interest_coin: active.interest_coin.clone(),
                expiry_duration: active.expiry_duration,
                collateral: active.collateral.clone(),
            };

            let refund_coin = offer.liquidity_coin.clone();
            let response = propose(
                deps.as_mut(),
                mock_env(),
                {
                    let funds = vec![refund_coin.clone()];
                    message_info(&proposer, &funds)
                },
                offer,
            )
            .expect("proposal succeeds");

            assert_eq!(
                response.attributes[0],
                attr("action", "propose_counter_offer")
            );

            expected_debt = expected_debt
                .checked_add(refund_coin.amount)
                .expect("debt sum fits");

            assert!(response.messages.is_empty());

            let replace_lowest = match &lowest_offer {
                Some((worst_addr, worst_coin)) => {
                    refund_coin.amount < worst_coin.amount
                        || (refund_coin.amount == worst_coin.amount
                            && proposer.as_str() < worst_addr.as_str())
                }
                None => true,
            };
            if replace_lowest {
                lowest_offer = Some((proposer.clone(), refund_coin.clone()));
            }

            let debt = OUTSTANDING_DEBT
                .load(deps.as_ref().storage)
                .expect("load succeeds")
                .expect("debt present");
            assert_eq!(debt.amount, expected_debt);
            assert_eq!(debt.denom, active.liquidity_coin.denom);
        }

        let (evicted_addr, evicted_coin) = lowest_offer.expect("worst offer recorded");
        let better_proposer = deps.api.addr_make("better-proposer");
        let better_offer = OpenInterest {
            liquidity_coin: {
                let mut coin = active.liquidity_coin.clone();
                coin.amount = coin
                    .amount
                    .checked_sub(Uint256::from(5u128))
                    .expect("amount stays positive");
                coin
            },
            interest_coin: active.interest_coin.clone(),
            expiry_duration: active.expiry_duration,
            collateral: active.collateral.clone(),
        };

        let response = propose(
            deps.as_mut(),
            mock_env(),
            message_info(&better_proposer, &[better_offer.liquidity_coin.clone()]),
            better_offer.clone(),
        )
        .expect("better proposal succeeds");

        assert_eq!(response.messages.len(), 1);
        let msg = response.messages[0].clone().msg;
        match msg {
            CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                assert_eq!(to_address, evicted_addr.to_string());
                assert_eq!(amount, vec![evicted_coin.clone()]);
            }
            _ => panic!("unexpected message"),
        }

        expected_debt = expected_debt
            .checked_add(better_offer.liquidity_coin.amount)
            .expect("debt increment fits")
            .checked_sub(evicted_coin.amount)
            .expect("debt decrement fits");
        let debt = OUTSTANDING_DEBT
            .load(deps.as_ref().storage)
            .expect("load succeeds")
            .expect("debt present");
        assert_eq!(debt.amount, expected_debt);
        assert_eq!(debt.denom, active.liquidity_coin.denom);

        let stored_evicted = COUNTER_OFFERS
            .may_load(deps.as_ref().storage, &evicted_addr)
            .expect("load succeeds");
        assert!(stored_evicted.is_none());

        let stored_new = COUNTER_OFFERS
            .may_load(deps.as_ref().storage, &better_proposer)
            .expect("load succeeds");
        assert!(stored_new.is_some());
    }

    #[test]
    fn rejects_offer_that_would_be_immediately_evicted() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let active = setup_open_interest(deps.as_mut(), &owner);

        let mut lowest_amount: Option<Uint256> = None;

        for i in 0..MAX_COUNTER_OFFERS {
            let proposer = deps.api.addr_make(&format!("proposer{i}"));
            let decrement = Uint256::from(20u128 + i as u128);
            let amount = active
                .liquidity_coin
                .amount
                .checked_sub(decrement)
                .expect("amount stays positive");
            let offer = OpenInterest {
                liquidity_coin: Coin::new(amount, "uusd"),
                interest_coin: active.interest_coin.clone(),
                expiry_duration: active.expiry_duration,
                collateral: active.collateral.clone(),
            };

            lowest_amount = match lowest_amount {
                Some(current) if current <= amount => Some(current),
                _ => Some(amount),
            };

            propose(
                deps.as_mut(),
                mock_env(),
                message_info(&proposer, &[offer.liquidity_coin.clone()]),
                offer,
            )
            .expect("setup proposal succeeds");
        }

        let mut low_offer = active.clone();
        let min_amount = lowest_amount.expect("lowest amount exists");
        low_offer.liquidity_coin.amount = min_amount
            .checked_sub(Uint256::from(1u128))
            .expect("remains positive");

        let late_proposer = deps.api.addr_make("late-proposer");
        let err = propose(
            deps.as_mut(),
            mock_env(),
            message_info(&late_proposer, &[low_offer.liquidity_coin.clone()]),
            low_offer,
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::CounterOfferNotCompetitive { .. }
        ));
    }

    #[test]
    fn rejects_equal_amount_when_full() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let active = setup_open_interest(deps.as_mut(), &owner);

        let mut lowest_amount: Option<Uint256> = None;

        for i in 0..MAX_COUNTER_OFFERS {
            let proposer = deps.api.addr_make(&format!("proposer{i}"));
            let decrement = Uint256::from(15u128 + i as u128);
            let amount = active
                .liquidity_coin
                .amount
                .checked_sub(decrement)
                .expect("amount stays positive");
            let offer = OpenInterest {
                liquidity_coin: Coin::new(amount, "uusd"),
                interest_coin: active.interest_coin.clone(),
                expiry_duration: active.expiry_duration,
                collateral: active.collateral.clone(),
            };

            lowest_amount = match lowest_amount {
                Some(current) if current <= amount => Some(current),
                _ => Some(amount),
            };

            propose(
                deps.as_mut(),
                mock_env(),
                message_info(&proposer, &[offer.liquidity_coin.clone()]),
                offer,
            )
            .expect("setup proposal succeeds");
        }

        let matching_amount = lowest_amount.expect("lowest amount exists");
        let mut equal_offer = active.clone();
        equal_offer.liquidity_coin.amount = matching_amount;

        let late_proposer = deps.api.addr_make("late-proposer");
        let err = propose(
            deps.as_mut(),
            mock_env(),
            message_info(&late_proposer, &[equal_offer.liquidity_coin.clone()]),
            equal_offer,
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::CounterOfferNotCompetitive { .. }
        ));
    }
}

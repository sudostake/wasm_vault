use cosmwasm_std::{
    attr, Addr, BankMsg, Coin, DepsMut, Env, MessageInfo, Order, Response, StdError, StdResult,
    Uint256,
};

use crate::{
    error::ContractError,
    state::{COUNTER_OFFERS, LENDER, MAX_COUNTER_OFFERS, OPEN_INTEREST, OUTSTANDING_DEBT, OWNER},
    types::OpenInterest,
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

/// Lets the vault owner accept a specific counter offer, identified by the `proposer: String` and
/// `expected_interest: OpenInterest` parameters.
/// Verifies ownership and proposal terms, refunds every other escrowed bidder, updates the lender
/// and open-interest state, and clears outstanding debt since only the winning liquidity remains locked.
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

fn validate_counter_offer(
    active: &OpenInterest,
    proposed: &OpenInterest,
) -> Result<(), ContractError> {
    if proposed.liquidity_coin.denom != active.liquidity_coin.denom
        || proposed.interest_coin != active.interest_coin
        || proposed.collateral != active.collateral
        || proposed.expiry_duration != active.expiry_duration
    {
        return Err(ContractError::CounterOfferTermsMismatch {});
    }

    if proposed.liquidity_coin.amount.is_zero() {
        return Err(ContractError::InvalidCoinAmount {
            field: "liquidity_coin",
        });
    }

    if proposed.liquidity_coin.amount >= active.liquidity_coin.amount {
        return Err(ContractError::CounterOfferNotSmaller {});
    }

    Ok(())
}

fn validate_counter_offer_escrow(
    info: &MessageInfo,
    proposed: &OpenInterest,
) -> Result<(), ContractError> {
    let denom = &proposed.liquidity_coin.denom;
    let expected = proposed.liquidity_coin.amount;
    let received = info
        .funds
        .iter()
        .filter(|coin| coin.denom == *denom)
        .fold(Uint256::zero(), |acc, coin| acc + coin.amount);

    if received != expected {
        return Err(ContractError::CounterOfferEscrowMismatch {
            denom: denom.clone(),
            expected,
            received,
        });
    }

    Ok(())
}

fn add_outstanding_debt(storage: &mut dyn cosmwasm_std::Storage, coin: &Coin) -> StdResult<()> {
    let current = OUTSTANDING_DEBT.may_load(storage)?.flatten();

    let updated = match current {
        Some(mut debt) => {
            if debt.denom != coin.denom {
                return Err(StdError::msg("Outstanding debt denom mismatch"));
            }
            debt.amount = debt.amount.checked_add(coin.amount)?;
            Some(debt)
        }
        None => Some(coin.clone()),
    };

    OUTSTANDING_DEBT.save(storage, &updated)?;
    Ok(())
}

fn release_outstanding_debt(storage: &mut dyn cosmwasm_std::Storage, coin: &Coin) -> StdResult<()> {
    let mut debt = OUTSTANDING_DEBT
        .may_load(storage)?
        .flatten()
        .ok_or_else(|| StdError::msg("No outstanding debt to release"))?;

    if debt.denom != coin.denom {
        return Err(StdError::msg("Outstanding debt denom mismatch"));
    }

    debt.amount = debt.amount.checked_sub(coin.amount)?;
    let updated = if debt.amount.is_zero() {
        None
    } else {
        Some(debt)
    };

    OUTSTANDING_DEBT.save(storage, &updated)?;
    Ok(())
}

fn determine_eviction_candidate(
    storage: &mut dyn cosmwasm_std::Storage,
    proposed: &OpenInterest,
) -> Result<Option<(Addr, OpenInterest)>, ContractError> {
    let snapshot = snapshot_counter_offer_capacity(storage)?;
    let Some((count, (worst_addr, worst_offer))) = snapshot else {
        return Ok(None);
    };
    let max_capacity = MAX_COUNTER_OFFERS;

    if count < max_capacity {
        return Ok(None);
    }

    let new_amount = proposed.liquidity_coin.amount;
    let worst_amount = worst_offer.liquidity_coin.amount;

    let new_is_worse = new_amount <= worst_amount;
    if new_is_worse {
        return Err(ContractError::CounterOfferNotCompetitive {
            minimum: worst_amount,
            denom: proposed.liquidity_coin.denom.clone(),
        });
    }

    Ok(Some((worst_addr, worst_offer)))
}

fn snapshot_counter_offer_capacity(
    storage: &mut dyn cosmwasm_std::Storage,
) -> StdResult<Option<(u8, (Addr, OpenInterest))>> {
    let mut entries = COUNTER_OFFERS.range(storage, None, None, Order::Ascending);
    let first = match entries.next() {
        Some(entry) => entry?,
        None => return Ok(None),
    };

    let mut count: u8 = 1;
    let mut worst = first;

    for entry in entries {
        let (addr, interest) = entry?;
        count += 1;
        let amount = interest.liquidity_coin.amount;
        let (ref _worst_addr, ref worst_interest) = worst;
        let worst_amount = worst_interest.liquidity_coin.amount;

        let should_replace = amount < worst_amount;

        if should_replace {
            worst = (addr, interest);
        }
    }

    Ok(Some((count, worst)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{COUNTER_OFFERS, LENDER, OPEN_INTEREST, OUTSTANDING_DEBT, OWNER};
    use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env};
    use cosmwasm_std::{attr, BankMsg, Coin, Uint256};

    fn setup_open_interest(deps: DepsMut, owner: &Addr) -> OpenInterest {
        let interest = OpenInterest {
            liquidity_coin: Coin::new(1_000u128, "uusd"),
            interest_coin: Coin::new(50u128, "ujuno"),
            expiry_duration: 86_400u64,
            collateral: Coin::new(2_000u128, "uatom"),
        };

        OWNER.save(deps.storage, owner).unwrap();
        OUTSTANDING_DEBT.save(deps.storage, &None).unwrap();
        LENDER.save(deps.storage, &None).unwrap();
        OPEN_INTEREST
            .save(deps.storage, &Some(interest.clone()))
            .unwrap();

        interest
    }

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
            cosmwasm_std::CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
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
        let mut payouts: Vec<(String, cosmwasm_std::Coin)> = response
            .messages
            .into_iter()
            .map(|msg| match msg.msg {
                cosmwasm_std::CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
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

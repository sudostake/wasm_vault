use cosmwasm_std::{
    attr, Addr, BankMsg, DepsMut, Env, MessageInfo, Order, Response, StdResult, Uint256,
};

use crate::{
    error::ContractError,
    state::{COUNTER_OFFERS, LENDER, MAX_COUNTER_OFFERS, OPEN_INTEREST},
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

    COUNTER_OFFERS.save(deps.storage, &info.sender, &proposed_interest)?;

    let evicted = enforce_capacity(deps.storage)?;

    let mut response = Response::new().add_attributes([
        attr("action", "propose_counter_offer"),
        attr("proposer", info.sender.as_str()),
        attr(
            "liquidity_amount",
            proposed_interest.liquidity_coin.amount.to_string(),
        ),
    ]);

    if let Some((addr, offer)) = evicted {
        response = response
            .add_attribute("evicted_proposer", addr.as_str())
            .add_message(BankMsg::Send {
                to_address: addr.to_string(),
                amount: vec![offer.liquidity_coin.clone()],
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
    let expected = Uint256::from(proposed.liquidity_coin.amount);
    let received = info
        .funds
        .iter()
        .filter(|coin| coin.denom == *denom)
        .fold(Uint256::zero(), |acc, coin| {
            acc + Uint256::from(coin.amount)
        });

    if received != expected {
        return Err(ContractError::CounterOfferEscrowMismatch {
            denom: denom.clone(),
            expected,
            received,
        });
    }

    Ok(())
}

fn enforce_capacity(
    storage: &mut dyn cosmwasm_std::Storage,
) -> StdResult<Option<(Addr, OpenInterest)>> {
    let mut count: u16 = 0;
    let mut worst: Option<(Addr, OpenInterest)> = None;

    for entry in COUNTER_OFFERS.range(storage, None, None, Order::Ascending) {
        let (addr, interest) = entry?;
        count += 1;
        let amount = interest.liquidity_coin.amount;

        let should_replace = match &worst {
            Some((worst_addr, worst_interest)) => {
                let worst_amount = worst_interest.liquidity_coin.amount;
                amount < worst_amount
                    || (amount == worst_amount && addr.as_str() < worst_addr.as_str())
            }
            None => true,
        };

        if should_replace {
            worst = Some((addr, interest));
        }
    }

    if count as u16 <= MAX_COUNTER_OFFERS as u16 {
        return Ok(None);
    }

    if let Some((addr, offer)) = worst {
        COUNTER_OFFERS.remove(storage, &addr);
        Ok(Some((addr, offer)))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{COUNTER_OFFERS, LENDER, OPEN_INTEREST, OWNER};
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
    fn stores_offer_and_evicts_smallest_when_full() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let active = setup_open_interest(deps.as_mut(), &owner);

        for i in 0..=MAX_COUNTER_OFFERS {
            let proposer = deps.api.addr_make(&format!("proposer{i}"));
            let decrement = Uint256::from(1u128 + i as u128);
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

            if i == MAX_COUNTER_OFFERS {
                assert_eq!(response.messages.len(), 1);
                let msg = response.messages[0].clone().msg;
                match msg {
                    cosmwasm_std::CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                        assert_eq!(to_address, proposer.to_string());
                        assert_eq!(amount, vec![refund_coin]);
                    }
                    _ => panic!("unexpected message"),
                }
            } else {
                assert!(response.messages.is_empty());
            }
        }

        let smallest = deps
            .api
            .addr_make(&format!("proposer{}", MAX_COUNTER_OFFERS));
        let stored = COUNTER_OFFERS
            .may_load(deps.as_ref().storage, &smallest)
            .expect("load succeeds");
        assert!(stored.is_none());
    }
}

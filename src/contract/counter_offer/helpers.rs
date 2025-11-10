use cosmwasm_std::{Addr, Coin, MessageInfo, Order, StdError, StdResult, Storage, Uint256};

use crate::{
    error::ContractError,
    state::{COUNTER_OFFERS, MAX_COUNTER_OFFERS, OUTSTANDING_DEBT},
    types::OpenInterest,
};

pub(crate) fn validate_counter_offer(
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

pub(crate) fn validate_counter_offer_escrow(
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

pub(crate) fn add_outstanding_debt(storage: &mut dyn Storage, coin: &Coin) -> StdResult<()> {
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

pub(crate) fn release_outstanding_debt(storage: &mut dyn Storage, coin: &Coin) -> StdResult<()> {
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

pub(crate) fn determine_eviction_candidate(
    storage: &mut dyn Storage,
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
    storage: &mut dyn Storage,
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

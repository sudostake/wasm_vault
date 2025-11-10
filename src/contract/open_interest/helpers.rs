use cosmwasm_std::{
    Addr, BankMsg, Coin, MessageInfo, Order, StdError, StdResult, Storage, Uint128, Uint256,
};
use std::collections::{btree_map::Entry, BTreeMap};
use std::convert::TryFrom;

use crate::{
    state::{COUNTER_OFFERS, OUTSTANDING_DEBT},
    types::OpenInterest,
    ContractError,
};

pub(crate) fn validate_open_interest(open_interest: &OpenInterest) -> Result<(), ContractError> {
    validate_coin(&open_interest.liquidity_coin, "liquidity_coin")?;
    validate_coin(&open_interest.interest_coin, "interest_coin")?;
    validate_coin(&open_interest.collateral, "collateral")?;

    if open_interest.expiry_duration == 0 {
        return Err(ContractError::InvalidExpiryDuration {});
    }

    validate_repayment_limits(open_interest)?;

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

fn validate_repayment_limits(open_interest: &OpenInterest) -> Result<(), ContractError> {
    build_repayment_amounts(open_interest)?;
    Ok(())
}

pub(crate) fn build_repayment_amounts(
    open_interest: &OpenInterest,
) -> Result<Vec<(String, Uint256, Uint128)>, ContractError> {
    let requirements = repayment_requirements(open_interest).map_err(ContractError::Std)?;

    requirements
        .into_iter()
        .map(|(denom, amount)| {
            let coin_amount =
                Uint128::try_from(amount).map_err(|_| ContractError::RepaymentAmountOverflow {
                    denom: denom.clone(),
                    requested: amount,
                })?;

            Ok((denom, amount, coin_amount))
        })
        .collect()
}

pub(crate) fn validate_liquidity_funding(
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

pub(crate) fn refund_counter_offer_escrow(storage: &mut dyn Storage) -> StdResult<Vec<BankMsg>> {
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

fn repayment_requirements(open_interest: &OpenInterest) -> StdResult<BTreeMap<String, Uint256>> {
    let mut requirements = BTreeMap::new();
    accumulate_repayment_requirement(&mut requirements, &open_interest.liquidity_coin)?;
    accumulate_repayment_requirement(&mut requirements, &open_interest.interest_coin)?;
    Ok(requirements)
}

fn accumulate_repayment_requirement(
    requirements: &mut BTreeMap<String, Uint256>,
    coin: &Coin,
) -> StdResult<()> {
    match requirements.entry(coin.denom.clone()) {
        Entry::Occupied(mut entry) => {
            let entry_val = *entry.get();
            let sum = entry_val
                .checked_add(coin.amount)
                .map_err(|_| StdError::msg("repayment amount overflow"))?;
            entry.insert(sum);
        }
        Entry::Vacant(entry) => {
            entry.insert(coin.amount);
        }
    }
    Ok(())
}

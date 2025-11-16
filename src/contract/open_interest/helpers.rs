use cosmwasm_std::{
    attr, Addr, Attribute, BankMsg, Coin, CosmosMsg, Deps, DepsMut, DistributionMsg, Env,
    MessageInfo, Order, StakingMsg, StdError, StdResult, Storage, Timestamp, Uint128, Uint256,
};
use std::collections::{btree_map::Entry, BTreeMap};
use std::convert::TryFrom;

use crate::{
    helpers::{
        minimum_collateral_lock_for_denom, query_staking_rewards_for_denom, require_owner_or_lender,
    },
    state::{COUNTER_OFFERS, LENDER, OPEN_INTEREST, OPEN_INTEREST_EXPIRY, OUTSTANDING_DEBT},
    types::OpenInterest,
    ContractError,
};

pub(crate) fn validate_open_interest(
    deps: &Deps,
    env: &Env,
    open_interest: &OpenInterest,
) -> Result<(), ContractError> {
    validate_coin(&open_interest.liquidity_coin, "liquidity_coin")?;
    validate_coin(&open_interest.interest_coin, "interest_coin")?;
    validate_coin(&open_interest.collateral, "collateral")?;

    if open_interest.expiry_duration == 0 {
        return Err(ContractError::InvalidExpiryDuration {});
    }

    build_repayment_amounts(open_interest)?;
    ensure_collateral_available(deps, env, open_interest)?;

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

fn ensure_collateral_available(
    deps: &Deps,
    env: &Env,
    open_interest: &OpenInterest,
) -> Result<(), ContractError> {
    let denom = open_interest.collateral.denom.clone();
    let requested = open_interest.collateral.amount;

    let available = query_available_balance(deps, env, &denom)?;
    if available >= requested {
        return Ok(());
    }

    let required_lock = minimum_collateral_lock_for_denom(deps, env, &denom, Some(open_interest))?;
    if available >= required_lock {
        return Ok(());
    }

    let staking_coverage = requested.saturating_sub(required_lock);
    let effective_balance = available
        .checked_add(staking_coverage)
        .map_err(StdError::from)?;

    if effective_balance >= requested {
        return Ok(());
    }

    Err(ContractError::InsufficientBalance {
        denom,
        available: effective_balance,
        requested,
    })
}

fn query_available_balance(deps: &Deps, env: &Env, denom: &str) -> StdResult<Uint256> {
    let balance = deps
        .querier
        .query_balance(env.contract.address.clone(), denom.to_string())?;
    Ok(balance.amount)
}

pub(crate) fn open_interest_attributes(
    action: &'static str,
    open_interest: &OpenInterest,
) -> Vec<Attribute> {
    vec![
        attr("action", action),
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
    ]
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

pub(crate) struct LiquidationState {
    pub(crate) open_interest: OpenInterest,
    pub(crate) lender: Addr,
    pub(crate) denom: String,
    pub(crate) contract_addr: Addr,
    pub(crate) bonded_denom: String,
}

pub fn set_active_lender(
    storage: &mut dyn Storage,
    lender: Addr,
    expiry: Timestamp,
) -> StdResult<()> {
    LENDER.save(storage, &Some(lender))?;
    OPEN_INTEREST_EXPIRY.save(storage, &Some(expiry))?;
    Ok(())
}

pub fn clear_active_lender(storage: &mut dyn Storage) -> StdResult<()> {
    LENDER.save(storage, &None)?;
    OPEN_INTEREST_EXPIRY.save(storage, &None)?;
    Ok(())
}

pub(crate) struct CollectedFunds {
    pub(crate) available: Uint256,
    pub(crate) rewards_claimed: Uint256,
    pub(crate) reward_claim_messages: Vec<CosmosMsg>,
}

pub(crate) fn load_liquidation_state(
    deps: &DepsMut,
    env: &Env,
    info: &MessageInfo,
) -> Result<LiquidationState, ContractError> {
    require_owner_or_lender(deps, info)?;

    let open_interest = OPEN_INTEREST
        .may_load(deps.storage)?
        .flatten()
        .ok_or(ContractError::NoOpenInterest {})?;

    let lender = LENDER
        .load(deps.storage)?
        .ok_or(ContractError::NoLender {})?;

    let expiry = OPEN_INTEREST_EXPIRY
        .load(deps.storage)?
        .expect("open interest expiry missing despite lender being set");

    if env.block.time < expiry {
        return Err(ContractError::OpenInterestNotExpired {});
    }

    let denom = open_interest.collateral.denom.clone();
    let contract_addr = env.contract.address.clone();
    let bonded_denom = deps.querier.query_bonded_denom()?;

    Ok(LiquidationState {
        open_interest,
        lender,
        denom,
        contract_addr,
        bonded_denom,
    })
}

pub(crate) fn get_outstanding_amount(
    state: &LiquidationState,
    deps: &DepsMut,
) -> Result<Uint256, ContractError> {
    let outstanding_debt = OUTSTANDING_DEBT.may_load(deps.storage)?.flatten();
    match outstanding_debt {
        Some(debt) => {
            if debt.denom != state.denom {
                return Err(ContractError::Std(StdError::msg(format!(
                    "Outstanding debt denom mismatch: expected {}, got {}",
                    state.denom, debt.denom
                ))));
            }
            let debt_amount = debt.amount;
            Ok(debt_amount)
        }
        None => {
            let collateral_amount = state.open_interest.collateral.amount;
            Ok(collateral_amount)
        }
    }
}

pub(crate) fn collect_funds(
    state: &LiquidationState,
    deps: &Deps,
    env: &Env,
    remaining: Uint256,
) -> Result<CollectedFunds, ContractError> {
    let balance = deps
        .querier
        .query_balance(state.contract_addr.clone(), state.denom.clone())?
        .amount;
    let mut total_available = balance;
    let mut reward_claim_messages = Vec::new();
    let mut rewards_claimed = Uint256::zero();

    if state.denom == state.bonded_denom && total_available < remaining {
        let delegations = deps
            .querier
            .query_all_delegations(state.contract_addr.clone())?;

        let reward_amount = query_staking_rewards_for_denom(deps, env, &state.denom)?;
        if !reward_amount.is_zero() {
            for delegation in delegations {
                reward_claim_messages.push(CosmosMsg::Distribution(
                    DistributionMsg::WithdrawDelegatorReward {
                        validator: delegation.validator.clone(),
                    },
                ));
            }
            rewards_claimed = reward_amount;
        }

        total_available = total_available.checked_add(reward_amount).map_err(|_| {
            ContractError::Std(StdError::msg("liquidation total available overflow"))
        })?;
    }

    Ok(CollectedFunds {
        available: total_available,
        rewards_claimed,
        reward_claim_messages,
    })
}

pub(crate) fn payout_message(
    state: &LiquidationState,
    payout_amount: Uint256,
) -> Result<CosmosMsg, ContractError> {
    let payout_value =
        Uint128::try_from(payout_amount).map_err(|_| ContractError::LiquidationAmountOverflow {
            denom: state.denom.clone(),
            requested: payout_amount,
        })?;

    Ok(CosmosMsg::Bank(BankMsg::Send {
        to_address: state.lender.to_string(),
        amount: vec![Coin::new(payout_value.u128(), state.denom.clone())],
    }))
}

pub(crate) fn schedule_undelegations(
    state: &LiquidationState,
    deps: &Deps,
    remaining: Uint256,
) -> Result<(Vec<CosmosMsg>, Uint256), ContractError> {
    if remaining.is_zero() {
        return Ok((Vec::new(), Uint256::zero()));
    }

    let delegations = deps
        .querier
        .query_all_delegations(state.contract_addr.clone())?;

    let mut messages = Vec::new();
    let mut remaining_to_undelegate = remaining;
    let mut undelegated = Uint256::zero();

    for delegation in delegations {
        if remaining_to_undelegate.is_zero() {
            break;
        }

        let stake_amount = delegation.amount.amount;
        if stake_amount.is_zero() {
            continue;
        }

        let amount = stake_amount.min(remaining_to_undelegate);

        let coin_amount =
            Uint128::try_from(amount).map_err(|_| ContractError::UndelegationAmountOverflow {
                denom: state.denom.clone(),
                requested: amount,
            })?;

        messages.push(CosmosMsg::Staking(StakingMsg::Undelegate {
            validator: delegation.validator.clone(),
            amount: Coin::new(coin_amount.u128(), state.denom.clone()),
        }));

        remaining_to_undelegate = remaining_to_undelegate
            .checked_sub(amount)
            .map_err(|_| ContractError::Std(StdError::msg("liquidation undelegate overflow")))?;
        undelegated = undelegated.checked_add(amount).map_err(|_| {
            ContractError::Std(StdError::msg("liquidation undelegated amount overflow"))
        })?;
    }

    Ok((messages, undelegated))
}

pub(crate) fn finalize_state(
    state: &LiquidationState,
    deps: &mut DepsMut,
    remaining: Uint256,
) -> Result<(), ContractError> {
    if remaining.is_zero() {
        OUTSTANDING_DEBT.save(deps.storage, &None)?;
        OPEN_INTEREST.save(deps.storage, &None)?;
        clear_active_lender(deps.storage)?;
        return Ok(());
    }

    let outstanding_coin = Coin::new(
        Uint128::try_from(remaining).map_err(|_| ContractError::RepaymentAmountOverflow {
            denom: state.denom.clone(),
            requested: remaining,
        })?,
        state.denom.clone(),
    );
    OUTSTANDING_DEBT.save(deps.storage, &Some(outstanding_coin))?;
    Ok(())
}

pub(crate) fn push_nonzero_attr(attrs: &mut Vec<Attribute>, key: &'static str, value: Uint256) {
    if value.is_zero() {
        return;
    }

    attrs.push(attr(key, value.to_string()));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::open_interest::test_helpers::{build_open_interest, sample_coin};
    use crate::ContractError;
    use cosmwasm_std::{
        coins,
        testing::{mock_dependencies, mock_env},
        Addr, Coin, DecCoin, Decimal, Decimal256, FullDelegation, Uint256, Validator,
    };

    fn test_open_interest(collateral: Coin) -> OpenInterest {
        build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            collateral,
        )
    }

    fn stub_validator() -> Validator {
        Validator::create(
            "validator".to_string(),
            Decimal::percent(5),
            Decimal::percent(10),
            Decimal::percent(1),
        )
    }

    fn reward_coin(amount: u128, denom: &str) -> DecCoin {
        DecCoin::new(
            Decimal256::from_atomics(Uint256::from(amount), 0).unwrap(),
            denom,
        )
    }

    fn staking_delegation(addr: Addr, amount: u128) -> FullDelegation {
        FullDelegation::create(
            addr,
            "validator".to_string(),
            Coin::new(amount, "ucosm"),
            Coin::new(amount, "ucosm"),
            vec![],
        )
    }

    #[test]
    fn rejects_non_staking_collateral_if_balance_missing() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        deps.querier
            .bank
            .update_balance(env.contract.address.as_str(), coins(150, "uatom"));

        let open_interest = test_open_interest(sample_coin(200, "uatom"));

        let err = validate_open_interest(&deps.as_ref(), &env, &open_interest).unwrap_err();

        assert!(matches!(
            err,
            ContractError::InsufficientBalance {
                denom,
                available,
                requested,
            } if denom == "uatom"
                && available == Uint256::from(150u128)
                && requested == Uint256::from(200u128)
        ));
    }

    #[test]
    fn accepts_staking_collateral_with_rewards_and_staked_balance() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        deps.querier
            .bank
            .update_balance(env.contract.address.as_str(), coins(50, "ucosm"));
        deps.querier.distribution.set_rewards(
            "validator",
            env.contract.address.as_str(),
            vec![reward_coin(80, "ucosm")],
        );
        let validator = stub_validator();
        let delegation = staking_delegation(env.contract.address.clone(), 100);
        deps.querier
            .staking
            .update("ucosm", &[validator], &[delegation]);

        let open_interest = test_open_interest(sample_coin(200, "ucosm"));
        validate_open_interest(&deps.as_ref(), &env, &open_interest)
            .expect("collateral should cover");
    }

    #[test]
    fn rejects_staking_collateral_when_combined_balance_is_insufficient() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        deps.querier
            .bank
            .update_balance(env.contract.address.as_str(), coins(50, "ucosm"));
        deps.querier.distribution.set_rewards(
            "validator",
            env.contract.address.as_str(),
            vec![reward_coin(20, "ucosm")],
        );
        let validator = stub_validator();
        let delegation = staking_delegation(env.contract.address.clone(), 100);
        deps.querier
            .staking
            .update("ucosm", &[validator], &[delegation]);

        let open_interest = test_open_interest(sample_coin(200, "ucosm"));

        let err = validate_open_interest(&deps.as_ref(), &env, &open_interest).unwrap_err();

        assert!(matches!(
            err,
            ContractError::InsufficientBalance {
                denom,
                available,
                requested,
            } if denom == "ucosm"
                && available == Uint256::from(170u128)
                && requested == Uint256::from(200u128)
        ));
    }
}

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Deps, DepsMut, Env, MessageInfo, StdError, StdResult, Uint256};

use crate::{
    error::ContractError,
    state::{LENDER, OWNER},
    types::OpenInterest,
};

/// CwTemplateContract is a wrapper around Addr that provides a lot of helpers
/// for working with this.
#[cw_serde]
pub struct CwTemplateContract(pub Addr);

impl CwTemplateContract {
    pub fn addr(&self) -> Addr {
        self.0.clone()
    }
}

pub fn require_owner(deps: &DepsMut, info: &MessageInfo) -> Result<Addr, ContractError> {
    let owner = OWNER.load(deps.storage)?;
    if info.sender != owner {
        Err(ContractError::Unauthorized {})
    } else {
        Ok(owner)
    }
}

pub fn require_owner_or_lender(deps: &DepsMut, info: &MessageInfo) -> Result<Addr, ContractError> {
    let owner = OWNER.load(deps.storage)?;
    if info.sender == owner {
        return Ok(owner);
    }

    let lender = LENDER.may_load(deps.storage)?.flatten();
    if let Some(lender_addr) = lender {
        if info.sender == lender_addr {
            return Ok(lender_addr);
        }
    }

    Err(ContractError::Unauthorized {})
}

pub fn query_staking_rewards(deps: &Deps, env: &Env) -> StdResult<Uint256> {
    // Rewards always payout in the bonded denom, so we sum every reward coin here.
    let response = deps
        .querier
        .query_delegation_total_rewards(env.contract.address.clone())?;

    response
        .total
        .into_iter()
        .try_fold(Uint256::zero(), |acc, coin| {
            acc.checked_add(coin.amount.to_uint_floor())
                .map_err(StdError::from)
        })
}

pub fn query_staked_balance(deps: &Deps, env: &Env, denom: &str) -> StdResult<Uint256> {
    let delegations = deps
        .querier
        .query_all_delegations(env.contract.address.clone())?;

    delegations
        .into_iter()
        .filter(|delegation| delegation.amount.denom == denom)
        .try_fold(Uint256::zero(), |acc, delegation| {
            acc.checked_add(delegation.amount.amount)
                .map_err(StdError::from)
        })
}

/// Returns the minimum amount of collateral that must remain locked for `denom`.
pub fn minimum_collateral_lock_for_denom(
    deps: &Deps,
    env: &Env,
    denom: &str,
    open_interest: Option<&OpenInterest>,
) -> StdResult<Uint256> {
    let Some(interest) = open_interest else {
        return Ok(Uint256::zero());
    };

    if interest.collateral.denom != denom {
        return Ok(Uint256::zero());
    };

    let bonded_denom = deps.querier.query_bonded_denom()?;
    if denom != bonded_denom {
        return Ok(interest.collateral.amount);
    };

    let rewards = query_staking_rewards(deps, env)?;
    let staked = query_staked_balance(deps, env, denom)?;
    let coverage = rewards.checked_add(staked).map_err(StdError::from)?;

    Ok(interest.collateral.amount.saturating_sub(coverage))
}

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Deps, DepsMut, Env, MessageInfo, StdError, StdResult, Uint256};

use crate::{error::ContractError, state::OWNER};

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

pub fn query_staking_rewards_for_denom(deps: &Deps, env: &Env, denom: &str) -> StdResult<Uint256> {
    let response = deps
        .querier
        .query_delegation_total_rewards(env.contract.address.clone())?;

    response
        .total
        .into_iter()
        .filter(|coin| coin.denom == denom)
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

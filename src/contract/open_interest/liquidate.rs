use cosmwasm_std::{
    attr, BankMsg, Coin, CosmosMsg, Delegation, DepsMut, DistributionMsg, Env, MessageInfo,
    Response, StakingMsg, StdError, Uint128, Uint256,
};

use crate::{
    helpers::{query_staking_rewards_for_denom, require_owner_or_lender},
    state::{LENDER, OPEN_INTEREST, OPEN_INTEREST_EXPIRY, OUTSTANDING_DEBT},
    ContractError,
};

use super::helpers::open_interest_attributes;
use crate::types::OpenInterest;

struct LiquidationContext {
    open_interest: OpenInterest,
    lender: cosmwasm_std::Addr,
    denom: String,
    contract_addr: cosmwasm_std::Addr,
    bonded_denom: String,
}

impl LiquidationContext {
    fn load(deps: &DepsMut, env: &Env, info: &MessageInfo) -> Result<Self, ContractError> {
        require_owner_or_lender(deps, info)?;

        let open_interest = OPEN_INTEREST
            .may_load(deps.storage)?
            .flatten()
            .ok_or(ContractError::NoOpenInterest {})?;

        let lender = LENDER
            .load(deps.storage)?
            .ok_or(ContractError::NoLender {})?;

        let expiry = OPEN_INTEREST_EXPIRY.load(deps.storage)?.ok_or_else(|| {
            ContractError::Std(StdError::msg(
                "open interest expiry missing despite lender being set",
            ))
        })?;

        if env.block.time < expiry {
            return Err(ContractError::OpenInterestNotExpired {});
        }

        let denom = open_interest.collateral.denom.clone();
        let contract_addr = env.contract.address.clone();
        let bonded_denom = deps.querier.query_bonded_denom()?;

        Ok(LiquidationContext {
            open_interest,
            lender,
            denom,
            contract_addr,
            bonded_denom,
        })
    }

    fn remaining_outstanding(&self, deps: &DepsMut) -> Result<Uint256, ContractError> {
        let outstanding_debt = OUTSTANDING_DEBT.may_load(deps.storage)?.flatten();
        match outstanding_debt {
            Some(debt) => {
                if debt.denom != self.denom {
                    return Err(ContractError::Std(StdError::msg(
                        "Outstanding debt denom mismatch",
                    )));
                }
                Ok(debt.amount)
            }
            None => Ok(self.open_interest.collateral.amount),
        }
    }

    fn is_bonded(&self) -> bool {
        self.denom == self.bonded_denom
    }

    fn collect_funds(
        &self,
        deps: &DepsMut,
        env: &Env,
        remaining: Uint256,
    ) -> Result<(Uint256, Uint256, Vec<CosmosMsg>, Vec<Delegation>), ContractError> {
        let mut messages = Vec::new();
        let mut total_available = deps
            .querier
            .query_balance(self.contract_addr.clone(), self.denom.clone())?
            .amount;
        let mut delegations = Vec::new();
        let mut rewards_claimed = Uint256::zero();

        if self.is_bonded() && total_available < remaining {
            delegations = deps
                .querier
                .query_all_delegations(self.contract_addr.clone())?;

            let reward_amount = query_staking_rewards_for_denom(&deps.as_ref(), env, &self.denom)?;
            if !reward_amount.is_zero() {
                for delegation in &delegations {
                    messages.push(CosmosMsg::Distribution(
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

        Ok((total_available, rewards_claimed, messages, delegations))
    }

    fn schedule_undelegations(
        &self,
        deps: &DepsMut,
        remaining: Uint256,
        mut delegations: Vec<Delegation>,
    ) -> Result<(Vec<CosmosMsg>, Uint256), ContractError> {
        if remaining.is_zero() {
            return Ok((Vec::new(), Uint256::zero()));
        }

        if delegations.is_empty() {
            delegations = deps
                .querier
                .query_all_delegations(self.contract_addr.clone())?;
        }

        let mut messages = Vec::new();
        let mut left = remaining;
        let mut undelegated = Uint256::zero();

        for delegation in delegations {
            if left.is_zero() {
                break;
            }

            let stake_amount = delegation.amount.amount;
            if stake_amount.is_zero() {
                continue;
            }

            let amount = if stake_amount < left {
                stake_amount
            } else {
                left
            };

            let coin_amount =
                Uint128::try_from(amount).map_err(|_| ContractError::RepaymentAmountOverflow {
                    denom: self.denom.clone(),
                    requested: amount,
                })?;

            messages.push(CosmosMsg::Staking(StakingMsg::Undelegate {
                validator: delegation.validator.clone(),
                amount: Coin::new(coin_amount, self.denom.clone()),
            }));

            left = left.checked_sub(amount).map_err(|_| {
                ContractError::Std(StdError::msg("liquidation undelegate overflow"))
            })?;
            undelegated = undelegated.checked_add(amount).map_err(|_| {
                ContractError::Std(StdError::msg("liquidation undelegated amount overflow"))
            })?;
        }

        Ok((messages, undelegated))
    }

    fn finalize_state(&self, deps: &mut DepsMut, remaining: Uint256) -> Result<(), ContractError> {
        if remaining.is_zero() {
            OUTSTANDING_DEBT.save(deps.storage, &None)?;
            OPEN_INTEREST.save(deps.storage, &None)?;
            LENDER.save(deps.storage, &None)?;
            OPEN_INTEREST_EXPIRY.save(deps.storage, &None)?;
            return Ok(());
        }

        let outstanding_coin = Coin::new(
            Uint128::try_from(remaining).map_err(|_| ContractError::RepaymentAmountOverflow {
                denom: self.denom.clone(),
                requested: remaining,
            })?,
            self.denom.clone(),
        );
        OUTSTANDING_DEBT.save(deps.storage, &Some(outstanding_coin))?;
        Ok(())
    }
}

pub fn liquidate(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let ctx = LiquidationContext::load(&deps, &env, &info)?;
    let remaining = ctx.remaining_outstanding(&deps)?;
    let mut messages = Vec::new();

    let (total_available, rewards_claimed, mut fund_messages, delegations) =
        ctx.collect_funds(&deps, &env, remaining)?;
    messages.append(&mut fund_messages);

    let payout_amount = if total_available < remaining {
        total_available
    } else {
        remaining
    };

    if payout_amount > Uint256::zero() {
        let payout_value = Uint128::try_from(payout_amount).map_err(|_| {
            ContractError::RepaymentAmountOverflow {
                denom: ctx.denom.clone(),
                requested: payout_amount,
            }
        })?;
        messages.push(CosmosMsg::Bank(BankMsg::Send {
            to_address: ctx.lender.to_string(),
            amount: vec![Coin::new(payout_value, ctx.denom.clone())],
        }));
    }

    let remaining_after_payout = remaining
        .checked_sub(payout_amount)
        .map_err(|_| ContractError::Std(StdError::msg("liquidation remaining overflow")))?;

    if !remaining_after_payout.is_zero() && !ctx.is_bonded() {
        return Err(ContractError::InsufficientBalance {
            denom: ctx.denom.clone(),
            available: total_available,
            requested: remaining,
        });
    }

    let (undelegate_msgs, undelegated_amount) =
        ctx.schedule_undelegations(&deps, remaining_after_payout, delegations)?;
    messages.extend(undelegate_msgs);

    ctx.finalize_state(&mut deps, remaining_after_payout)?;

    let mut attrs = open_interest_attributes("liquidate_open_interest", &ctx.open_interest);
    attrs.push(attr("lender", ctx.lender.as_str()));

    if payout_amount > Uint256::zero() {
        attrs.push(attr("payout_amount", payout_amount.to_string()));
    }

    if !rewards_claimed.is_zero() {
        attrs.push(attr("rewards_claimed", rewards_claimed.to_string()));
    }

    if !undelegated_amount.is_zero() {
        attrs.push(attr("undelegated_amount", undelegated_amount.to_string()));
    }

    if !remaining_after_payout.is_zero() {
        attrs.push(attr("outstanding_debt", remaining_after_payout.to_string()));
    }

    let mut response = Response::new().add_attributes(attrs);
    for msg in messages {
        response = response.add_message(msg);
    }

    Ok(response)
}

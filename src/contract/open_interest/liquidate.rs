use cosmwasm_std::{
    attr, Attribute, BankMsg, Coin, CosmosMsg, Delegation, Deps, DepsMut, DistributionMsg, Env,
    MessageInfo, Response, StakingMsg, StdError, Uint128, Uint256,
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

struct CollectedFunds {
    available: Uint256,
    rewards_claimed: Uint256,
    reward_claim_messages: Vec<CosmosMsg>,
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

    fn get_outstanding_amount(&self, deps: &DepsMut) -> Result<Uint256, ContractError> {
        let outstanding_debt = OUTSTANDING_DEBT.may_load(deps.storage)?.flatten();
        match outstanding_debt {
            Some(debt) => {
                if debt.denom != self.denom {
                    return Err(ContractError::Std(StdError::msg(format!(
                        "Outstanding debt denom mismatch: expected {}, got {}",
                        self.denom, debt.denom
                    ))));
                }
                #[allow(clippy::useless_conversion)]
                let debt_amount = Uint256::from(debt.amount);
                Ok(debt_amount)
            }
            None => {
                #[allow(clippy::useless_conversion)]
                let collateral_amount = Uint256::from(self.open_interest.collateral.amount);
                Ok(collateral_amount)
            }
        }
    }

    fn is_bonded(&self) -> bool {
        self.denom == self.bonded_denom
    }

    /// Gather the available balance, rewards, and any reward-claim messages needed for liquidation.
    fn collect_funds(
        &self,
        deps: &Deps,
        env: &Env,
        remaining: Uint256,
    ) -> Result<CollectedFunds, ContractError> {
        let balance = deps
            .querier
            .query_balance(self.contract_addr.clone(), self.denom.clone())?
            .amount;
        #[allow(clippy::useless_conversion)]
        let mut total_available: Uint256 = Uint256::from(balance);
        let mut reward_claim_messages = Vec::new();
        let mut rewards_claimed = Uint256::zero();

        if self.is_bonded() && total_available < remaining {
            let delegations = deps
                .querier
                .query_all_delegations(self.contract_addr.clone())?;

            let reward_amount = query_staking_rewards_for_denom(deps, env, &self.denom)?;
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

    fn payout_message(&self, payout_amount: Uint256) -> Result<CosmosMsg, ContractError> {
        let payout_value = Uint128::try_from(payout_amount).map_err(|_| {
            ContractError::LiquidationAmountOverflow {
                denom: self.denom.clone(),
                requested: payout_amount,
            }
        })?;

        Ok(CosmosMsg::Bank(BankMsg::Send {
            to_address: self.lender.to_string(),
            amount: vec![Coin::new(payout_value.u128(), self.denom.clone())],
        }))
    }

    /// Request undelegations until the remaining amount is fulfilled (or delegations are exhausted).
    fn schedule_undelegations(
        &self,
        deps: &Deps,
        remaining: Uint256,
    ) -> Result<(Vec<CosmosMsg>, Uint256), ContractError> {
        if remaining.is_zero() {
            return Ok((Vec::new(), Uint256::zero()));
        }

        let delegations = deps
            .querier
            .query_all_delegations(self.contract_addr.clone())?;

        let mut messages = Vec::new();
        let mut remaining_to_undelegate = remaining;
        let mut undelegated = Uint256::zero();

        for delegation in delegations {
            if remaining_to_undelegate.is_zero() {
                break;
            }

            #[allow(clippy::useless_conversion)]
            let stake_amount = Uint256::from(delegation.amount.amount);
            if stake_amount.is_zero() {
                continue;
            }

            let amount = stake_amount.min(remaining_to_undelegate);

            let coin_amount = Uint128::try_from(amount).map_err(|_| {
                ContractError::UndelegationAmountOverflow {
                    denom: self.denom.clone(),
                    requested: amount,
                }
            })?;

            messages.push(CosmosMsg::Staking(StakingMsg::Undelegate {
                validator: delegation.validator.clone(),
                amount: Coin::new(coin_amount.u128(), self.denom.clone()),
            }));

            remaining_to_undelegate =
                remaining_to_undelegate.checked_sub(amount).map_err(|_| {
                    ContractError::Std(StdError::msg("liquidation undelegate overflow"))
                })?;
            undelegated = undelegated.checked_add(amount).map_err(|_| {
                ContractError::Std(StdError::msg("liquidation undelegated amount overflow"))
            })?;
        }

        Ok((messages, undelegated))
    }

    /// Update open interest storage depending on whether any debt remains after liquidation.
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

fn push_nonzero_attr(attrs: &mut Vec<Attribute>, key: &'static str, value: Uint256) {
    if value.is_zero() {
        return;
    }

    attrs.push(attr(key, value.to_string()));
}

pub fn liquidate(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let ctx = LiquidationContext::load(&deps, &env, &info)?;
    let remaining = ctx.get_outstanding_amount(&deps)?;

    let mut messages = Vec::new();
    let collected = ctx.collect_funds(&deps.as_ref(), &env, remaining)?;
    let CollectedFunds {
        available,
        rewards_claimed,
        reward_claim_messages,
    } = collected;
    messages.extend(reward_claim_messages);

    let payout_amount = available.min(remaining);
    if !payout_amount.is_zero() {
        messages.push(ctx.payout_message(payout_amount)?);
    }

    let remaining_after_payout = remaining
        .checked_sub(payout_amount)
        .map_err(|_| ContractError::Std(StdError::msg("liquidation remaining overflow")))?;

    if !remaining_after_payout.is_zero() && !ctx.is_bonded() {
        return Err(ContractError::InsufficientBalance {
            denom: ctx.denom.clone(),
            available,
            requested: remaining,
        });
    }

    let (undelegate_msgs, undelegated_amount) =
        ctx.schedule_undelegations(&deps.as_ref(), remaining_after_payout)?;
    messages.extend(undelegate_msgs);

    let settled_remaining = remaining_after_payout
        .checked_sub(undelegated_amount)
        .map_err(|_| ContractError::Std(StdError::msg("settled remaining underflow")))?;
    ctx.finalize_state(&mut deps, settled_remaining)?;

    let mut attrs = open_interest_attributes("liquidate_open_interest", &ctx.open_interest);
    attrs.push(attr("lender", ctx.lender.as_str()));
    push_nonzero_attr(&mut attrs, "payout_amount", payout_amount);
    push_nonzero_attr(&mut attrs, "rewards_claimed", rewards_claimed);
    push_nonzero_attr(&mut attrs, "undelegated_amount", undelegated_amount);
    push_nonzero_attr(&mut attrs, "outstanding_debt", settled_remaining);

    let mut response = Response::new().add_attributes(attrs);
    for msg in messages {
        response = response.add_message(msg);
    }

    Ok(response)
}

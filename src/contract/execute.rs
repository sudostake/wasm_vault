#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{DepsMut, Env, MessageInfo, Response};

use super::{
    claim, counter_offer, delegate, open_interest, redelegate, transfer, undelegate, vote, withdraw,
};
use crate::error::ContractError;
use crate::msg::ExecuteMsg;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::Noop {} => Ok(Response::new()),
        ExecuteMsg::Delegate { validator, amount } => {
            delegate::execute(deps, env, info, validator, amount)
        }
        ExecuteMsg::Undelegate { validator, amount } => {
            undelegate::execute(deps, env, info, validator, amount)
        }
        ExecuteMsg::Redelegate {
            src_validator,
            dst_validator,
            amount,
        } => redelegate::execute(deps, env, info, src_validator, dst_validator, amount),
        ExecuteMsg::ClaimDelegatorRewards {} => claim::execute(deps, env, info),
        ExecuteMsg::Withdraw {
            denom,
            amount,
            recipient,
        } => withdraw::execute(deps, env, info, denom, amount, recipient),
        ExecuteMsg::Vote {
            proposal_id,
            option,
        } => vote::execute_vote(deps, env, info, proposal_id, option),
        ExecuteMsg::VoteWeighted {
            proposal_id,
            options,
        } => vote::execute_weighted_vote(deps, env, info, proposal_id, options),
        ExecuteMsg::TransferOwnership { new_owner } => transfer::execute(deps, info, new_owner),
        ExecuteMsg::OpenInterest(open_interest_msg) => {
            open_interest::execute(deps, env, info, open_interest_msg)
        }
        ExecuteMsg::FundOpenInterest(expected_interest) => {
            open_interest::fund(deps, env, info, expected_interest)
        }
        ExecuteMsg::ProposeCounterOffer(open_interest) => {
            counter_offer::propose(deps, env, info, open_interest)
        }
        ExecuteMsg::AcceptCounterOffer {
            proposer,
            open_interest,
        } => counter_offer::accept(deps, env, info, proposer, open_interest),
        ExecuteMsg::CancelCounterOffer {} => counter_offer::cancel(deps, env, info),
        ExecuteMsg::CloseOpenInterest {} => open_interest::close(deps, info),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        state::{COUNTER_OFFERS, LENDER, OPEN_INTEREST, OUTSTANDING_DEBT, OWNER},
        types::OpenInterest,
    };
    use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env};
    use cosmwasm_std::{coins, Uint128};

    #[test]
    fn execute_returns_empty_response() {
        let mut deps = mock_dependencies();
        let caller = deps.api.addr_make("caller");
        let info = message_info(&caller, &[]);

        let response = execute(deps.as_mut(), mock_env(), info, ExecuteMsg::Noop {})
            .expect("execute succeeds");

        assert!(response.messages.is_empty());
        assert!(response.attributes.is_empty());
    }

    #[test]
    fn execute_delegate_flows_through_module() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        OWNER
            .save(deps.as_mut().storage, &owner)
            .expect("owner stored");
        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &None)
            .expect("zero debt stored");
        OPEN_INTEREST
            .save(deps.as_mut().storage, &None)
            .expect("no open interest stored");

        deps.querier.staking.update("ucosm", &[], &[]);
        deps.querier
            .bank
            .update_balance(mock_env().contract.address.as_str(), coins(100, "ucosm"));

        let validator = deps.api.addr_make("validator").into_string();
        let env = mock_env();

        let err = execute(
            deps.as_mut(),
            env,
            message_info(&owner, &[]),
            ExecuteMsg::Delegate {
                validator,
                amount: Uint128::new(50),
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::ValidatorNotFound { .. }));
    }

    #[test]
    fn execute_undelegate_flows_through_module() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        OWNER
            .save(deps.as_mut().storage, &owner)
            .expect("owner stored");
        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &None)
            .expect("zero debt stored");
        OPEN_INTEREST
            .save(deps.as_mut().storage, &None)
            .expect("no open interest stored");

        let validator = deps.api.addr_make("validator").into_string();
        let env = mock_env();

        let err = execute(
            deps.as_mut(),
            env,
            message_info(&owner, &[]),
            ExecuteMsg::Undelegate {
                validator: validator.clone(),
                amount: Uint128::new(50),
            },
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::DelegationNotFound { validator: v } if v == validator
        ));
    }

    #[test]
    fn execute_redelegate_flows_through_module() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        OWNER
            .save(deps.as_mut().storage, &owner)
            .expect("owner stored");
        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &None)
            .expect("zero debt stored");
        OPEN_INTEREST
            .save(deps.as_mut().storage, &None)
            .expect("no open interest stored");

        let src_validator = deps.api.addr_make("validator").into_string();
        let dst_validator = deps.api.addr_make("validator-two").into_string();
        let env = mock_env();

        let err = execute(
            deps.as_mut(),
            env,
            message_info(&owner, &[]),
            ExecuteMsg::Redelegate {
                src_validator: src_validator.clone(),
                dst_validator,
                amount: Uint128::new(50),
            },
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::DelegationNotFound { validator: v } if v == src_validator
        ));
    }

    #[test]
    fn execute_withdraw_flows_through_module() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        OWNER
            .save(deps.as_mut().storage, &owner)
            .expect("owner stored");
        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &None)
            .expect("zero debt stored");
        OPEN_INTEREST
            .save(deps.as_mut().storage, &None)
            .expect("no open interest stored");

        let env = mock_env();
        let err = execute(
            deps.as_mut(),
            env,
            message_info(&owner, &[]),
            ExecuteMsg::Withdraw {
                denom: "ucosm".to_string(),
                amount: Uint128::new(50),
                recipient: None,
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::InsufficientBalance { .. }));
    }

    #[test]
    fn execute_claim_rewards_flows_through_module() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        OWNER
            .save(deps.as_mut().storage, &owner)
            .expect("owner stored");

        let env = mock_env();
        let err = execute(
            deps.as_mut(),
            env,
            message_info(&owner, &[]),
            ExecuteMsg::ClaimDelegatorRewards {},
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::NoDelegations {}));
    }

    #[test]
    fn execute_transfer_ownership_flows_through_module() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        OWNER
            .save(deps.as_mut().storage, &owner)
            .expect("owner stored");

        let err = execute(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            ExecuteMsg::TransferOwnership {
                new_owner: owner.to_string(),
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::OwnershipUnchanged {}));
    }

    #[test]
    fn execute_open_interest_flows_through_module() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        OWNER
            .save(deps.as_mut().storage, &owner)
            .expect("owner stored");
        OPEN_INTEREST
            .save(deps.as_mut().storage, &None)
            .expect("open interest defaults to none");

        let err = execute(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            ExecuteMsg::OpenInterest(OpenInterest {
                liquidity_coin: cosmwasm_std::Coin::new(0u128, "uusd"),
                interest_coin: cosmwasm_std::Coin::new(5u128, "ujuno"),
                expiry_duration: 86_400,
                collateral: cosmwasm_std::Coin::new(200u128, "uatom"),
            }),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::InvalidCoinAmount {
                field: "liquidity_coin"
            }
        ));
    }

    #[test]
    fn execute_close_open_interest_flows_through_module() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        OWNER
            .save(deps.as_mut().storage, &owner)
            .expect("owner stored");
        LENDER
            .save(deps.as_mut().storage, &None)
            .expect("lender defaults to none");
        let open_interest = OpenInterest {
            liquidity_coin: cosmwasm_std::Coin::new(1u128, "uusd"),
            interest_coin: cosmwasm_std::Coin::new(1u128, "ujuno"),
            expiry_duration: 100,
            collateral: cosmwasm_std::Coin::new(2u128, "uatom"),
        };
        OPEN_INTEREST
            .save(deps.as_mut().storage, &Some(open_interest))
            .expect("open interest stored");

        execute(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            ExecuteMsg::CloseOpenInterest {},
        )
        .expect("close succeeds");

        let stored = OPEN_INTEREST
            .load(deps.as_ref().storage)
            .expect("state loaded");
        assert!(stored.is_none());
    }

    #[test]
    fn execute_counter_offer_flows_through_module() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        OWNER
            .save(deps.as_mut().storage, &owner)
            .expect("owner stored");
        LENDER
            .save(deps.as_mut().storage, &None)
            .expect("lender cleared");

        let base_interest = OpenInterest {
            liquidity_coin: cosmwasm_std::Coin::new(1_000u128, "uusd"),
            interest_coin: cosmwasm_std::Coin::new(50u128, "ujuno"),
            expiry_duration: 86_400,
            collateral: cosmwasm_std::Coin::new(2_000u128, "uatom"),
        };

        OPEN_INTEREST
            .save(deps.as_mut().storage, &Some(base_interest.clone()))
            .expect("open interest stored");

        let proposer = deps.api.addr_make("proposer");
        let offer = OpenInterest {
            liquidity_coin: cosmwasm_std::Coin::new(950u128, "uusd"),
            ..base_interest
        };

        execute(
            deps.as_mut(),
            mock_env(),
            message_info(&proposer, &[offer.liquidity_coin.clone()]),
            ExecuteMsg::ProposeCounterOffer(offer.clone()),
        )
        .expect("counter offer succeeds");

        let stored = COUNTER_OFFERS
            .load(deps.as_ref().storage, &proposer)
            .expect("counter offer stored");
        assert_eq!(stored, offer);
    }
}

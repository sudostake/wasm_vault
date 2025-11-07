#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{DepsMut, Env, MessageInfo, Response};

use super::{claim, delegate, redelegate, undelegate, withdraw};
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{OUTSTANDING_DEBT, OWNER};
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
            .save(deps.as_mut().storage, &0u128)
            .expect("zero debt stored");

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
            .save(deps.as_mut().storage, &0u128)
            .expect("zero debt stored");

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
            .save(deps.as_mut().storage, &0u128)
            .expect("zero debt stored");

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
            .save(deps.as_mut().storage, &0u128)
            .expect("zero debt stored");

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
}

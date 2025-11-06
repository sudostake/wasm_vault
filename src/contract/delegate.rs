use cosmwasm_std::{attr, Coin, DepsMut, Env, MessageInfo, Response, StakingMsg};

use crate::{state::OWNER, ContractError};

pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    validator: String,
    amount: Coin,
) -> Result<Response, ContractError> {
    let owner = OWNER.load(deps.storage)?;
    if info.sender != owner {
        return Err(ContractError::Unauthorized {});
    }

    if !info.funds.is_empty() {
        return Err(ContractError::FundsNotAccepted {});
    }

    if amount.amount.is_zero() {
        return Err(ContractError::InvalidDelegationAmount {});
    }

    let validator_addr = deps.api.addr_validate(&validator)?.into_string();
    let denom = amount.denom.clone();
    let requested = amount.amount;

    let balance = deps
        .querier
        .query_balance(env.contract.address.clone(), denom.clone())?;

    if balance.amount < requested {
        return Err(ContractError::InsufficientBalance {
            denom,
            available: balance.amount,
            requested,
        });
    }

    if deps
        .querier
        .query_validator(validator_addr.clone())?
        .is_none()
    {
        return Err(ContractError::ValidatorNotFound {
            validator: validator_addr,
        });
    }

    Ok(Response::new()
        .add_message(StakingMsg::Delegate {
            validator: validator_addr.clone(),
            amount: amount.clone(),
        })
        .add_attributes([
            attr("action", "delegate"),
            attr("validator", validator_addr),
            attr("denom", denom),
            attr("amount", requested.to_string()),
        ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env};
    use cosmwasm_std::{coins, Decimal, Validator};

    #[test]
    fn fails_for_unauthorized_sender() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        OWNER
            .save(deps.as_mut().storage, &owner)
            .expect("owner stored");

        let info = message_info(&deps.api.addr_make("intruder"), &[]);
        let amount = Coin::new(10u128, "ucosm");
        let validator = deps.api.addr_make("validator").into_string();
        let err = execute(deps.as_mut(), mock_env(), info, validator, amount).unwrap_err();

        assert!(matches!(err, ContractError::Unauthorized {}));
    }

    #[test]
    fn fails_for_zero_amount() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        OWNER
            .save(deps.as_mut().storage, &owner)
            .expect("owner stored");

        let info = message_info(&owner, &[]);
        let validator = deps.api.addr_make("validator").into_string();
        let err = execute(
            deps.as_mut(),
            mock_env(),
            info,
            validator,
            Coin::new(0u128, "ucosm"),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::InvalidDelegationAmount {}));
    }

    #[test]
    fn fails_when_funds_attached() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        OWNER
            .save(deps.as_mut().storage, &owner)
            .expect("owner stored");

        let info = message_info(&owner, &coins(10, "ucosm"));
        let validator = deps.api.addr_make("validator").into_string();
        let err = execute(
            deps.as_mut(),
            mock_env(),
            info,
            validator,
            Coin::new(10u128, "ucosm"),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::FundsNotAccepted {}));
    }

    #[test]
    fn fails_for_insufficient_balance() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        OWNER
            .save(deps.as_mut().storage, &owner)
            .expect("owner stored");

        let contract_addr = mock_env().contract.address;
        deps.querier
            .bank
            .update_balance(contract_addr.as_str(), coins(50, "ucosm"));

        let info = message_info(&owner, &[]);
        let amount = Coin::new(100u128, "ucosm");

        let validator = deps.api.addr_make("validator").into_string();
        let err = execute(deps.as_mut(), mock_env(), info, validator, amount).unwrap_err();

        assert!(matches!(err, ContractError::InsufficientBalance { .. }));
    }

    #[test]
    fn fails_for_missing_validator() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        OWNER
            .save(deps.as_mut().storage, &owner)
            .expect("owner stored");

        let env = mock_env();
        deps.querier
            .bank
            .update_balance(env.contract.address.as_str(), coins(100, "ucosm"));

        let info = message_info(&owner, &[]);
        let validator = deps.api.addr_make("validator").into_string();
        let err = execute(
            deps.as_mut(),
            env,
            info,
            validator,
            Coin::new(50u128, "ucosm"),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::ValidatorNotFound { .. }));
    }

    #[test]
    fn creates_delegate_message() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        OWNER
            .save(deps.as_mut().storage, &owner)
            .expect("owner stored");

        let env = mock_env();
        let validator = deps.api.addr_make("validator");
        let denom = "ucosm";

        deps.querier
            .bank
            .update_balance(env.contract.address.as_str(), coins(200, denom));

        let validator_addr = validator.clone().into_string();
        let validator_obj = Validator::create(
            validator_addr.clone(),
            Decimal::percent(5),
            Decimal::percent(10),
            Decimal::percent(1),
        );

        deps.querier.staking.update(denom, &[validator_obj], &[]);

        let info = message_info(&owner, &[]);
        let amount = Coin::new(150u128, denom);

        let response = execute(
            deps.as_mut(),
            env,
            info,
            validator_addr.clone(),
            amount.clone(),
        )
        .expect("delegation succeeds");

        assert_eq!(response.messages.len(), 1);
        let msg = response.messages[0].clone().msg;
        match msg {
            cosmwasm_std::CosmosMsg::Staking(StakingMsg::Delegate {
                validator,
                amount: delegated,
            }) => {
                assert_eq!(validator, validator_addr);
                assert_eq!(delegated, amount);
            }
            _ => panic!("unexpected message"),
        }
    }
}

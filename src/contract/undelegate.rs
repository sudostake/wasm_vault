use cosmwasm_std::{attr, Coin, DepsMut, Env, MessageInfo, Response, StakingMsg, Uint128, Uint256};

use crate::{state::OWNER, ContractError};

pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    validator: String,
    amount: Uint128,
) -> Result<Response, ContractError> {
    let owner = OWNER.load(deps.storage)?;
    if info.sender != owner {
        return Err(ContractError::Unauthorized {});
    }

    if amount.is_zero() {
        return Err(ContractError::InvalidUndelegationAmount {});
    }

    let validator_addr = deps.api.addr_validate(&validator)?.into_string();
    let denom = deps.querier.query_bonded_denom()?;
    let requested = Uint256::from(amount);

    let delegation = deps
        .querier
        .query_delegation(env.contract.address.clone(), validator_addr.clone())?
        .ok_or_else(|| ContractError::DelegationNotFound {
            validator: validator_addr.clone(),
        })?;

    if delegation.amount.amount < requested {
        return Err(ContractError::InsufficientDelegatedBalance {
            validator: validator_addr.clone(),
            delegated: delegation.amount.amount,
            requested,
        });
    }

    let undelegate_coin = Coin::new(requested, denom.clone());

    Ok(Response::new()
        .add_message(StakingMsg::Undelegate {
            validator: validator_addr.clone(),
            amount: undelegate_coin,
        })
        .add_attributes([
            attr("action", "undelegate"),
            attr("validator", validator_addr),
            attr("denom", denom),
            attr("amount", amount.to_string()),
        ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::OUTSTANDING_DEBT;
    use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env};
    use cosmwasm_std::{Addr, Coin, Decimal, FullDelegation, Storage, Uint128, Uint256, Validator};

    fn setup_owner_and_zero_debt(storage: &mut dyn Storage, owner: &Addr) {
        OWNER.save(storage, owner).expect("owner stored");
        OUTSTANDING_DEBT
            .save(storage, &0u128)
            .expect("zero debt stored");
    }

    #[test]
    fn fails_for_unauthorized_sender() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        let info = message_info(&deps.api.addr_make("intruder"), &[]);
        let validator = deps.api.addr_make("validator").into_string();
        let err =
            execute(deps.as_mut(), mock_env(), info, validator, Uint128::new(10)).unwrap_err();

        assert!(matches!(err, ContractError::Unauthorized {}));
    }

    #[test]
    fn fails_for_zero_amount() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        let info = message_info(&owner, &[]);
        let validator = deps.api.addr_make("validator").into_string();
        let err = execute(deps.as_mut(), mock_env(), info, validator, Uint128::zero()).unwrap_err();

        assert!(matches!(err, ContractError::InvalidUndelegationAmount {}));
    }

    #[test]
    fn fails_when_delegation_missing() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        let env = mock_env();
        deps.querier.staking.update("ucosm", &[], &[]);

        let info = message_info(&owner, &[]);
        let validator = deps.api.addr_make("validator").into_string();
        let err = execute(
            deps.as_mut(),
            env,
            info,
            validator.clone(),
            Uint128::new(10),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::DelegationNotFound { validator: v } if v == validator
        ));
    }

    #[test]
    fn fails_when_delegated_balance_insufficient() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        let env = mock_env();
        let contract_addr = env.contract.address.clone();
        let validator = deps.api.addr_make("validator");
        let validator_addr = validator.clone().into_string();

        let delegation = FullDelegation::create(
            contract_addr,
            validator_addr.clone(),
            Coin::new(75u128, "ucosm"),
            Coin::new(75u128, "ucosm"),
            vec![],
        );

        let validator_obj = Validator::create(
            validator_addr.clone(),
            Decimal::percent(5),
            Decimal::percent(10),
            Decimal::percent(1),
        );

        deps.querier
            .staking
            .update("ucosm", &[validator_obj], &[delegation]);

        let info = message_info(&owner, &[]);
        let err = execute(
            deps.as_mut(),
            env,
            info,
            validator_addr.clone(),
            Uint128::new(100),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::InsufficientDelegatedBalance {
                validator,
                delegated,
                requested,
            } if validator == validator_addr && delegated == Uint256::from(75u128) && requested == Uint256::from(100u128)
        ));
    }

    #[test]
    fn creates_undelegate_message() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        let env = mock_env();
        let contract_addr = env.contract.address.clone();
        let validator = deps.api.addr_make("validator");
        let validator_addr = validator.clone().into_string();

        let delegation = FullDelegation::create(
            contract_addr,
            validator_addr.clone(),
            Coin::new(300u128, "ucosm"),
            Coin::new(300u128, "ucosm"),
            vec![],
        );

        let validator_obj = Validator::create(
            validator_addr.clone(),
            Decimal::percent(5),
            Decimal::percent(10),
            Decimal::percent(1),
        );

        deps.querier
            .staking
            .update("ucosm", &[validator_obj], &[delegation]);

        let info = message_info(&owner, &[]);
        let amount = Uint128::new(150);

        let response = execute(deps.as_mut(), env, info, validator_addr.clone(), amount)
            .expect("undelegate succeeds");

        assert_eq!(response.messages.len(), 1);
        let msg = response.messages[0].clone().msg;
        match msg {
            cosmwasm_std::CosmosMsg::Staking(StakingMsg::Undelegate {
                validator,
                amount: undelegated,
            }) => {
                assert_eq!(validator, validator_addr);
                assert_eq!(undelegated, Coin::new(amount, "ucosm"));
            }
            _ => panic!("unexpected message"),
        }
    }

    #[test]
    fn allows_undelegation_even_with_outstanding_debt() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &750u128)
            .expect("debt stored");

        let env = mock_env();
        let contract_addr = env.contract.address.clone();
        let validator = deps.api.addr_make("validator");
        let validator_addr = validator.clone().into_string();

        let delegation = FullDelegation::create(
            contract_addr,
            validator_addr.clone(),
            Coin::new(400u128, "ucosm"),
            Coin::new(400u128, "ucosm"),
            vec![],
        );

        let validator_obj = Validator::create(
            validator_addr.clone(),
            Decimal::percent(5),
            Decimal::percent(10),
            Decimal::percent(1),
        );

        deps.querier
            .staking
            .update("ucosm", &[validator_obj], &[delegation]);

        let info = message_info(&owner, &[]);
        execute(deps.as_mut(), env, info, validator_addr, Uint128::new(200))
            .expect("undelegate succeeds even with debt");
    }
}

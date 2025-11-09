use cosmwasm_std::{attr, Coin, DepsMut, Env, MessageInfo, Response, StakingMsg, Uint128, Uint256};

use crate::{
    state::{OUTSTANDING_DEBT, OWNER},
    ContractError,
};

pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    src_validator: String,
    dst_validator: String,
    amount: Uint128,
) -> Result<Response, ContractError> {
    let owner = OWNER.load(deps.storage)?;
    if info.sender != owner {
        return Err(ContractError::Unauthorized {});
    }

    if amount.is_zero() {
        return Err(ContractError::InvalidRedelegationAmount {});
    }

    let src_addr = deps.api.addr_validate(&src_validator)?.into_string();
    let dst_addr = deps.api.addr_validate(&dst_validator)?.into_string();

    if src_addr == dst_addr {
        return Err(ContractError::RedelegateToSameValidator {});
    }

    let denom = deps.querier.query_bonded_denom()?;

    if let Some(debt) = OUTSTANDING_DEBT.load(deps.storage)? {
        if debt.denom == denom {
            return Err(ContractError::OutstandingDebt { amount: debt });
        }
    }

    let requested = Uint256::from(amount);

    let delegation = deps
        .querier
        .query_delegation(env.contract.address.clone(), src_addr.clone())?
        .ok_or_else(|| ContractError::DelegationNotFound {
            validator: src_addr.clone(),
        })?;

    if delegation.amount.amount < requested {
        return Err(ContractError::InsufficientDelegatedBalance {
            validator: src_addr.clone(),
            delegated: delegation.amount.amount,
            requested,
        });
    }

    if deps.querier.query_validator(dst_addr.clone())?.is_none() {
        return Err(ContractError::ValidatorNotFound {
            validator: dst_addr.clone(),
        });
    }

    let redelegate_coin = Coin::new(requested, denom.clone());

    Ok(Response::new()
        .add_message(StakingMsg::Redelegate {
            src_validator: src_addr.clone(),
            dst_validator: dst_addr.clone(),
            amount: redelegate_coin,
        })
        .add_attributes([
            attr("action", "redelegate"),
            attr("src_validator", src_addr),
            attr("dst_validator", dst_addr),
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
            .save(storage, &None)
            .expect("zero debt stored");
    }

    #[test]
    fn fails_for_unauthorized_sender() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        let info = message_info(&deps.api.addr_make("intruder"), &[]);
        let err = execute(
            deps.as_mut(),
            mock_env(),
            info,
            "validator".to_string(),
            "validator-two".to_string(),
            Uint128::new(10),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::Unauthorized {}));
    }

    #[test]
    fn fails_for_zero_amount() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        let info = message_info(&owner, &[]);
        let err = execute(
            deps.as_mut(),
            mock_env(),
            info,
            "validator".to_string(),
            "validator-two".to_string(),
            Uint128::zero(),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::InvalidRedelegationAmount {}));
    }

    #[test]
    fn fails_when_outstanding_debt_matches_bonded_denom() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);
        let bonded_denom = deps
            .as_ref()
            .querier
            .query_bonded_denom()
            .expect("bonded denom available");
        OUTSTANDING_DEBT
            .save(
                deps.as_mut().storage,
                &Some(Coin::new(250u128, bonded_denom.clone())),
            )
            .expect("debt stored");
        deps.querier
            .staking
            .update(bonded_denom.as_str(), &[], &[]);

        let info = message_info(&owner, &[]);
        let src_validator = deps.api.addr_make("validator").into_string();
        let dst_validator = deps.api.addr_make("validator-two").into_string();
        let err = execute(
            deps.as_mut(),
            mock_env(),
            info,
            src_validator.clone(),
            dst_validator,
            Uint128::new(10),
        )
        .unwrap_err();

        match err {
            ContractError::OutstandingDebt { amount } => {
                assert_eq!(amount, Coin::new(250u128, bonded_denom));
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn allows_redelegation_when_outstanding_debt_is_other_denom() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);
        let bonded_denom = deps
            .as_ref()
            .querier
            .query_bonded_denom()
            .expect("bonded denom available");
        let other_denom = format!("{bonded_denom}_alt");
        OUTSTANDING_DEBT
            .save(
                deps.as_mut().storage,
                &Some(Coin::new(250u128, other_denom)),
            )
            .expect("debt stored");

        let env = mock_env();
        let contract_addr = env.contract.address.clone();
        let src_validator_addr = deps.api.addr_make("validator").into_string();
        let dst_validator_addr = deps.api.addr_make("validator-two").into_string();

        let delegation = FullDelegation::create(
            contract_addr,
            src_validator_addr.clone(),
            Coin::new(300u128, bonded_denom.clone()),
            Coin::new(300u128, bonded_denom.clone()),
            vec![],
        );

        let src_validator_obj = Validator::create(
            src_validator_addr.clone(),
            Decimal::percent(5),
            Decimal::percent(10),
            Decimal::percent(1),
        );
        let dst_validator_obj = Validator::create(
            dst_validator_addr.clone(),
            Decimal::percent(4),
            Decimal::percent(9),
            Decimal::percent(1),
        );

        deps.querier.staking.update(
            bonded_denom.as_str(),
            &[src_validator_obj, dst_validator_obj],
            &[delegation],
        );

        let info = message_info(&owner, &[]);
        let amount = Uint128::new(50);

        let response = execute(
            deps.as_mut(),
            env,
            info,
            src_validator_addr.clone(),
            dst_validator_addr.clone(),
            amount,
        )
        .expect("redelegation succeeds");

        assert_eq!(response.messages.len(), 1);
    }

    #[test]
    fn fails_when_same_validator_used() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        let info = message_info(&owner, &[]);
        let validator = deps.api.addr_make("validator").into_string();

        let err = execute(
            deps.as_mut(),
            mock_env(),
            info,
            validator.clone(),
            validator,
            Uint128::new(10),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::RedelegateToSameValidator {}));
    }

    #[test]
    fn fails_when_delegation_missing() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        let env = mock_env();
        deps.querier.staking.update("ucosm", &[], &[]);

        let info = message_info(&owner, &[]);
        let src_validator = deps.api.addr_make("validator").into_string();
        let dst_validator = deps.api.addr_make("validator-two").into_string();

        let err = execute(
            deps.as_mut(),
            env,
            info,
            src_validator.clone(),
            dst_validator,
            Uint128::new(10),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::DelegationNotFound { validator: v } if v == src_validator
        ));
    }

    #[test]
    fn fails_when_delegated_balance_insufficient() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        let env = mock_env();
        let contract_addr = env.contract.address.clone();
        let src_validator_addr = deps.api.addr_make("validator").into_string();
        let dst_validator_addr = deps.api.addr_make("validator-two").into_string();

        let delegation = FullDelegation::create(
            contract_addr,
            src_validator_addr.clone(),
            Coin::new(40u128, "ucosm"),
            Coin::new(40u128, "ucosm"),
            vec![],
        );

        let src_validator_obj = Validator::create(
            src_validator_addr.clone(),
            Decimal::percent(5),
            Decimal::percent(10),
            Decimal::percent(1),
        );

        deps.querier
            .staking
            .update("ucosm", &[src_validator_obj], &[delegation]);

        let info = message_info(&owner, &[]);
        let err = execute(
            deps.as_mut(),
            env,
            info,
            src_validator_addr.clone(),
            dst_validator_addr,
            Uint128::new(100),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::InsufficientDelegatedBalance {
                validator,
                delegated,
                requested,
            } if validator == src_validator_addr && delegated == Uint256::from(40u128) && requested == Uint256::from(100u128)
        ));
    }

    #[test]
    fn fails_when_destination_validator_missing() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        let env = mock_env();
        let contract_addr = env.contract.address.clone();
        let src_validator = deps.api.addr_make("validator").into_string();
        let dst_validator = deps.api.addr_make("validator-two").into_string();

        let delegation = FullDelegation::create(
            contract_addr.clone(),
            src_validator.clone(),
            Coin::new(120u128, "ucosm"),
            Coin::new(120u128, "ucosm"),
            vec![],
        );

        let src_validator_obj = Validator::create(
            src_validator.clone(),
            Decimal::percent(5),
            Decimal::percent(10),
            Decimal::percent(1),
        );

        deps.querier
            .staking
            .update("ucosm", &[src_validator_obj], &[delegation]);

        let info = message_info(&owner, &[]);
        let err = execute(
            deps.as_mut(),
            env,
            info,
            src_validator,
            dst_validator.clone(),
            Uint128::new(50),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::ValidatorNotFound { validator } if validator == dst_validator
        ));
    }

    #[test]
    fn creates_redelegate_message() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        let env = mock_env();
        let contract_addr = env.contract.address.clone();
        let src_validator_addr = deps.api.addr_make("validator").into_string();
        let dst_validator_addr = deps.api.addr_make("validator-two").into_string();

        let delegation = FullDelegation::create(
            contract_addr,
            src_validator_addr.clone(),
            Coin::new(300u128, "ucosm"),
            Coin::new(300u128, "ucosm"),
            vec![],
        );

        let src_validator_obj = Validator::create(
            src_validator_addr.clone(),
            Decimal::percent(5),
            Decimal::percent(10),
            Decimal::percent(1),
        );
        let dst_validator_obj = Validator::create(
            dst_validator_addr.clone(),
            Decimal::percent(4),
            Decimal::percent(9),
            Decimal::percent(1),
        );

        deps.querier.staking.update(
            "ucosm",
            &[src_validator_obj, dst_validator_obj],
            &[delegation],
        );

        let info = message_info(&owner, &[]);
        let amount = Uint128::new(150);

        let response = execute(
            deps.as_mut(),
            env,
            info,
            src_validator_addr.clone(),
            dst_validator_addr.clone(),
            amount,
        )
        .expect("redelegate succeeds");

        assert_eq!(response.messages.len(), 1);
        let msg = response.messages[0].clone().msg;
        match msg {
            cosmwasm_std::CosmosMsg::Staking(StakingMsg::Redelegate {
                src_validator,
                dst_validator,
                amount: redelegated,
            }) => {
                assert_eq!(src_validator, src_validator_addr);
                assert_eq!(dst_validator, dst_validator_addr);
                assert_eq!(redelegated, Coin::new(amount, "ucosm"));
            }
            _ => panic!("unexpected message"),
        }
    }
}

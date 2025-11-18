use cosmwasm_std::{
    attr, Coin, Deps, DepsMut, Env, MessageInfo, Response, StakingMsg, Uint128, Uint256,
};
use std::convert::TryFrom;

use crate::{
    helpers::require_owner,
    state::{LENDER, OPEN_INTEREST, OUTSTANDING_DEBT},
    ContractError,
};

pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    validator: String,
    amount: Uint128,
) -> Result<Response, ContractError> {
    require_owner(&deps, &info)?;

    if amount.is_zero() {
        return Err(ContractError::InvalidDelegationAmount {});
    }

    let validator_addr = deps.api.addr_validate(&validator)?.into_string();
    let denom = deps.querier.query_bonded_denom()?;
    let requested = Uint256::from(amount);

    let reserved_debt = reserved_debt_for_denom(&deps.as_ref(), &denom)?;

    let balance = deps
        .querier
        .query_balance(env.contract.address.clone(), denom.clone())?;
    let available_after_reserved = balance.amount.saturating_sub(reserved_debt);

    if available_after_reserved < requested {
        return Err(ContractError::InsufficientBalance {
            denom: denom.clone(),
            available: Uint128::try_from(available_after_reserved).expect("available fits in u128"),
            requested: Uint128::try_from(requested).expect("requested fits in u128"),
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

    let delegate_coin = Coin::new(requested, denom.clone());

    Ok(Response::new()
        .add_message(StakingMsg::Delegate {
            validator: validator_addr.clone(),
            amount: delegate_coin.clone(),
        })
        .add_attributes([
            attr("action", "delegate"),
            attr("validator", validator_addr),
            attr("denom", denom),
            attr("amount", amount.to_string()),
        ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        state::{LENDER, OPEN_INTEREST, OUTSTANDING_DEBT, OWNER},
        types::OpenInterest,
    };
    use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env};
    use cosmwasm_std::{coins, Addr, Coin, Decimal, Storage, Uint128, Validator};

    fn setup_owner_and_zero_debt(storage: &mut dyn Storage, owner: &Addr) {
        OWNER.save(storage, owner).expect("owner stored");
        OUTSTANDING_DEBT
            .save(storage, &None)
            .expect("zero debt stored");
        LENDER.save(storage, &None).expect("lender cleared");
        OPEN_INTEREST
            .save(storage, &None)
            .expect("open interest cleared");
    }

    #[test]
    fn fails_for_unauthorized_sender() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        let info = message_info(&deps.api.addr_make("intruder"), &[]);
        let amount = Uint128::new(10);
        let validator = deps.api.addr_make("validator").into_string();
        let err = execute(deps.as_mut(), mock_env(), info, validator, amount).unwrap_err();

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

        assert!(matches!(err, ContractError::InvalidDelegationAmount {}));
    }

    #[test]
    fn fails_for_insufficient_balance() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        let contract_addr = mock_env().contract.address;
        deps.querier
            .bank
            .update_balance(contract_addr.as_str(), coins(50, "ucosm"));

        let info = message_info(&owner, &[]);
        let amount = Uint128::new(100);

        let validator = deps.api.addr_make("validator").into_string();
        let err = execute(deps.as_mut(), mock_env(), info, validator, amount).unwrap_err();

        assert!(matches!(err, ContractError::InsufficientBalance { .. }));
    }

    #[test]
    fn fails_for_missing_validator() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        let env = mock_env();
        deps.querier.staking.update("ucosm", &[], &[]);
        deps.querier
            .bank
            .update_balance(env.contract.address.as_str(), coins(100, "ucosm"));

        let info = message_info(&owner, &[]);
        let validator = deps.api.addr_make("validator").into_string();
        let err = execute(deps.as_mut(), env, info, validator, Uint128::new(50)).unwrap_err();

        assert!(matches!(err, ContractError::ValidatorNotFound { .. }));
    }

    #[test]
    fn fails_when_outstanding_debt_exists_for_denom() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &Some(Coin::new(500u128, "ucosm")))
            .expect("debt stored");

        deps.querier.staking.update("ucosm", &[], &[]);

        let info = message_info(&owner, &[]);
        let validator = deps.api.addr_make("validator").into_string();
        let err =
            execute(deps.as_mut(), mock_env(), info, validator, Uint128::new(50)).unwrap_err();

        assert!(matches!(
            err,
            ContractError::OutstandingDebt { amount }
                if amount == Coin::new(500u128, "ucosm")
        ));
    }

    #[test]
    fn allows_delegation_when_outstanding_debt_is_other_denom() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &Some(Coin::new(750u128, "uatom")))
            .expect("debt stored");

        let env = mock_env();
        let denom = "ucosm";
        let validator = deps.api.addr_make("validator");

        deps.querier
            .bank
            .update_balance(env.contract.address.as_str(), coins(300, denom));

        let validator_addr = validator.clone().into_string();
        let validator_obj = Validator::create(
            validator_addr.clone(),
            Decimal::percent(5),
            Decimal::percent(10),
            Decimal::percent(1),
        );

        deps.querier.staking.update(denom, &[validator_obj], &[]);

        let info = message_info(&owner, &[]);
        let amount = Uint128::new(200);

        let response =
            execute(deps.as_mut(), env, info, validator_addr.clone(), amount).expect("succeeds");

        assert_eq!(response.messages.len(), 1);
    }

    #[test]
    fn allows_delegation_with_counter_offer_outstanding_debt() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        let denom = "ucosm";
        let open_interest = OpenInterest {
            liquidity_coin: Coin::new(400u128, denom),
            interest_coin: Coin::new(20u128, "ujuno"),
            expiry_duration: 86_400u64,
            collateral: Coin::new(200u128, "uatom"),
        };

        OPEN_INTEREST
            .save(deps.as_mut().storage, &Some(open_interest))
            .expect("open interest stored");

        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &Some(Coin::new(150u128, denom)))
            .expect("debt stored");

        let env = mock_env();
        deps.querier
            .bank
            .update_balance(env.contract.address.as_str(), coins(600, denom));

        let validator = deps.api.addr_make("validator");
        let validator_addr = validator.clone().into_string();
        let validator_obj = Validator::create(
            validator_addr.clone(),
            Decimal::percent(5),
            Decimal::percent(10),
            Decimal::percent(1),
        );

        deps.querier.staking.update(denom, &[validator_obj], &[]);

        let info = message_info(&owner, &[]);
        let amount = Uint128::new(200);

        let response =
            execute(deps.as_mut(), env, info, validator_addr.clone(), amount).expect("succeeds");

        assert_eq!(response.messages.len(), 1);
    }

    #[test]
    fn fails_when_reserved_debt_blocks_delegation() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        let denom = "ucosm";
        let open_interest = OpenInterest {
            liquidity_coin: Coin::new(400u128, denom),
            interest_coin: Coin::new(20u128, "ujuno"),
            expiry_duration: 86_400u64,
            collateral: Coin::new(200u128, "uatom"),
        };

        OPEN_INTEREST
            .save(deps.as_mut().storage, &Some(open_interest))
            .expect("open interest stored");

        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &Some(Coin::new(450u128, denom)))
            .expect("debt stored");

        let env = mock_env();
        deps.querier
            .bank
            .update_balance(env.contract.address.as_str(), coins(500, denom));

        let validator = deps.api.addr_make("validator");
        let validator_addr = validator.clone().into_string();
        let validator_obj = Validator::create(
            validator_addr.clone(),
            Decimal::percent(5),
            Decimal::percent(10),
            Decimal::percent(1),
        );

        deps.querier.staking.update(denom, &[validator_obj], &[]);

        let info = message_info(&owner, &[]);
        let amount = Uint128::new(100);

        let err = execute(deps.as_mut(), env, info, validator_addr, amount).unwrap_err();

        assert!(matches!(
            err,
            ContractError::InsufficientBalance { denom, available, requested }
                if denom == "ucosm"
                    && available == Uint128::from(50u128)
                    && requested == Uint128::from(100u128)
        ));
    }

    #[test]
    fn creates_delegate_message() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

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
        let amount = Uint128::new(150);

        let response = execute(deps.as_mut(), env, info, validator_addr.clone(), amount)
            .expect("delegation succeeds");

        assert_eq!(response.messages.len(), 1);
        let msg = response.messages[0].clone().msg;
        match msg {
            cosmwasm_std::CosmosMsg::Staking(StakingMsg::Delegate {
                validator,
                amount: delegated,
            }) => {
                assert_eq!(validator, validator_addr);
                assert_eq!(delegated, Coin::new(amount, denom));
            }
            _ => panic!("unexpected message"),
        }
    }
}

fn reserved_debt_for_denom(deps: &Deps, denom: &str) -> Result<Uint256, ContractError> {
    if let Some(debt) = OUTSTANDING_DEBT.load(deps.storage)? {
        if debt.denom == denom {
            let has_open_interest = OPEN_INTEREST.load(deps.storage)?.is_some();
            let lender_exists = LENDER.load(deps.storage)?.is_some();

            if has_open_interest && !lender_exists {
                // Reserve the outstanding debt only for counter-offer escrow (open interest without lender).
                return Ok(debt.amount);
            }

            return Err(ContractError::OutstandingDebt { amount: debt });
        }
    }

    Ok(Uint256::zero())
}

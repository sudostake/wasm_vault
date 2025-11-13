use cosmwasm_std::{
    attr, BankMsg, Coin, Deps, DepsMut, Env, MessageInfo, Response, StdError, StdResult, Uint128,
    Uint256,
};

use crate::{
    state::{OPEN_INTEREST, OUTSTANDING_DEBT, OWNER},
    types::OpenInterest,
    ContractError,
};
use std::cmp::max;

pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    denom: String,
    amount: Uint128,
    recipient: Option<String>,
) -> Result<Response, ContractError> {
    let owner = OWNER.load(deps.storage)?;
    if info.sender != owner {
        return Err(ContractError::Unauthorized {});
    }

    if amount.is_zero() {
        return Err(ContractError::InvalidWithdrawalAmount {});
    }

    let outstanding_debt = OUTSTANDING_DEBT.load(deps.storage)?;
    let open_interest = OPEN_INTEREST.load(deps.storage)?;
    let deps_ref = deps.as_ref();

    let requested = Uint256::from(amount);
    let withdrawable =
        available_to_withdraw(&deps_ref, &env, &denom, &outstanding_debt, &open_interest)?;

    if withdrawable < requested {
        return Err(ContractError::InsufficientBalance {
            denom: denom.clone(),
            available: withdrawable,
            requested,
        });
    }

    let recipient_addr = match recipient {
        Some(addr) => deps.api.addr_validate(&addr)?,
        None => owner,
    };
    let recipient_str = recipient_addr.to_string();

    let withdraw_coin = Coin::new(amount, denom.clone());

    Ok(Response::new()
        .add_message(BankMsg::Send {
            to_address: recipient_str.clone(),
            amount: vec![withdraw_coin],
        })
        .add_attributes([
            attr("action", "withdraw"),
            attr("denom", denom),
            attr("amount", amount.to_string()),
            attr("recipient", recipient_str),
        ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contract::open_interest::test_helpers::{build_open_interest, sample_coin},
        state::{OPEN_INTEREST, OUTSTANDING_DEBT},
    };
    use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env};
    use cosmwasm_std::{
        coins, Addr, Coin, DecCoin, Decimal, Decimal256, FullDelegation, Storage, Uint128, Uint256,
        Validator,
    };

    fn setup_owner_and_zero_debt(storage: &mut dyn Storage, owner: &Addr) {
        OWNER.save(storage, owner).expect("owner stored");
        OUTSTANDING_DEBT
            .save(storage, &None)
            .expect("zero debt stored");
        OPEN_INTEREST
            .save(storage, &None)
            .expect("open interest cleared");
    }

    #[test]
    fn fails_for_unauthorized_sender() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);
        let intruder = deps.api.addr_make("intruder");

        let err = execute(
            deps.as_mut(),
            mock_env(),
            message_info(&intruder, &[]),
            "ucosm".to_string(),
            Uint128::new(50),
            None,
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::Unauthorized {}));
    }

    #[test]
    fn fails_for_zero_amount() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        let err = execute(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            "ucosm".to_string(),
            Uint128::zero(),
            None,
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::InvalidWithdrawalAmount {}));
    }

    #[test]
    fn fails_for_outstanding_debt_on_matching_denom() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &Some(Coin::new(250u128, "ucosm")))
            .expect("debt stored");

        let env = mock_env();
        deps.querier
            .bank
            .update_balance(env.contract.address.as_str(), coins(200, "ucosm"));

        let err = execute(
            deps.as_mut(),
            env,
            message_info(&owner, &[]),
            "ucosm".to_string(),
            Uint128::new(10),
            None,
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::InsufficientBalance {
                denom,
                available,
                requested,
            } if denom == "ucosm"
                && available == Uint256::zero()
                && requested == Uint256::from(10u128)
        ));
    }

    #[test]
    fn allows_withdraw_when_balance_exceeds_outstanding_debt() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &Some(Coin::new(250u128, "ucosm")))
            .expect("debt stored");

        let env = mock_env();
        deps.querier
            .bank
            .update_balance(env.contract.address.as_str(), coins(600, "ucosm"));

        let response = execute(
            deps.as_mut(),
            env,
            message_info(&owner, &[]),
            "ucosm".to_string(),
            Uint128::new(200),
            None,
        )
        .expect("withdraw succeeds");

        assert_eq!(response.messages.len(), 1);
        let msg = response.messages[0].clone().msg;
        match msg {
            cosmwasm_std::CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                assert_eq!(to_address, owner.to_string());
                assert_eq!(amount, vec![Coin::new(200u128, "ucosm")]);
            }
            _ => panic!("unexpected message"),
        }
    }

    #[test]
    fn fails_for_insufficient_balance() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        let err = execute(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            "ucosm".to_string(),
            Uint128::new(100),
            None,
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::InsufficientBalance { .. }));
    }

    #[test]
    fn sends_funds_to_owner_when_no_recipient_provided() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        let env = mock_env();
        deps.querier
            .bank
            .update_balance(env.contract.address.as_str(), coins(400, "ucosm"));

        let response = execute(
            deps.as_mut(),
            env,
            message_info(&owner, &[]),
            "ucosm".to_string(),
            Uint128::new(150),
            None,
        )
        .expect("withdraw succeeds");

        assert_eq!(response.messages.len(), 1);
        let msg = response.messages[0].clone().msg;
        match msg {
            cosmwasm_std::CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                assert_eq!(to_address, owner.to_string());
                assert_eq!(amount, vec![Coin::new(150u128, "ucosm")]);
            }
            _ => panic!("unexpected message"),
        }
    }

    #[test]
    fn sends_funds_to_custom_recipient() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let recipient = deps.api.addr_make("friend");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        let env = mock_env();
        deps.querier
            .bank
            .update_balance(env.contract.address.as_str(), coins(500, "ucosm"));

        let response = execute(
            deps.as_mut(),
            env,
            message_info(&owner, &[]),
            "ucosm".to_string(),
            Uint128::new(200),
            Some(recipient.to_string()),
        )
        .expect("withdraw succeeds");

        assert_eq!(response.messages.len(), 1);
        let msg = response.messages[0].clone().msg;
        match msg {
            cosmwasm_std::CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                assert_eq!(to_address, recipient.to_string());
                assert_eq!(amount, vec![Coin::new(200u128, "ucosm")]);
            }
            _ => panic!("unexpected message"),
        }
    }

    #[test]
    fn allows_withdrawal_when_denom_differs_from_debt() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &Some(Coin::new(999u128, "ucosm")))
            .expect("debt stored");

        let env = mock_env();
        let other_denom = "uother".to_string();
        deps.querier.bank.update_balance(
            env.contract.address.as_str(),
            coins(600, other_denom.as_str()),
        );

        let response = execute(
            deps.as_mut(),
            env,
            message_info(&owner, &[]),
            other_denom.clone(),
            Uint128::new(250),
            None,
        )
        .expect("withdraw succeeds for non-bonded denom");

        assert_eq!(response.messages.len(), 1);
        let msg = response.messages[0].clone().msg;
        match msg {
            cosmwasm_std::CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                assert_eq!(to_address, owner.to_string());
                assert_eq!(amount, vec![Coin::new(250u128, other_denom)]);
            }
            _ => panic!("unexpected message"),
        }
    }

    #[test]
    fn blocks_withdrawal_below_unfunded_nonbonded_collateral() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        let env = mock_env();
        let bonded_denom = "uosm".to_string();
        deps.querier.staking.update(bonded_denom.clone(), &[], &[]);
        let collateral_denom = "uother".to_string();

        deps.querier.bank.update_balance(
            env.contract.address.as_str(),
            coins(200, collateral_denom.as_str()),
        );

        let open_interest = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, collateral_denom.as_str()),
        );
        OPEN_INTEREST
            .save(deps.as_mut().storage, &Some(open_interest))
            .expect("open interest stored");

        let err = execute(
            deps.as_mut(),
            env.clone(),
            message_info(&owner, &[]),
            collateral_denom.clone(),
            Uint128::new(10),
            None,
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::InsufficientBalance {
                denom,
                available,
                requested,
            } if denom == collateral_denom
                && available == Uint256::zero()
                && requested == Uint256::from(10u128)
        ));
    }

    #[test]
    fn blocks_withdrawal_below_unfunded_staked_collateral() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);

        let env = mock_env();
        let bonded_denom = "ucosm".to_string();

        deps.querier.bank.update_balance(
            env.contract.address.as_str(),
            coins(120, bonded_denom.as_str()),
        );

        let validator = stub_validator();
        let delegation =
            staking_delegation(env.contract.address.clone(), 100, bonded_denom.as_str());
        deps.querier
            .staking
            .update(bonded_denom.as_str(), &[validator.clone()], &[delegation]);
        deps.querier.distribution.set_rewards(
            validator.address.clone(),
            env.contract.address.as_str(),
            vec![reward_coin(30, bonded_denom.as_str())],
        );

        let open_interest = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, bonded_denom.as_str()),
        );
        OPEN_INTEREST
            .save(deps.as_mut().storage, &Some(open_interest))
            .expect("open interest stored");

        let err = execute(
            deps.as_mut(),
            env.clone(),
            message_info(&owner, &[]),
            bonded_denom.clone(),
            Uint128::new(100),
            None,
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::InsufficientBalance {
                denom,
                available,
                requested,
            } if denom == bonded_denom
                && available == Uint256::from(50u128)
                && requested == Uint256::from(100u128)
        ));
    }

    fn stub_validator() -> Validator {
        Validator::create(
            "validator".to_string(),
            Decimal::percent(5),
            Decimal::percent(10),
            Decimal::percent(1),
        )
    }

    fn staking_delegation(addr: Addr, amount: u128, denom: &str) -> FullDelegation {
        FullDelegation::create(
            addr,
            "validator".to_string(),
            Coin::new(amount, denom),
            Coin::new(amount, denom),
            vec![],
        )
    }

    fn reward_coin(amount: u128, denom: &str) -> DecCoin {
        DecCoin::new(
            Decimal256::from_atomics(Uint256::from(amount), 0).unwrap(),
            denom,
        )
    }
}

fn available_to_withdraw(
    deps: &Deps,
    env: &Env,
    denom: &str,
    outstanding_debt: &Option<Coin>,
    open_interest: &Option<OpenInterest>,
) -> StdResult<Uint256> {
    let balance = deps
        .querier
        .query_balance(env.contract.address.clone(), denom.to_string())?;
    let available = Uint256::from(balance.amount);

    let collateral_lock = collateral_lock_for_denom(deps, env, denom, open_interest)?;
    let debt_requirement = match outstanding_debt {
        Some(debt) if debt.denom == denom => debt.amount,
        _ => Uint256::zero(),
    };

    let required_minimum = max(debt_requirement, collateral_lock);
    Ok(available.saturating_sub(required_minimum))
}

fn collateral_lock_for_denom(
    deps: &Deps,
    env: &Env,
    denom: &str,
    open_interest: &Option<OpenInterest>,
) -> StdResult<Uint256> {
    let Some(interest) = open_interest else {
        return Ok(Uint256::zero());
    };

    if interest.collateral.denom != denom {
        return Ok(Uint256::zero());
    }

    let bonded_denom = deps.querier.query_bonded_denom()?;
    if denom != bonded_denom {
        return Ok(interest.collateral.amount);
    }

    let rewards = query_staking_rewards_for_denom(deps, env, denom)?;
    let staked = query_staked_balance(deps, env, denom)?;
    let coverage = rewards.checked_add(staked).map_err(StdError::from)?;

    Ok(interest.collateral.amount.saturating_sub(coverage))
}

fn query_staking_rewards_for_denom(deps: &Deps, env: &Env, denom: &str) -> StdResult<Uint256> {
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

fn query_staked_balance(deps: &Deps, env: &Env, denom: &str) -> StdResult<Uint256> {
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

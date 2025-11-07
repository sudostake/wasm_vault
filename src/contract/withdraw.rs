use cosmwasm_std::{attr, BankMsg, Coin, DepsMut, Env, MessageInfo, Response, Uint128, Uint256};

use crate::{
    state::{OUTSTANDING_DEBT, OWNER},
    ContractError,
};

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

    let bonded_denom = deps.querier.query_bonded_denom()?;
    if denom == bonded_denom {
        let debt = OUTSTANDING_DEBT.load(deps.storage)?;
        if debt > 0 {
            return Err(ContractError::OutstandingDebt {
                amount: Uint128::from(debt),
            });
        }
    }

    let requested = Uint256::from(amount);
    let balance = deps
        .querier
        .query_balance(env.contract.address.clone(), denom.clone())?;

    let available = Uint256::from(balance.amount);
    if available < requested {
        return Err(ContractError::InsufficientBalance {
            denom: denom.clone(),
            available,
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
    use crate::state::OUTSTANDING_DEBT;
    use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env};
    use cosmwasm_std::{coins, Addr, Storage};

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
    fn fails_for_outstanding_debt_on_bonded_denom() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);
        deps.querier.staking.update("ucosm", &[], &[]);

        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &250u128)
            .expect("debt stored");

        let err = execute(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            "ucosm".to_string(),
            Uint128::new(10),
            None,
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::OutstandingDebt { amount } if amount == Uint128::new(250)
        ));
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
    fn allows_withdrawal_of_non_bonded_denom_even_with_debt() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner_and_zero_debt(deps.as_mut().storage, &owner);
        deps.querier.staking.update("ucosm", &[], &[]);

        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &999u128)
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
}

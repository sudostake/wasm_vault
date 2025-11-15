use cosmwasm_std::{attr, BankMsg, Coin, DepsMut, Env, MessageInfo, Response};

use crate::{
    helpers::require_owner,
    state::{LENDER, OPEN_INTEREST, OPEN_INTEREST_EXPIRY, OUTSTANDING_DEBT},
    ContractError,
};

use super::helpers::{build_repayment_amounts, open_interest_attributes};

pub fn repay(deps: DepsMut, env: Env, info: MessageInfo) -> Result<Response, ContractError> {
    require_owner(&deps, &info)?;

    if let Some(debt) = OUTSTANDING_DEBT.load(deps.storage)? {
        return Err(ContractError::OutstandingDebt { amount: debt });
    }

    let open_interest = OPEN_INTEREST
        .load(deps.storage)?
        .ok_or(ContractError::NoOpenInterest {})?;

    let lender = LENDER
        .load(deps.storage)?
        .ok_or(ContractError::NoLender {})?;

    let repayment_amounts = build_repayment_amounts(&open_interest)?;
    let contract_addr = env.contract.address.clone();

    let mut repayment_coins = Vec::with_capacity(repayment_amounts.len());
    for (denom, requested_amount, coin_amount) in repayment_amounts {
        let balance = deps
            .querier
            .query_balance(contract_addr.clone(), denom.clone())?;
        let available_amount = balance.amount;

        if available_amount < requested_amount {
            return Err(ContractError::InsufficientBalance {
                denom: denom.clone(),
                available: available_amount,
                requested: requested_amount,
            });
        }

        repayment_coins.push(Coin::new(coin_amount, denom));
    }

    OPEN_INTEREST.save(deps.storage, &None)?;
    LENDER.save(deps.storage, &None)?;
    OPEN_INTEREST_EXPIRY.save(deps.storage, &None)?;
    let mut attrs = open_interest_attributes("repay_open_interest", &open_interest);
    attrs.push(attr("lender", lender.as_str()));

    let response = Response::new()
        .add_attributes(attrs)
        .add_message(BankMsg::Send {
            to_address: lender.to_string(),
            amount: repayment_coins,
        });

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contract::open_interest::test_helpers::{
            build_open_interest, sample_coin, setup, setup_active_open_interest,
        },
        state::{LENDER, OPEN_INTEREST, OUTSTANDING_DEBT},
        ContractError,
    };
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        BankMsg,
    };
    use std::collections::BTreeMap;

    #[test]
    fn repay_rejects_without_active_open_interest() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut().storage, &owner);

        let err = repay(deps.as_mut(), mock_env(), message_info(&owner, &[])).unwrap_err();

        assert!(matches!(err, ContractError::NoOpenInterest {}));
    }

    #[test]
    fn repay_rejects_without_lender() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut().storage, &owner);

        let interest = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(15, "uinterest"),
            86_400,
            sample_coin(200, "uatom"),
        );
        OPEN_INTEREST
            .save(deps.as_mut().storage, &Some(interest))
            .expect("open interest stored");

        let err = repay(deps.as_mut(), mock_env(), message_info(&owner, &[])).unwrap_err();

        assert!(matches!(err, ContractError::NoLender {}));
    }

    #[test]
    fn repay_rejects_non_owner_senders() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let lender = deps.api.addr_make("lender");
        let interest = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(15, "uinterest"),
            86_400,
            sample_coin(200, "uatom"),
        );
        setup_active_open_interest(deps.as_mut().storage, &owner, &lender, &interest);

        let intruder = deps.api.addr_make("intruder");
        let err = repay(deps.as_mut(), mock_env(), message_info(&intruder, &[])).unwrap_err();

        assert!(matches!(err, ContractError::Unauthorized {}));
    }

    #[test]
    fn repay_rejects_insufficient_liquidity() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let lender = deps.api.addr_make("lender");
        let interest = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(15, "uinterest"),
            86_400,
            sample_coin(200, "uatom"),
        );
        setup_active_open_interest(deps.as_mut().storage, &owner, &lender, &interest);

        let env = mock_env();
        deps.querier.bank.update_balance(
            env.contract.address.as_str(),
            vec![interest.interest_coin.clone()],
        );

        let err = repay(deps.as_mut(), env, message_info(&owner, &[])).unwrap_err();

        assert!(matches!(
            err,
            ContractError::InsufficientBalance { denom, .. }
                if denom == interest.liquidity_coin.denom
        ));
    }

    #[test]
    fn repay_rejects_insufficient_interest() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let lender = deps.api.addr_make("lender");
        let interest = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(15, "uinterest"),
            86_400,
            sample_coin(200, "uatom"),
        );
        setup_active_open_interest(deps.as_mut().storage, &owner, &lender, &interest);

        let env = mock_env();
        deps.querier.bank.update_balance(
            env.contract.address.as_str(),
            vec![interest.liquidity_coin.clone()],
        );

        let err = repay(deps.as_mut(), env, message_info(&owner, &[])).unwrap_err();

        assert!(matches!(
            err,
            ContractError::InsufficientBalance { denom, .. }
                if denom == interest.interest_coin.denom
        ));
    }

    #[test]
    fn repay_rejects_with_outstanding_debt() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let lender = deps.api.addr_make("lender");
        let interest = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(15, "uinterest"),
            86_400,
            sample_coin(200, "uatom"),
        );
        setup_active_open_interest(deps.as_mut().storage, &owner, &lender, &interest);

        OUTSTANDING_DEBT
            .save(
                deps.as_mut().storage,
                &Some(interest.liquidity_coin.clone()),
            )
            .expect("debt stored");

        let err = repay(deps.as_mut(), mock_env(), message_info(&owner, &[])).unwrap_err();

        assert!(matches!(
            err,
            ContractError::OutstandingDebt { amount }
                if amount == interest.liquidity_coin
        ));
    }

    #[test]
    fn repay_succeeds_and_clears_state() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let lender = deps.api.addr_make("lender");
        let interest = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(15, "uinterest"),
            86_400,
            sample_coin(200, "uatom"),
        );
        setup_active_open_interest(deps.as_mut().storage, &owner, &lender, &interest);

        let env = mock_env();
        deps.querier.bank.update_balance(
            env.contract.address.as_str(),
            vec![
                interest.liquidity_coin.clone(),
                interest.interest_coin.clone(),
            ],
        );

        let response =
            repay(deps.as_mut(), env.clone(), message_info(&owner, &[])).expect("repay succeeds");

        assert!(response
            .attributes
            .iter()
            .any(|attr| attr.key == "action" && attr.value == "repay_open_interest"));
        assert!(response
            .attributes
            .iter()
            .any(|attr| attr.key == "lender" && attr.value == lender.to_string()));

        let send_msg = match &response.messages[0].msg {
            cosmwasm_std::CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                assert_eq!(to_address, lender.as_str());
                amount.clone()
            }
            msg => panic!("unexpected message: {msg:?}"),
        };

        let mut sent = BTreeMap::new();
        for coin in send_msg {
            sent.insert(coin.denom.clone(), coin.amount);
        }

        let mut expected = BTreeMap::new();
        expected.insert(
            interest.liquidity_coin.denom.clone(),
            interest.liquidity_coin.amount,
        );
        expected.insert(
            interest.interest_coin.denom.clone(),
            interest.interest_coin.amount,
        );

        assert_eq!(sent, expected);

        assert!(OPEN_INTEREST
            .load(deps.as_ref().storage)
            .expect("interest fetched")
            .is_none());
        assert!(LENDER
            .load(deps.as_ref().storage)
            .expect("lender fetched")
            .is_none());
        assert!(OUTSTANDING_DEBT
            .load(deps.as_ref().storage)
            .expect("debt fetched")
            .is_none());
    }
}

use cosmwasm_std::{attr, DepsMut, Env, MessageInfo, Response};

use crate::ContractError;

use super::helpers::{
    collect_funds, finalize_state, get_outstanding_amount, load_liquidation_state,
    open_interest_attributes, payout_message, push_nonzero_attr, schedule_undelegations,
    CollectedFunds,
};

pub fn liquidate(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let state = load_liquidation_state(&deps, &env, &info)?;
    let remaining = get_outstanding_amount(&state, &deps)?;

    let mut messages = Vec::new();
    let CollectedFunds {
        available,
        rewards_claimed,
        reward_claim_messages,
    } = collect_funds(&state, &deps.as_ref(), &env, remaining)?;
    messages.extend(reward_claim_messages);

    let payout_amount = available.min(remaining);

    if !payout_amount.is_zero() {
        messages.push(payout_message(&state, payout_amount)?);
    }
    let remaining_after_payout = remaining
        .checked_sub(payout_amount)
        .expect("liquidation remaining underflow");

    if !remaining_after_payout.is_zero() && state.collateral_denom != state.bonded_denom {
        return Err(ContractError::InsufficientBalance {
            denom: state.collateral_denom.clone(),
            available,
            requested: remaining,
        });
    }

    let (undelegate_msgs, undelegated_amount) =
        schedule_undelegations(&state, &deps.as_ref(), remaining_after_payout)?;
    messages.extend(undelegate_msgs);

    let settled_remaining = remaining_after_payout
        .checked_sub(undelegated_amount)
        .expect("settled remaining underflow");
    finalize_state(&state, &mut deps, settled_remaining)?;

    let mut attrs = open_interest_attributes("liquidate_open_interest", &state.open_interest);
    attrs.push(attr("lender", state.lender.as_str()));
    attrs.push(attr("liquidator", info.sender.as_str()));
    push_nonzero_attr(&mut attrs, "requested_amount", remaining);
    push_nonzero_attr(&mut attrs, "available_balance", available);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contract::open_interest::test_helpers::{
            build_open_interest, sample_coin, setup_active_open_interest,
        },
        state::{LENDER, OPEN_INTEREST, OPEN_INTEREST_EXPIRY, OUTSTANDING_DEBT},
        ContractError,
    };
    use cosmwasm_std::{
        attr, coins,
        testing::{message_info, mock_dependencies, mock_env},
        BankMsg, Coin, CosmosMsg, Timestamp, Uint128,
    };

    fn new_open_interest(collateral: &str) -> crate::types::OpenInterest {
        build_open_interest(
            sample_coin(5, "uluna"),
            sample_coin(2, "uinterest"),
            86_400,
            sample_coin(10, collateral),
        )
    }

    #[test]
    fn liquidate_rejects_non_owner_or_lender() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let lender = deps.api.addr_make("lender");
        let open_interest = new_open_interest("uatom");
        setup_active_open_interest(deps.as_mut().storage, &owner, &lender, &open_interest);

        let intruder = deps.api.addr_make("intruder");
        let err = liquidate(deps.as_mut(), mock_env(), message_info(&intruder, &[])).unwrap_err();

        assert!(matches!(err, ContractError::Unauthorized {}));
    }

    #[test]
    fn liquidate_rejects_before_expiry() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let lender = deps.api.addr_make("lender");
        let open_interest = new_open_interest("uatom");
        setup_active_open_interest(deps.as_mut().storage, &owner, &lender, &open_interest);

        OPEN_INTEREST_EXPIRY
            .save(deps.as_mut().storage, &Some(Timestamp::from_seconds(1_000)))
            .expect("expiry stored");

        let mut env = mock_env();
        env.block.time = Timestamp::from_seconds(0);
        let err = liquidate(deps.as_mut(), env, message_info(&owner, &[])).unwrap_err();

        assert!(
            matches!(err, ContractError::OpenInterestNotExpired {}),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn liquidate_pays_lender_and_clears_state() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let lender = deps.api.addr_make("lender");
        let bonded_denom = deps.as_ref().querier.query_bonded_denom().unwrap();
        let collateral_denom = if bonded_denom == "uusd" {
            "ujuno"
        } else {
            "uusd"
        };
        let open_interest = new_open_interest(collateral_denom);
        setup_active_open_interest(deps.as_mut().storage, &owner, &lender, &open_interest);

        let env = mock_env();
        deps.querier
            .bank
            .update_balance(env.contract.address.as_str(), coins(25, collateral_denom));

        let amount_u128 = 25u128;
        let amount = Uint128::from(amount_u128);
        OUTSTANDING_DEBT
            .save(
                deps.as_mut().storage,
                &Some(Coin::new(amount_u128, collateral_denom.to_string())),
            )
            .expect("debt stored");

        let response =
            liquidate(deps.as_mut(), env.clone(), message_info(&owner, &[])).expect("liquidate");

        assert!(OPEN_INTEREST.load(deps.as_ref().storage).unwrap().is_none());
        assert!(OPEN_INTEREST_EXPIRY
            .load(deps.as_ref().storage)
            .unwrap()
            .is_none());
        assert!(LENDER.load(deps.as_ref().storage).unwrap().is_none());
        assert!(OUTSTANDING_DEBT
            .load(deps.as_ref().storage)
            .unwrap()
            .is_none());

        assert!(response
            .attributes
            .contains(&attr("action", "liquidate_open_interest")));
        assert!(response
            .attributes
            .contains(&attr("payout_amount", amount.to_string())));

        assert_eq!(response.messages.len(), 1);
        match &response.messages[0].msg {
            CosmosMsg::Bank(BankMsg::Send {
                to_address,
                amount: msg_amount,
            }) => {
                assert_eq!(to_address, lender.as_str());
                assert_eq!(
                    msg_amount.as_slice(),
                    &[Coin::new(amount_u128, collateral_denom)]
                );
            }
            msg => panic!("unexpected message: {msg:?}"),
        }
    }

    #[test]
    fn liquidate_rejects_insufficient_balance() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let lender = deps.api.addr_make("lender");
        let bonded_denom = deps.as_ref().querier.query_bonded_denom().unwrap();
        let collateral_denom = if bonded_denom == "uusd" {
            "ujuno"
        } else {
            "uusd"
        };
        let open_interest = new_open_interest(collateral_denom);
        setup_active_open_interest(deps.as_mut().storage, &owner, &lender, &open_interest);

        let amount_u128 = 20u128;
        let amount = Uint128::from(amount_u128);
        OUTSTANDING_DEBT
            .save(
                deps.as_mut().storage,
                &Some(Coin::new(amount_u128, collateral_denom.to_string())),
            )
            .expect("debt stored");

        let err = liquidate(deps.as_mut(), mock_env(), message_info(&owner, &[])).unwrap_err();

        assert!(matches!(
            err,
            ContractError::InsufficientBalance {
                denom,
                available,
                requested,
            } if denom == collateral_denom
                && available.is_zero()
                && requested == amount
        ));
    }
}

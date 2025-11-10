use cosmwasm_std::{attr, BankMsg, DepsMut, Env, MessageInfo, Response};

use crate::{
    error::ContractError,
    state::{COUNTER_OFFERS, OPEN_INTEREST},
};

use super::helpers::release_outstanding_debt;

pub fn cancel(deps: DepsMut, _env: Env, info: MessageInfo) -> Result<Response, ContractError> {
    OPEN_INTEREST
        .load(deps.storage)?
        .ok_or(ContractError::NoOpenInterest {})?;

    let proposer = info.sender.clone();
    let stored_offer = COUNTER_OFFERS
        .may_load(deps.storage, &proposer)?
        .ok_or_else(|| ContractError::CounterOfferNotFound {
            proposer: proposer.to_string(),
        })?;

    release_outstanding_debt(deps.storage, &stored_offer.liquidity_coin)?;
    COUNTER_OFFERS.remove(deps.storage, &proposer);

    let response = Response::new()
        .add_attributes([
            attr("action", "cancel_counter_offer"),
            attr("proposer", proposer.as_str()),
            attr(
                "liquidity_amount",
                stored_offer.liquidity_coin.amount.to_string(),
            ),
        ])
        .add_message(BankMsg::Send {
            to_address: proposer.to_string(),
            amount: vec![stored_offer.liquidity_coin.clone()],
        });

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::counter_offer::propose;
    use crate::contract::counter_offer::test_helpers::setup_open_interest;
    use crate::error::ContractError;
    use crate::state::{COUNTER_OFFERS, OPEN_INTEREST, OUTSTANDING_DEBT};
    use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env};
    use cosmwasm_std::{attr, BankMsg, CosmosMsg, Uint256};

    #[test]
    fn proposer_can_cancel_counter_offer() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let active = setup_open_interest(deps.as_mut(), &owner);

        let proposer = deps.api.addr_make("proposer");
        let mut offer = active.clone();
        offer.liquidity_coin.amount = offer
            .liquidity_coin
            .amount
            .checked_sub(Uint256::from(25u128))
            .expect("amount stays positive");

        propose(
            deps.as_mut(),
            mock_env(),
            message_info(&proposer, &[offer.liquidity_coin.clone()]),
            offer.clone(),
        )
        .expect("proposal stored");

        let response = cancel(deps.as_mut(), mock_env(), message_info(&proposer, &[]))
            .expect("cancel succeeds");

        let attributes = response.attributes;
        let messages = response.messages;

        assert_eq!(
            attributes,
            vec![
                attr("action", "cancel_counter_offer"),
                attr("proposer", proposer.as_str()),
                attr("liquidity_amount", offer.liquidity_coin.amount.to_string()),
            ]
        );
        assert_eq!(messages.len(), 1);

        let msg = &messages[0].msg;
        match msg {
            CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                assert_eq!(to_address, proposer.as_str());
                assert_eq!(amount, &vec![offer.liquidity_coin.clone()]);
            }
            other => panic!("unexpected message: {:?}", other),
        }

        let stored = COUNTER_OFFERS
            .may_load(deps.as_ref().storage, &proposer)
            .expect("load succeeds");
        assert!(stored.is_none());

        let debt = OUTSTANDING_DEBT
            .load(deps.as_ref().storage)
            .expect("load succeeds");
        assert!(debt.is_none());
    }

    #[test]
    fn cancel_rejects_missing_offer() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_open_interest(deps.as_mut(), &owner);

        let missing = deps.api.addr_make("missing");
        let err = cancel(deps.as_mut(), mock_env(), message_info(&missing, &[])).unwrap_err();

        match err {
            ContractError::CounterOfferNotFound { proposer } => {
                assert_eq!(proposer, missing.to_string());
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn cancel_rejects_without_open_interest() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let active = setup_open_interest(deps.as_mut(), &owner);

        let proposer = deps.api.addr_make("proposer");
        let mut offer = active.clone();
        offer.liquidity_coin.amount = offer
            .liquidity_coin
            .amount
            .checked_sub(Uint256::from(5u128))
            .expect("amount stays positive");

        propose(
            deps.as_mut(),
            mock_env(),
            message_info(&proposer, &[offer.liquidity_coin.clone()]),
            offer.clone(),
        )
        .expect("proposal stored");

        OPEN_INTEREST
            .save(deps.as_mut().storage, &None)
            .expect("cleared open interest");

        let err = cancel(deps.as_mut(), mock_env(), message_info(&proposer, &[])).unwrap_err();

        assert!(matches!(err, ContractError::NoOpenInterest {}));
    }
}

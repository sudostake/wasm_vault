use cosmwasm_std::{attr, DepsMut, MessageInfo, Response};

use crate::{
    state::{LENDER, OPEN_INTEREST, OWNER},
    ContractError,
};

use super::helpers::refund_counter_offer_escrow;

pub fn close(deps: DepsMut, info: MessageInfo) -> Result<Response, ContractError> {
    let owner = OWNER.load(deps.storage)?;
    if info.sender != owner {
        return Err(ContractError::Unauthorized {});
    }

    if LENDER.load(deps.storage)?.is_some() {
        return Err(ContractError::LenderAlreadySet {});
    }

    let open_interest = OPEN_INTEREST
        .load(deps.storage)?
        .ok_or(ContractError::NoOpenInterest {})?;

    OPEN_INTEREST.save(deps.storage, &None)?;
    let refund_msgs = refund_counter_offer_escrow(deps.storage)?;

    let response = Response::new().add_attributes([
        attr("action", "close_open_interest"),
        attr(
            "liquidity_denom",
            open_interest.liquidity_coin.denom.clone(),
        ),
        attr(
            "liquidity_amount",
            open_interest.liquidity_coin.amount.to_string(),
        ),
        attr("interest_denom", open_interest.interest_coin.denom.clone()),
        attr(
            "interest_amount",
            open_interest.interest_coin.amount.to_string(),
        ),
        attr("collateral_denom", open_interest.collateral.denom.clone()),
        attr(
            "collateral_amount",
            open_interest.collateral.amount.to_string(),
        ),
    ]);

    Ok(response.add_messages(refund_msgs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contract::open_interest::{
            execute,
            test_helpers::{build_open_interest, sample_coin, setup},
        },
        state::{COUNTER_OFFERS, LENDER, OPEN_INTEREST, OUTSTANDING_DEBT},
        ContractError,
    };
    use cosmwasm_std::{
        attr,
        testing::{message_info, mock_dependencies, mock_env},
        BankMsg, Coin, Order,
    };

    #[test]
    fn close_rejects_non_owner_senders() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut().storage, &owner);
        let intruder = deps.api.addr_make("intruder");

        let err = close(deps.as_mut(), message_info(&intruder, &[])).unwrap_err();

        assert!(matches!(err, ContractError::Unauthorized {}));
    }

    #[test]
    fn close_requires_active_open_interest() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut().storage, &owner);

        let err = close(deps.as_mut(), message_info(&owner, &[])).unwrap_err();

        assert!(matches!(err, ContractError::NoOpenInterest {}));
    }

    #[test]
    fn close_rejects_when_lender_present() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut().storage, &owner);

        let request = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );

        OPEN_INTEREST
            .save(deps.as_mut().storage, &Some(request.clone()))
            .expect("open interest stored");
        let lender = deps.api.addr_make("lender");
        LENDER
            .save(deps.as_mut().storage, &Some(lender))
            .expect("lender stored");

        let err = close(deps.as_mut(), message_info(&owner, &[])).unwrap_err();

        assert!(matches!(err, ContractError::LenderAlreadySet {}));
    }

    #[test]
    fn close_clears_open_interest_and_emits_attributes() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut().storage, &owner);

        let request = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );

        OPEN_INTEREST
            .save(deps.as_mut().storage, &Some(request.clone()))
            .expect("open interest stored");

        let response = close(deps.as_mut(), message_info(&owner, &[])).expect("close succeeds");

        assert!(response.messages.is_empty());
        assert_eq!(
            response.attributes[0],
            attr("action", "close_open_interest")
        );

        let stored = OPEN_INTEREST
            .load(deps.as_ref().storage)
            .expect("open interest fetched");

        assert!(stored.is_none());
    }

    #[test]
    fn close_clears_counter_offers() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut().storage, &owner);

        let request = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );

        OPEN_INTEREST
            .save(deps.as_mut().storage, &Some(request.clone()))
            .expect("open interest stored");

        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &Some(request.liquidity_coin.clone()))
            .expect("debt stored");

        let proposer = deps.api.addr_make("proposer");
        COUNTER_OFFERS
            .save(deps.as_mut().storage, &proposer, &request)
            .expect("counter offer stored");

        let response = close(deps.as_mut(), message_info(&owner, &[])).expect("close succeeds");

        assert_eq!(response.messages.len(), 1);
        let message = &response.messages[0];
        match &message.msg {
            cosmwasm_std::CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                assert_eq!(to_address, proposer.as_str());
                assert_eq!(amount.as_slice(), &[request.liquidity_coin.clone()]);
            }
            msg => panic!("unexpected refund message: {msg:?}"),
        }

        let mut offers = COUNTER_OFFERS.range(deps.as_ref().storage, None, None, Order::Ascending);
        assert!(offers.next().is_none());

        let debt = OUTSTANDING_DEBT
            .load(deps.as_ref().storage)
            .expect("debt queried");
        assert!(debt.is_none());
    }

    #[test]
    fn owner_can_reopen_interest_after_closing_offers() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut().storage, &owner);

        let initial_request = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );

        execute(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            initial_request.clone(),
        )
        .expect("initial open interest succeeds");

        let proposer = deps.api.addr_make("stale");
        let offer = build_open_interest(
            sample_coin(90, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );
        COUNTER_OFFERS
            .save(deps.as_mut().storage, &proposer, &offer)
            .expect("counter offer stored");
        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &Some(offer.liquidity_coin.clone()))
            .expect("debt stored");

        close(deps.as_mut(), message_info(&owner, &[])).expect("close succeeds");

        let reopened_request = build_open_interest(
            sample_coin(200, "uusd"),
            sample_coin(10, "ujuno"),
            172_800,
            sample_coin(300, "uatom"),
        );

        let response = execute(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            reopened_request.clone(),
        )
        .expect("reopen succeeds");

        assert!(response.messages.is_empty());

        let stored = OPEN_INTEREST
            .load(deps.as_ref().storage)
            .expect("open interest fetched");
        assert_eq!(stored, Some(reopened_request));

        let debt = OUTSTANDING_DEBT
            .load(deps.as_ref().storage)
            .expect("debt fetched");
        assert!(debt.is_none());

        let mut offers = COUNTER_OFFERS.range(deps.as_ref().storage, None, None, Order::Ascending);
        assert!(offers.next().is_none());
    }

    #[test]
    fn close_refunds_multiple_offers_and_clears_debt() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut().storage, &owner);

        let request = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );

        OPEN_INTEREST
            .save(deps.as_mut().storage, &Some(request.clone()))
            .expect("open interest stored");

        let proposer_a = deps.api.addr_make("proposer-a");
        let proposer_b = deps.api.addr_make("proposer-b");

        let offer_a = build_open_interest(
            sample_coin(90, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );
        let offer_b = build_open_interest(
            sample_coin(80, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );

        COUNTER_OFFERS
            .save(deps.as_mut().storage, &proposer_a, &offer_a)
            .expect("offer A stored");
        COUNTER_OFFERS
            .save(deps.as_mut().storage, &proposer_b, &offer_b)
            .expect("offer B stored");

        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &Some(Coin::new(170u128, "uusd")))
            .expect("debt stored");

        let response = close(deps.as_mut(), message_info(&owner, &[])).expect("close succeeds");

        assert_eq!(response.messages.len(), 2);
        let mut recipients = response
            .messages
            .iter()
            .map(|msg| match &msg.msg {
                cosmwasm_std::CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                    (to_address.as_str(), amount.as_slice())
                }
                msg => panic!("unexpected message: {msg:?}"),
            })
            .collect::<Vec<_>>();

        recipients.sort_by(|a, b| a.0.cmp(b.0));

        let mut expected = vec![
            (
                proposer_a.as_str(),
                std::slice::from_ref(&offer_a.liquidity_coin),
            ),
            (
                proposer_b.as_str(),
                std::slice::from_ref(&offer_b.liquidity_coin),
            ),
        ];
        expected.sort_by(|a, b| a.0.cmp(b.0));

        assert_eq!(recipients, expected);

        let debt = OUTSTANDING_DEBT
            .load(deps.as_ref().storage)
            .expect("debt queried");
        assert!(debt.is_none());
    }
}

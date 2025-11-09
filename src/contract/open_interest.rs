use cosmwasm_std::{attr, Coin, DepsMut, Env, MessageInfo, Response};

use crate::{
    state::{COUNTER_OFFERS, LENDER, OPEN_INTEREST, OWNER},
    types::OpenInterest,
    ContractError,
};

pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    open_interest: OpenInterest,
) -> Result<Response, ContractError> {
    let owner = OWNER.load(deps.storage)?;
    if info.sender != owner {
        return Err(ContractError::Unauthorized {});
    }

    if OPEN_INTEREST.load(deps.storage)?.is_some() {
        return Err(ContractError::OpenInterestAlreadyExists {});
    }

    validate_open_interest(&open_interest)?;

    OPEN_INTEREST.save(deps.storage, &Some(open_interest.clone()))?;
    COUNTER_OFFERS.clear(deps.storage);

    Ok(Response::new().add_attributes([
        attr("action", "open_interest"),
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
        attr("expiry_duration", open_interest.expiry_duration.to_string()),
    ]))
}

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
    COUNTER_OFFERS.clear(deps.storage);

    Ok(Response::new().add_attributes([
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
    ]))
}

fn validate_open_interest(open_interest: &OpenInterest) -> Result<(), ContractError> {
    validate_coin(&open_interest.liquidity_coin, "liquidity_coin")?;
    validate_coin(&open_interest.interest_coin, "interest_coin")?;
    validate_coin(&open_interest.collateral, "collateral")?;

    if open_interest.expiry_duration == 0 {
        return Err(ContractError::InvalidExpiryDuration {});
    }

    Ok(())
}

fn validate_coin(coin: &Coin, field: &'static str) -> Result<(), ContractError> {
    if coin.amount.is_zero() {
        return Err(ContractError::InvalidCoinAmount { field });
    }

    if coin.denom.is_empty() {
        return Err(ContractError::InvalidCoinDenom { field });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::{
        attr,
        testing::{message_info, mock_dependencies, mock_env},
        Addr, Order,
    };

    fn setup(deps: DepsMut, owner: &Addr) {
        OWNER.save(deps.storage, owner).expect("owner stored");
        LENDER.save(deps.storage, &None).expect("lender cleared");
        OPEN_INTEREST
            .save(deps.storage, &None)
            .expect("open interest cleared");
    }

    fn sample_coin(amount: u128, denom: &str) -> Coin {
        Coin::new(amount, denom)
    }

    fn build_open_interest(
        liquidity_coin: Coin,
        interest_coin: Coin,
        expiry_duration: u64,
        collateral: Coin,
    ) -> OpenInterest {
        OpenInterest {
            liquidity_coin,
            interest_coin,
            expiry_duration,
            collateral,
        }
    }

    #[test]
    fn rejects_non_owner_senders() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut(), &owner);
        let intruder = deps.api.addr_make("intruder");
        let request = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );

        let response = execute(
            deps.as_mut(),
            mock_env(),
            message_info(&intruder, &[]),
            request,
        );

        assert!(matches!(response, Err(ContractError::Unauthorized {})));
    }

    #[test]
    fn rejects_when_interest_already_open() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut(), &owner);

        let request = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );

        OPEN_INTEREST
            .save(deps.as_mut().storage, &Some(request.clone()))
            .expect("interest stored");

        let err = execute(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            request,
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::OpenInterestAlreadyExists {}));
    }

    #[test]
    fn rejects_zero_coin_amounts() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut(), &owner);
        let request = build_open_interest(
            Coin::new(0u128, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );

        let err = execute(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            request,
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::InvalidCoinAmount {
                field: "liquidity_coin"
            }
        ));
    }

    #[test]
    fn rejects_empty_denoms() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut(), &owner);
        let request = build_open_interest(
            sample_coin(100, ""),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );

        let err = execute(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            request,
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ContractError::InvalidCoinDenom {
                field: "liquidity_coin"
            }
        ));
    }

    #[test]
    fn rejects_zero_expiry_duration() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut(), &owner);
        let request = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            0,
            sample_coin(200, "uatom"),
        );

        let err = execute(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            request,
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::InvalidExpiryDuration {}));
    }

    #[test]
    fn stores_open_interest_when_inputs_valid() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut(), &owner);
        let request = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );

        let response = execute(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            request.clone(),
        )
        .expect("open interest succeeds");

        assert!(response.messages.is_empty());
        assert_eq!(response.attributes.len(), 8);

        let stored = OPEN_INTEREST
            .load(deps.as_ref().storage)
            .expect("interest fetched");

        assert_eq!(stored, Some(request));
    }

    #[test]
    fn close_rejects_non_owner_senders() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut(), &owner);
        let intruder = deps.api.addr_make("intruder");

        let err = close(deps.as_mut(), message_info(&intruder, &[])).unwrap_err();

        assert!(matches!(err, ContractError::Unauthorized {}));
    }

    #[test]
    fn close_requires_active_open_interest() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut(), &owner);

        let err = close(deps.as_mut(), message_info(&owner, &[])).unwrap_err();

        assert!(matches!(err, ContractError::NoOpenInterest {}));
    }

    #[test]
    fn close_rejects_when_lender_present() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut(), &owner);

        let request = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );

        OPEN_INTEREST
            .save(deps.as_mut().storage, &Some(request))
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
        setup(deps.as_mut(), &owner);

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
        setup(deps.as_mut(), &owner);

        let request = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );

        OPEN_INTEREST
            .save(deps.as_mut().storage, &Some(request.clone()))
            .expect("open interest stored");

        let proposer = deps.api.addr_make("proposer");
        COUNTER_OFFERS
            .save(deps.as_mut().storage, &proposer, &request)
            .expect("counter offer stored");

        close(deps.as_mut(), message_info(&owner, &[])).expect("close succeeds");

        let mut offers = COUNTER_OFFERS.range(deps.as_ref().storage, None, None, Order::Ascending);
        assert!(offers.next().is_none());
    }

    #[test]
    fn opening_interest_clears_stale_counter_offers() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut(), &owner);

        let stale_offer = build_open_interest(
            sample_coin(50, "uusd"),
            sample_coin(5, "ujuno"),
            10,
            sample_coin(20, "uatom"),
        );
        let proposer = deps.api.addr_make("stale");
        COUNTER_OFFERS
            .save(deps.as_mut().storage, &proposer, &stale_offer)
            .expect("stale offer stored");

        let request = build_open_interest(
            sample_coin(100, "uusd"),
            sample_coin(5, "ujuno"),
            86_400,
            sample_coin(200, "uatom"),
        );

        execute(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            request,
        )
        .expect("open interest succeeds");

        let mut offers = COUNTER_OFFERS.range(deps.as_ref().storage, None, None, Order::Ascending);
        assert!(offers.next().is_none());
    }
}

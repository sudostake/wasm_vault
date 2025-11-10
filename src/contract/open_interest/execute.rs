use cosmwasm_std::{attr, DepsMut, Env, MessageInfo, Response};

use crate::{
    state::{COUNTER_OFFERS, OPEN_INTEREST, OWNER},
    types::OpenInterest,
    ContractError,
};

use super::helpers::validate_open_interest;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contract::open_interest::test_helpers::{build_open_interest, sample_coin, setup},
        state::OPEN_INTEREST,
        ContractError,
    };
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        Coin, Uint128, Uint256,
    };

    #[test]
    fn rejects_non_owner_senders() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut().storage, &owner);
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
        setup(deps.as_mut().storage, &owner);

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
        setup(deps.as_mut().storage, &owner);
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
        setup(deps.as_mut().storage, &owner);
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
        setup(deps.as_mut().storage, &owner);
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
    fn rejects_repayment_overflow() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut().storage, &owner);
        let request = build_open_interest(
            Coin::new(Uint128::MAX.u128(), "uusd"),
            Coin::new(1u128, "uusd"),
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

        let expected = Uint256::from(Uint128::MAX) + Uint256::from(1u128);

        assert!(matches!(
            err,
            ContractError::RepaymentAmountOverflow { denom, requested }
                if denom == "uusd" && requested == expected
        ));
    }

    #[test]
    fn stores_open_interest_when_inputs_valid() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup(deps.as_mut().storage, &owner);
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
}

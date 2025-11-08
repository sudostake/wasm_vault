#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{to_json_binary, Deps, Env, QueryResponse, StdResult};

use crate::msg::QueryMsg;
use crate::state::{LENDER, OPEN_INTEREST, OWNER};
use crate::types::InfoResponse;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<QueryResponse> {
    match msg {
        QueryMsg::Info => query_info(deps),
    }
}

fn query_info(deps: Deps) -> StdResult<QueryResponse> {
    let owner = OWNER.load(deps.storage)?;
    let lender = LENDER.load(deps.storage)?;
    let open_interest = OPEN_INTEREST.load(deps.storage)?;

    let response = InfoResponse {
        message: "wasm_vault".to_string(),
        owner: owner.into_string(),
        lender: lender.map(|addr| addr.into_string()),
        open_interest,
    };

    to_json_binary(&response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::OpenInterest;
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        Coin,
    };

    #[test]
    fn query_info_returns_owner_and_lender() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let lender = deps.api.addr_make("lender");

        OWNER
            .save(deps.as_mut().storage, &owner)
            .expect("owner saved");
        LENDER
            .save(deps.as_mut().storage, &Some(lender.clone()))
            .expect("lender saved");

        let open_interest = OpenInterest {
            liquidity_coin: Coin::new(100u128, "uusd"),
            interest_coin: Coin::new(5u128, "uusd"),
            expiry_duration: 86_400u64,
            collateral: Coin::new(200u128, "ujuno"),
        };

        OPEN_INTEREST
            .save(deps.as_mut().storage, &Some(open_interest.clone()))
            .expect("open interest saved");

        let response = query(deps.as_ref(), mock_env(), QueryMsg::Info).expect("query succeeds");

        let info: InfoResponse = cosmwasm_std::from_json(response).expect("valid json");

        assert_eq!(info.message, "wasm_vault");
        assert_eq!(info.owner, owner.into_string());
        assert_eq!(info.lender, Some(lender.into_string()));
        assert_eq!(info.open_interest, Some(open_interest));
    }

    #[test]
    fn query_info_allows_absent_lender() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");

        OWNER
            .save(deps.as_mut().storage, &owner)
            .expect("owner saved");
        LENDER
            .save(deps.as_mut().storage, &None)
            .expect("lender defaults to none");

        OPEN_INTEREST
            .save(deps.as_mut().storage, &None)
            .expect("open interest defaults to none");

        let response = query(deps.as_ref(), mock_env(), QueryMsg::Info).expect("query succeeds");

        let info: InfoResponse = cosmwasm_std::from_json(response).expect("valid json");

        assert_eq!(info.message, "wasm_vault");
        assert_eq!(info.owner, owner.into_string());
        assert_eq!(info.lender, None);
        assert_eq!(info.open_interest, None);
    }

    #[test]
    fn query_info_fails_without_owner() {
        let deps = mock_dependencies();

        let err = query(deps.as_ref(), mock_env(), QueryMsg::Info).unwrap_err();

        assert!(
            err.to_string().contains("not found"),
            "unexpected error type: {err}"
        );
    }
}

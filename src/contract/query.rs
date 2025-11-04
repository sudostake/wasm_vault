#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{Deps, Env, QueryResponse, StdResult};

use crate::msg::QueryMsg;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(_deps: Deps, _env: Env, _msg: QueryMsg) -> StdResult<QueryResponse> {
    Ok(QueryResponse::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{mock_dependencies, mock_env};

    #[test]
    fn query_returns_default_response() {
        let deps = mock_dependencies();

        let response = query(deps.as_ref(), mock_env(), QueryMsg::Info).unwrap();

        assert_eq!(response, QueryResponse::default());
    }
}

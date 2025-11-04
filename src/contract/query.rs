#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{to_json_binary, Deps, Env, QueryResponse, StdResult};

use crate::msg::QueryMsg;
use crate::state::OWNER;
use crate::types::InfoResponse;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<QueryResponse> {
    match msg {
        QueryMsg::Info => query_info(deps),
    }
}

fn query_info(deps: Deps) -> StdResult<QueryResponse> {
    let owner = OWNER.load(deps.storage)?;

    let response = InfoResponse {
        message: "wasm_vault".to_string(),
        owner: owner.into_string(),
    };

    to_json_binary(&response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{mock_dependencies, mock_env};

    #[test]
    fn query_info_returns_owner() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");

        OWNER
            .save(deps.as_mut().storage, &owner)
            .expect("owner saved");

        let response = query(deps.as_ref(), mock_env(), QueryMsg::Info)
            .expect("query succeeds");

        let info: InfoResponse = cosmwasm_std::from_json(response).expect("valid json");

        assert_eq!(info.message, "wasm_vault");
        assert_eq!(info.owner, owner.into_string());
    }

    #[test]
    fn query_info_fails_without_owner() {
        let deps = mock_dependencies();

        let err = query(deps.as_ref(), mock_env(), QueryMsg::Info).unwrap_err();

        match err {
            cosmwasm_std::StdError::NotFound { .. } => {}
            _ => panic!("unexpected error type"),
        }
    }
}

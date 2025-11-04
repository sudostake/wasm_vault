#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{DepsMut, Env, MessageInfo, Response};

use crate::error::ContractError;
use crate::msg::ExecuteMsg;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    _deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    _msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    Ok(Response::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env};

    #[test]
    fn execute_returns_empty_response() {
        let mut deps = mock_dependencies();
        let caller = deps.api.addr_make("caller");
        let info = message_info(&caller, &[]);

        let response = execute(
            deps.as_mut(),
            mock_env(),
            info,
            ExecuteMsg::Noop {},
        )
        .expect("execute succeeds");

        assert!(response.messages.is_empty());
        assert!(response.attributes.is_empty());
    }
}

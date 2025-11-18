#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{DepsMut, Env, MessageInfo, Response};
use cw2::set_contract_version;

use crate::contract::open_interest::clear_active_lender;
use crate::error::ContractError;
use crate::msg::InstantiateMsg;
use crate::state::{
    DEFAULT_LIQUIDATION_UNBONDING_SECONDS, LAST_LIQUIDATION_UNBONDING,
    LIQUIDATION_UNBONDING_DURATION, OPEN_INTEREST, OUTSTANDING_DEBT, OWNER,
};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:wasm_vault";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let owner = match msg.owner {
        Some(owner) => deps.api.addr_validate(&owner)?,
        None => info.sender.clone(),
    };
    OWNER.save(deps.storage, &owner)?;
    OUTSTANDING_DEBT.save(deps.storage, &None)?;
    OPEN_INTEREST.save(deps.storage, &None)?;
    clear_active_lender(deps.storage)?;
    let duration = msg
        .liquidation_unbonding_duration
        .unwrap_or(DEFAULT_LIQUIDATION_UNBONDING_SECONDS);
    LIQUIDATION_UNBONDING_DURATION.save(deps.storage, &duration)?;
    LAST_LIQUIDATION_UNBONDING.save(deps.storage, &None)?;

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("owner", owner))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{
        COUNTER_OFFERS, DEFAULT_LIQUIDATION_UNBONDING_SECONDS, LENDER,
        LIQUIDATION_UNBONDING_DURATION,
    };
    use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env};

    #[test]
    fn instantiate_respects_explicit_owner() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let sender = deps.api.addr_make("sender");

        let msg = InstantiateMsg {
            owner: Some(owner.to_string()),
            liquidation_unbonding_duration: None,
        };
        let info = message_info(&sender, &[]);

        let response = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

        assert_eq!(response.messages.len(), 0);
        assert_eq!(2, response.attributes.len());
        assert_eq!(response.attributes[0].key, "method");
        assert_eq!(response.attributes[0].value, "instantiate");
        assert_eq!(response.attributes[1].key, "owner");
        assert_eq!(response.attributes[1].value, owner.as_str());

        let saved_owner = OWNER.load(&deps.storage).unwrap();
        assert_eq!(saved_owner, owner);

        let saved_lender = LENDER.load(&deps.storage).unwrap();
        assert_eq!(saved_lender, None);

        let debt = OUTSTANDING_DEBT.load(&deps.storage).unwrap();
        assert_eq!(debt, None);

        let stored_open_interest = OPEN_INTEREST.load(&deps.storage).unwrap();
        assert_eq!(stored_open_interest, None);

        let stored_duration = LIQUIDATION_UNBONDING_DURATION
            .load(deps.as_ref().storage)
            .expect("duration stored");
        assert_eq!(stored_duration, DEFAULT_LIQUIDATION_UNBONDING_SECONDS);

        let mut offers =
            COUNTER_OFFERS.range(&deps.storage, None, None, cosmwasm_std::Order::Ascending);
        assert!(offers.next().is_none());
    }

    #[test]
    fn instantiate_defaults_to_sender() {
        let mut deps = mock_dependencies();
        let sender = deps.api.addr_make("creator");

        let msg = InstantiateMsg {
            owner: None,
            liquidation_unbonding_duration: None,
        };
        let info = message_info(&sender, &[]);

        instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

        let saved_owner = OWNER.load(&deps.storage).unwrap();
        assert_eq!(saved_owner, sender);

        let saved_lender = LENDER.load(&deps.storage).unwrap();
        assert_eq!(saved_lender, None);

        let debt = OUTSTANDING_DEBT.load(&deps.storage).unwrap();
        assert_eq!(debt, None);

        let stored_open_interest = OPEN_INTEREST.load(&deps.storage).unwrap();
        assert_eq!(stored_open_interest, None);

        let mut offers =
            COUNTER_OFFERS.range(&deps.storage, None, None, cosmwasm_std::Order::Ascending);
        assert!(offers.next().is_none());
    }

    #[test]
    fn instantiate_can_override_unbonding_duration() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let sender = deps.api.addr_make("sender");

        let msg = InstantiateMsg {
            owner: Some(owner.to_string()),
            liquidation_unbonding_duration: Some(3_600),
        };
        let info = message_info(&sender, &[]);

        instantiate(deps.as_mut(), mock_env(), info, msg).expect("instantiate succeeds");

        let stored_duration = LIQUIDATION_UNBONDING_DURATION
            .load(deps.as_ref().storage)
            .expect("duration stored");
        assert_eq!(stored_duration, 3_600);
    }
}

use cosmwasm_std::{attr, DepsMut, MessageInfo, Response};

use crate::{state::OWNER, ContractError};

pub fn execute(
    deps: DepsMut,
    info: MessageInfo,
    new_owner: String,
) -> Result<Response, ContractError> {
    let current_owner = OWNER.load(deps.storage)?;

    if info.sender != current_owner {
        return Err(ContractError::Unauthorized {});
    }

    let validated_new_owner = deps.api.addr_validate(&new_owner)?;

    if validated_new_owner == current_owner {
        return Err(ContractError::OwnershipUnchanged {});
    }

    OWNER.save(deps.storage, &validated_new_owner)?;

    Ok(Response::new().add_attributes([
        attr("action", "transfer_ownership"),
        attr("previous_owner", info.sender.into_string()),
        attr("new_owner", validated_new_owner.into_string()),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{message_info, mock_dependencies};

    #[test]
    fn fails_for_unauthorized_sender() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        OWNER
            .save(deps.as_mut().storage, &owner)
            .expect("owner stored");

        let intruder = deps.api.addr_make("intruder");
        let err = execute(
            deps.as_mut(),
            message_info(&intruder, &[]),
            "new_owner".to_string(),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::Unauthorized {}));
    }

    #[test]
    fn fails_for_same_owner() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        OWNER
            .save(deps.as_mut().storage, &owner)
            .expect("owner stored");

        let err = execute(
            deps.as_mut(),
            message_info(&owner, &[]),
            owner.to_string(),
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::OwnershipUnchanged {}));
    }

    #[test]
    fn updates_owner_and_emits_attributes() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        let new_owner = deps.api.addr_make("new_owner");
        OWNER
            .save(deps.as_mut().storage, &owner)
            .expect("owner stored");

        let response = execute(
            deps.as_mut(),
            message_info(&owner, &[]),
            new_owner.to_string(),
        )
        .expect("transfer succeeds");

        let saved = OWNER
            .load(deps.as_ref().storage)
            .expect("owner should be updated");
        assert_eq!(saved, new_owner);

        assert_eq!(response.attributes.len(), 3);
        assert!(response
            .attributes
            .iter()
            .any(|attr| attr.key == "action" && attr.value == "transfer_ownership"));
        assert!(response.attributes.iter().any(|attr| {
            attr.key == "previous_owner" && attr.value == owner.to_string()
        }));
        assert!(response.attributes.iter().any(|attr| {
            attr.key == "new_owner" && attr.value == new_owner.to_string()
        }));
    }
}

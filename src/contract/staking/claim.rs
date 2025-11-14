use cosmwasm_std::{DepsMut, DistributionMsg, Env, MessageInfo, Response};

use crate::{helpers::require_owner, ContractError};

pub fn execute(deps: DepsMut, env: Env, info: MessageInfo) -> Result<Response, ContractError> {
    require_owner(&deps, &info)?;

    let delegations = deps
        .querier
        .query_all_delegations(env.contract.address.clone())?;

    if delegations.is_empty() {
        return Err(ContractError::NoDelegations {});
    }

    let mut response = Response::new()
        .add_attribute("action", "claim_delegator_rewards")
        .add_attribute("validator_count", delegations.len().to_string());

    for delegation in delegations {
        response = response.add_message(DistributionMsg::WithdrawDelegatorReward {
            validator: delegation.validator,
        });
    }

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{OUTSTANDING_DEBT, OWNER};
    use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env};
    use cosmwasm_std::{Addr, Coin, Decimal, DistributionMsg, FullDelegation, Storage, Validator};

    fn setup_owner(storage: &mut dyn Storage, owner: &Addr) {
        OWNER.save(storage, owner).expect("owner stored");
    }

    #[test]
    fn fails_for_unauthorized_sender() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner(deps.as_mut().storage, &owner);

        let intruder = deps.api.addr_make("intruder");
        let err = execute(deps.as_mut(), mock_env(), message_info(&intruder, &[])).unwrap_err();

        assert!(matches!(err, ContractError::Unauthorized {}));
    }

    #[test]
    fn fails_when_no_delegations_exist() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner(deps.as_mut().storage, &owner);
        deps.querier.staking.update("ucosm", &[], &[]);

        let err = execute(deps.as_mut(), mock_env(), message_info(&owner, &[])).unwrap_err();

        assert!(matches!(err, ContractError::NoDelegations {}));
    }

    #[test]
    fn creates_withdraw_messages_for_each_validator() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner(deps.as_mut().storage, &owner);
        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &None)
            .expect("zero debt stored");

        let env = mock_env();
        let contract_addr = env.contract.address.clone();
        let validator_one = deps.api.addr_make("validator").into_string();
        let validator_two = deps.api.addr_make("validator-two").into_string();

        let delegation_one = FullDelegation::create(
            contract_addr.clone(),
            validator_one.clone(),
            Coin::new(300u128, "ucosm"),
            Coin::new(300u128, "ucosm"),
            vec![],
        );
        let delegation_two = FullDelegation::create(
            contract_addr.clone(),
            validator_two.clone(),
            Coin::new(200u128, "ucosm"),
            Coin::new(200u128, "ucosm"),
            vec![],
        );

        let validator_obj_one = Validator::create(
            validator_one.clone(),
            Decimal::percent(5),
            Decimal::percent(10),
            Decimal::percent(1),
        );
        let validator_obj_two = Validator::create(
            validator_two.clone(),
            Decimal::percent(4),
            Decimal::percent(9),
            Decimal::percent(1),
        );

        deps.querier.staking.update(
            "ucosm",
            &[validator_obj_one, validator_obj_two],
            &[delegation_one, delegation_two],
        );

        let response =
            execute(deps.as_mut(), env, message_info(&owner, &[])).expect("claim rewards succeeds");

        assert_eq!(response.messages.len(), 2);
        let mut validators: Vec<String> = response
            .messages
            .iter()
            .map(|msg| match msg.msg.clone() {
                cosmwasm_std::CosmosMsg::Distribution(
                    DistributionMsg::WithdrawDelegatorReward { validator },
                ) => validator,
                other => panic!("unexpected message: {other:?}"),
            })
            .collect();
        validators.sort();

        let mut expected = vec![validator_one, validator_two];
        expected.sort();
        assert_eq!(validators, expected);

        assert!(response
            .attributes
            .iter()
            .any(|attr| attr.key == "action" && attr.value == "claim_delegator_rewards"));
        assert!(response
            .attributes
            .iter()
            .any(|attr| attr.key == "validator_count" && attr.value == "2"));
    }
}

use cosmwasm_std::{
    attr, DepsMut, Env, GovMsg, MessageInfo, Response, VoteOption, WeightedVoteOption,
};

use crate::{
    state::OWNER,
    ContractError,
};

pub fn execute_vote(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    proposal_id: u64,
    option: VoteOption,
) -> Result<Response, ContractError> {
    let owner = OWNER.load(deps.storage)?;
    if info.sender != owner {
        return Err(ContractError::Unauthorized {});
    }

    Ok(Response::new()
        .add_message(GovMsg::Vote {
            proposal_id,
            option,
        })
        .add_attributes([
            attr("action", "vote"),
            attr("proposal_id", proposal_id.to_string()),
            attr("vote_type", "standard"),
        ]))
}

pub fn execute_weighted_vote(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    proposal_id: u64,
    options: Vec<WeightedVoteOption>,
) -> Result<Response, ContractError> {
    let owner = OWNER.load(deps.storage)?;
    if info.sender != owner {
        return Err(ContractError::Unauthorized {});
    }

    let option_count = options.len().to_string();

    Ok(Response::new()
        .add_message(GovMsg::VoteWeighted {
            proposal_id,
            options,
        })
        .add_attributes([
            attr("action", "vote"),
            attr("proposal_id", proposal_id.to_string()),
            attr("vote_type", "weighted"),
            attr("option_count", option_count),
        ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env};
    use cosmwasm_std::{Addr, Decimal, Storage};

    fn setup_owner(storage: &mut dyn Storage, owner: &Addr) {
        OWNER.save(storage, owner).expect("owner stored");
    }

    #[test]
    fn standard_vote_requires_owner() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner(deps.as_mut().storage, &owner);

        let intruder = deps.api.addr_make("intruder");
        let err = execute_vote(
            deps.as_mut(),
            mock_env(),
            message_info(&intruder, &[]),
            42,
            VoteOption::Yes,
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::Unauthorized {}));
    }

    #[test]
    fn creates_standard_vote_message() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner(deps.as_mut().storage, &owner);

        let response = execute_vote(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            7,
            VoteOption::No,
        )
        .expect("vote succeeds");

        assert_eq!(response.messages.len(), 1);
        match response.messages[0].msg.clone() {
            cosmwasm_std::CosmosMsg::Gov(GovMsg::Vote {
                proposal_id,
                option,
            }) => {
                assert_eq!(proposal_id, 7);
                assert_eq!(option, VoteOption::No);
            }
            other => panic!("unexpected message: {other:?}"),
        }
    }

    #[test]
    fn weighted_vote_requires_owner() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner(deps.as_mut().storage, &owner);

        let intruder = deps.api.addr_make("intruder");
        let err = execute_weighted_vote(
            deps.as_mut(),
            mock_env(),
            message_info(&intruder, &[]),
            12,
            vec![WeightedVoteOption {
                option: VoteOption::Yes,
                weight: Decimal::percent(100),
            }],
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::Unauthorized {}));
    }

    #[test]
    fn creates_weighted_vote_message() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("owner");
        setup_owner(deps.as_mut().storage, &owner);

        let options = vec![
            WeightedVoteOption {
                option: VoteOption::Yes,
                weight: Decimal::percent(60),
            },
            WeightedVoteOption {
                option: VoteOption::No,
                weight: Decimal::percent(40),
            },
        ];

        let response = execute_weighted_vote(
            deps.as_mut(),
            mock_env(),
            message_info(&owner, &[]),
            9,
            options.clone(),
        )
        .expect("vote succeeds");

        assert_eq!(response.messages.len(), 1);
        match response.messages[0].msg.clone() {
            cosmwasm_std::CosmosMsg::Gov(GovMsg::VoteWeighted {
                proposal_id,
                options: vote_options,
            }) => {
                assert_eq!(proposal_id, 9);
                assert_eq!(vote_options, options);
            }
            other => panic!("unexpected message: {other:?}"),
        }
    }
}

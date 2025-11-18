use cw_multi_test::Executor;

use crate::common::{mock_app, store_contract};

use wasm_vault::{
    msg::{ExecuteMsg, InstantiateMsg},
    state::OWNER,
};

#[test]
fn owner_can_transfer_ownership() {
    let mut app = mock_app();
    let code_id = store_contract(&mut app);

    let owner = app.api().addr_make("owner");
    let new_owner = app.api().addr_make("new_owner");

    let contract_addr = app
        .instantiate_contract(
            code_id,
            owner.clone(),
            &InstantiateMsg {
                owner: Some(owner.to_string()),
                liquidation_unbonding_duration: None,
            },
            &[],
            "vault",
            None,
        )
        .expect("instantiate succeeds");

    app.execute_contract(
        owner.clone(),
        contract_addr.clone(),
        &ExecuteMsg::TransferOwnership {
            new_owner: new_owner.to_string(),
        },
        &[],
    )
    .expect("transfer should succeed");

    let saved_owner = OWNER
        .query(&app.wrap(), contract_addr)
        .expect("owner must be stored");

    assert_eq!(saved_owner, new_owner);
}

#[test]
fn non_owner_cannot_transfer_ownership() {
    let mut app = mock_app();
    let code_id = store_contract(&mut app);

    let owner = app.api().addr_make("owner");
    let intruder = app.api().addr_make("intruder");
    let new_owner = app.api().addr_make("new_owner");

    let contract_addr = app
        .instantiate_contract(
            code_id,
            owner.clone(),
            &InstantiateMsg {
                owner: Some(owner.to_string()),
                liquidation_unbonding_duration: None,
            },
            &[],
            "vault",
            None,
        )
        .expect("instantiate succeeds");

    let err = app
        .execute_contract(
            intruder.clone(),
            contract_addr,
            &ExecuteMsg::TransferOwnership {
                new_owner: new_owner.to_string(),
            },
            &[],
        )
        .unwrap_err();

    assert!(err.to_string().contains("Unauthorized"));
}

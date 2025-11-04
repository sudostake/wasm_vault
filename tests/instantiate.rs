use cosmwasm_std::{to_json_binary, Addr, Event, WasmMsg};
use cw2::query_contract_info;
use cw_multi_test::{AppResponse, Executor};

mod common;

use common::{mock_app, store_contract};

use wasm_vault::msg::InstantiateMsg;
use wasm_vault::state::OWNER;

#[test]
fn instantiate_respects_explicit_owner() {
    let mut app = mock_app();
    let code_id = store_contract(&mut app);

    let sender = app.api().addr_make("creator");
    let explicit_owner = app.api().addr_make("explicit_owner");

    let instantiate_msg = InstantiateMsg {
        owner: Some(explicit_owner.to_string()),
    };

    let response = app
        .execute(
            sender.clone(),
            WasmMsg::Instantiate {
                admin: None,
                code_id,
                msg: to_json_binary(&instantiate_msg).unwrap(),
                funds: vec![],
                label: "wasm-vault".to_string(),
            }
            .into(),
        )
        .expect("instantiate should succeed");

    assert_wasm_event_contains(
        &response,
        Event::new("wasm")
            .add_attribute("method", "instantiate")
            .add_attribute("owner", explicit_owner.to_string()),
    );

    let contract_addr = contract_address_from_response(&response);

    let saved_owner = OWNER
        .query(&app.wrap(), contract_addr.clone())
        .expect("owner should be persisted");
    assert_eq!(saved_owner, explicit_owner);

    let contract_version = query_contract_info(&app.wrap(), contract_addr).unwrap();
    assert_eq!(contract_version.contract, "crates.io:wasm_vault");
    assert_eq!(contract_version.version, env!("CARGO_PKG_VERSION"));
}

#[test]
fn instantiate_defaults_to_sender() {
    let mut app = mock_app();
    let code_id = store_contract(&mut app);

    let sender = app.api().addr_make("user");

    let instantiate_msg = InstantiateMsg { owner: None };

    let response = app
        .execute(
            sender.clone(),
            WasmMsg::Instantiate {
                admin: None,
                code_id,
                msg: to_json_binary(&instantiate_msg).unwrap(),
                funds: vec![],
                label: format!("vault-owned-by-{}", sender),
            }
            .into(),
        )
        .expect("instantiate should succeed");

    assert_wasm_event_contains(
        &response,
        Event::new("wasm")
            .add_attribute("method", "instantiate")
            .add_attribute("owner", sender.to_string()),
    );

    let contract_addr = contract_address_from_response(&response);

    let saved_owner = OWNER
        .query(&app.wrap(), contract_addr)
        .expect("owner should default to sender");
    assert_eq!(saved_owner, sender);
}

fn contract_address_from_response(response: &AppResponse) -> Addr {
    response
        .events
        .iter()
        .find(|event| event.ty == "wasm")
        .and_then(|event| {
            event
                .attributes
                .iter()
                .find(|attr| attr.key == "_contract_address")
        })
        .map(|attr| Addr::unchecked(attr.value.clone()))
        .expect("contract address event attribute missing")
}

fn assert_wasm_event_contains(response: &AppResponse, expected: Event) {
    let wasm_event = response
        .events
        .iter()
        .find(|event| event.ty == expected.ty)
        .expect("wasm event missing from response");

    for attribute in expected.attributes {
        assert!(
            wasm_event.attributes.contains(&attribute),
            "missing attribute {}={}",
            attribute.key,
            attribute.value
        );
    }
}

use cosmwasm_std::{coins, Decimal, Validator};
use cw_multi_test::{AppBuilder, BasicApp, ContractWrapper, StakingInfo};

use wasm_vault::contract::{execute, instantiate, query};

pub const DENOM: &str = "ucosm";
const CREATOR_FUNDS: u128 = 1_000_000;
const USER_FUNDS: u128 = 500_000;

pub fn mock_app() -> BasicApp {
    let mut app = AppBuilder::default().build(|router, api, storage| {
        let creator = api.addr_make("creator");
        router
            .bank
            .init_balance(storage, &creator, coins(CREATOR_FUNDS, DENOM))
            .unwrap();

        let user = api.addr_make("user");
        router
            .bank
            .init_balance(storage, &user, coins(USER_FUNDS, DENOM))
            .unwrap();

        router
            .staking
            .setup(
                storage,
                StakingInfo {
                    bonded_denom: DENOM.to_string(),
                    unbonding_time: 14 * 24 * 60 * 60,
                    apr: Decimal::percent(12),
                },
            )
            .unwrap();
    });

    let block_info = app.block_info();
    app.init_modules(|router, api, storage| {
        let validator = Validator::create(
            api.addr_make("validator").into_string(),
            Decimal::percent(5),
            Decimal::percent(10),
            Decimal::percent(1),
        );

        router
            .staking
            .add_validator(api, storage, &block_info, validator)
            .unwrap();
    });

    app
}

pub fn store_contract(app: &mut BasicApp) -> u64 {
    let contract = ContractWrapper::new(execute, instantiate, query);
    app.store_code(Box::new(contract))
}

use cosmwasm_std::{
    coins,
    testing::{MockApi, MockStorage},
    Decimal, Decimal256, Empty, Validator,
};
use cw_multi_test::{
    App, AppBuilder, BankKeeper, BasicApp, ContractWrapper, DistributionKeeper, FailingModule, Gov,
    GovAcceptingModule, GovFailingModule, IbcFailingModule, StakeKeeper, StakingInfo,
    StargateFailing, WasmKeeper,
};

use wasm_vault::contract::{execute, instantiate, query};

pub const DENOM: &str = "ucosm";
const CREATOR_FUNDS: u128 = 1_000_000;
const USER_FUNDS: u128 = 500_000;

pub type VaultApp<G> = App<
    BankKeeper,
    MockApi,
    MockStorage,
    FailingModule<Empty, Empty, Empty>,
    WasmKeeper<Empty, Empty>,
    StakeKeeper,
    DistributionKeeper,
    IbcFailingModule,
    G,
    StargateFailing,
>;

pub fn mock_app() -> BasicApp {
    build_app_with_gov(GovFailingModule::new())
}

pub fn mock_app_with_gov_accepting() -> VaultApp<GovAcceptingModule> {
    build_app_with_gov(GovAcceptingModule::new())
}

pub fn mock_app_with_gov_failing() -> VaultApp<GovFailingModule> {
    build_app_with_gov(GovFailingModule::new())
}

fn build_app_with_gov<G: Gov>(gov: G) -> VaultApp<G> {
    let mut app = AppBuilder::new()
        .with_gov(gov)
        .build(|router, api, storage| {
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
                        apr: Decimal256::percent(12),
                    },
                )
                .unwrap();
        });

    let block_info = app.block_info();
    app.init_modules(|router, api, storage| {
        let validator_one = Validator::create(
            api.addr_make("validator").into_string(),
            Decimal::percent(5),
            Decimal::percent(10),
            Decimal::percent(1),
        );
        let validator_two = Validator::create(
            api.addr_make("validator-two").into_string(),
            Decimal::percent(4),
            Decimal::percent(9),
            Decimal::percent(1),
        );

        router
            .staking
            .add_validator(api, storage, &block_info, validator_one)
            .unwrap();
        router
            .staking
            .add_validator(api, storage, &block_info, validator_two)
            .unwrap();
    });

    app
}

pub fn store_contract<G: Gov>(app: &mut VaultApp<G>) -> u64 {
    let contract = ContractWrapper::new(execute, instantiate, query);
    app.store_code(Box::new(contract))
}

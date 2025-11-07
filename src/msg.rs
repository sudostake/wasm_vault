pub use crate::types::InfoResponse;
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Uint128;

#[cw_serde]
pub struct InstantiateMsg {
    pub owner: Option<String>,
}

#[cw_serde]
pub enum ExecuteMsg {
    Noop {},
    Delegate {
        validator: String,
        amount: Uint128,
    },
    Undelegate {
        validator: String,
        amount: Uint128,
    },
    Redelegate {
        src_validator: String,
        dst_validator: String,
        amount: Uint128,
    },
    Withdraw {
        denom: String,
        amount: Uint128,
        recipient: Option<String>,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(InfoResponse)]
    Info,
}

use cosmwasm_schema::cw_serde;
use cosmwasm_std::Uint128;

#[cw_serde]
pub struct InfoResponse {
    pub message: String,
    pub owner: String,
}

#[cw_serde]
pub struct OutstandingDebt {
    /// Amount of collateral locked to settle the debt in the chain's staking denom
    pub amount: Uint128,
}

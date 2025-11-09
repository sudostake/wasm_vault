use cosmwasm_schema::cw_serde;
use cosmwasm_std::Coin;

#[cw_serde]
pub struct InfoResponse {
    pub message: String,
    pub owner: String,
    pub lender: Option<String>,
    pub open_interest: Option<OpenInterest>,
    pub counter_offers: Option<Vec<CounterOffer>>,
}

#[cw_serde]
pub struct OpenInterest {
    /// Coin the borrower wants to receive as liquidity.
    pub liquidity_coin: Coin,
    /// Coin used to pay interest back to the lender.
    pub interest_coin: Coin,
    /// Time (in seconds) remaining before the position expires.
    pub expiry_duration: u64,
    /// Collateral provided to secure the open interest.
    pub collateral: Coin,
}

#[cw_serde]
pub struct CounterOffer {
    /// Address of the lender proposing a change.
    pub proposer: String,
    /// Proposed open interest terms. Only the amount should deviate.
    pub open_interest: OpenInterest,
}

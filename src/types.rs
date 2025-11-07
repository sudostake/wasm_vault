use cosmwasm_schema::cw_serde;

#[cw_serde]
pub struct InfoResponse {
    pub message: String,
    pub owner: String,
    pub lender: Option<String>,
}

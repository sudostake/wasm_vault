use cosmwasm_std::{StdError, Uint256};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Delegation amount must be greater than zero")]
    InvalidDelegationAmount {},

    #[error("Funds not accepted for delegation")]
    FundsNotAccepted {},

    #[error("Insufficient balance: have {available} {denom}, need {requested}")]
    InsufficientBalance {
        denom: String,
        available: Uint256,
        requested: Uint256,
    },

    #[error("Validator not found: {validator}")]
    ValidatorNotFound { validator: String },
}

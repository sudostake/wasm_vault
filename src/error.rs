use cosmwasm_std::{Coin, StdError, Uint256};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("New owner must be different from the current owner")]
    OwnershipUnchanged {},

    #[error("Delegation amount must be greater than zero")]
    InvalidDelegationAmount {},

    #[error("Insufficient balance: have {available} {denom}, need {requested}")]
    InsufficientBalance {
        denom: String,
        available: Uint256,
        requested: Uint256,
    },

    #[error("Outstanding debt of {amount} must be settled before delegating")]
    OutstandingDebt { amount: Coin },

    #[error("Validator not found: {validator}")]
    ValidatorNotFound { validator: String },

    #[error("Undelegation amount must be greater than zero")]
    InvalidUndelegationAmount {},

    #[error("Redelegation amount must be greater than zero")]
    InvalidRedelegationAmount {},

    #[error("Withdrawal amount must be greater than zero")]
    InvalidWithdrawalAmount {},

    #[error("Cannot redelegate to the same validator")]
    RedelegateToSameValidator {},

    #[error("Delegation not found for validator {validator}")]
    DelegationNotFound { validator: String },

    #[error("Insufficient delegated balance for validator {validator}: have {delegated}, need {requested}")]
    InsufficientDelegatedBalance {
        validator: String,
        delegated: Uint256,
        requested: Uint256,
    },

    #[error("No delegations found to claim rewards from")]
    NoDelegations {},
}

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

    #[error("An open interest is already active")]
    OpenInterestAlreadyExists {},

    #[error("No open interest is currently active")]
    NoOpenInterest {},

    #[error("A lender has already been set")]
    LenderAlreadySet {},

    #[error("{field} amount must be greater than zero")]
    InvalidCoinAmount { field: &'static str },

    #[error("{field} denom must not be empty")]
    InvalidCoinDenom { field: &'static str },

    #[error("Expiry duration must be greater than zero seconds")]
    InvalidExpiryDuration {},

    #[error("Counter offer terms must match the active open interest")]
    CounterOfferTermsMismatch {},

    #[error("Counter offer liquidity must be less than the active open interest")]
    CounterOfferNotSmaller {},

    #[error("Counter offer escrow must provide {expected} {denom}, received {received}")]
    CounterOfferEscrowMismatch {
        denom: String,
        expected: Uint256,
        received: Uint256,
    },

    #[error("Funding escrow must provide {expected} {denom}, received {received}")]
    OpenInterestFundingMismatch {
        denom: String,
        expected: Uint256,
        received: Uint256,
    },

    #[error("Fund request does not match the active open interest")]
    OpenInterestMismatch {},

    #[error("Proposer already has an active counter offer")]
    CounterOfferAlreadyExists {},

    #[error("Counter offers are full; liquidity must be greater than {minimum} {denom}")]
    CounterOfferNotCompetitive { minimum: Uint256, denom: String },

    #[error("Counter offer from {proposer} not found")]
    CounterOfferNotFound { proposer: String },

    #[error("Counter offer payload for {proposer} does not match stored terms")]
    CounterOfferMismatch { proposer: String },

    #[error("Cannot undelegate while an open interest is active")]
    UndelegateWhileOpenInterestActive {},
}

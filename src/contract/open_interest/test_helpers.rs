use cosmwasm_std::{Addr, Coin, Storage};

use crate::{
    state::{LENDER, OPEN_INTEREST, OUTSTANDING_DEBT, OWNER},
    types::OpenInterest,
};

pub fn setup(storage: &mut dyn Storage, owner: &Addr) {
    OWNER.save(storage, owner).expect("owner stored");
    LENDER.save(storage, &None).expect("lender cleared");
    OUTSTANDING_DEBT.save(storage, &None).expect("debt cleared");
    OPEN_INTEREST
        .save(storage, &None)
        .expect("open interest cleared");
}

pub fn setup_active_open_interest(
    storage: &mut dyn Storage,
    owner: &Addr,
    lender: &Addr,
    open_interest: &OpenInterest,
) {
    setup(storage, owner);
    OPEN_INTEREST
        .save(storage, &Some(open_interest.clone()))
        .expect("open interest stored");
    LENDER
        .save(storage, &Some(lender.clone()))
        .expect("lender stored");
}

pub fn sample_coin(amount: u128, denom: &str) -> Coin {
    Coin::new(amount, denom)
}

pub fn build_open_interest(
    liquidity_coin: Coin,
    interest_coin: Coin,
    expiry_duration: u64,
    collateral: Coin,
) -> OpenInterest {
    OpenInterest {
        liquidity_coin,
        interest_coin,
        expiry_duration,
        collateral,
    }
}

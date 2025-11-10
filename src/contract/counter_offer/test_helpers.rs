use cosmwasm_std::{Addr, Coin, DepsMut};

use crate::{
    state::{COUNTER_OFFERS, LENDER, OPEN_INTEREST, OUTSTANDING_DEBT, OWNER},
    types::OpenInterest,
};

pub fn setup_open_interest(deps: DepsMut, owner: &Addr) -> OpenInterest {
    let interest = OpenInterest {
        liquidity_coin: Coin::new(1_000u128, "uusd"),
        interest_coin: Coin::new(50u128, "ujuno"),
        expiry_duration: 86_400u64,
        collateral: Coin::new(2_000u128, "uatom"),
    };

    OWNER.save(deps.storage, owner).expect("owner stored");
    OUTSTANDING_DEBT
        .save(deps.storage, &None)
        .expect("debt cleared");
    LENDER.save(deps.storage, &None).expect("lender cleared");
    OPEN_INTEREST
        .save(deps.storage, &Some(interest.clone()))
        .expect("open interest stored");
    COUNTER_OFFERS
        .clear(deps.storage)
        .expect("counter offers cleared");

    interest
}

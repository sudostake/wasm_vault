use crate::types::OpenInterest;
use cosmwasm_std::{Addr, Coin, Timestamp};
use cw_storage_plus::{Item, Map};

/// Maximum number of counter offers a vault will record simultaneously.
pub const MAX_COUNTER_OFFERS: u8 = u8::MAX;

pub const OWNER: Item<Addr> = Item::new("owner");
pub const LENDER: Item<Option<Addr>> = Item::new("lender");
pub const OUTSTANDING_DEBT: Item<Option<Coin>> = Item::new("outstanding_debt");
pub const OPEN_INTEREST: Item<Option<OpenInterest>> = Item::new("open_interest");
pub const OPEN_INTEREST_EXPIRY: Item<Option<Timestamp>> = Item::new("open_interest_expiry");
pub const COUNTER_OFFERS: Map<&Addr, OpenInterest> = Map::new("counter_offers");

/// Safe default for the unstaking delay used in liquidation logic.
pub const DEFAULT_LIQUIDATION_UNBONDING_SECONDS: u64 = 21 * 24 * 60 * 60;
/// Hard cap on custom liquidation intervals (30 days in seconds).
pub const MAX_LIQUIDATION_UNBONDING_SECONDS: u64 = 30 * 24 * 60 * 60;

pub const LIQUIDATION_UNBONDING_DURATION: Item<u64> = Item::new("liquidation_unbonding_duration");
pub const LAST_LIQUIDATION_UNBONDING: Item<Option<Timestamp>> =
    Item::new("last_liquidation_unbonding");

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::{testing::mock_dependencies, Coin, Order, Timestamp};

    #[test]
    fn owner_item_persists_addresses() {
        let mut deps = mock_dependencies();
        let address = Addr::unchecked("owner");

        OWNER
            .save(deps.as_mut().storage, &address)
            .expect("save succeeds");
        let loaded = OWNER
            .load(deps.as_ref().storage)
            .expect("load retrieves saved address");

        assert_eq!(loaded, address);
    }

    #[test]
    fn lender_item_handles_optional_addresses() {
        let mut deps = mock_dependencies();
        let address = Addr::unchecked("lender");

        // Save and load a concrete lender address
        LENDER
            .save(deps.as_mut().storage, &Some(address.clone()))
            .expect("save succeeds");
        let loaded = LENDER.load(deps.as_ref().storage).expect("load succeeds");

        assert_eq!(loaded, Some(address));

        // Ensure the default state can be absent
        let fresh_deps = mock_dependencies();
        let missing = LENDER
            .may_load(fresh_deps.as_ref().storage)
            .expect("may_load succeeds");

        assert_eq!(missing, None);
    }

    #[test]
    fn outstanding_debt_item_handles_optional_coin() {
        let mut deps = mock_dependencies();
        let denom = "ucosm";
        let debt_coin = Coin::new(50u128, denom);

        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &Some(debt_coin.clone()))
            .expect("save succeeds");

        let loaded = OUTSTANDING_DEBT
            .load(deps.as_ref().storage)
            .expect("query succeeds");

        assert_eq!(loaded, Some(debt_coin));

        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &None)
            .expect("clearing debt succeeds");

        let cleared = OUTSTANDING_DEBT
            .load(deps.as_ref().storage)
            .expect("load succeeds");

        assert!(cleared.is_none());
    }

    #[test]
    fn open_interest_item_handles_optional_state() {
        let mut deps = mock_dependencies();
        let entry = OpenInterest {
            liquidity_coin: Coin::new(100u128, "uusd"),
            interest_coin: Coin::new(5u128, "uusd"),
            expiry_duration: 86_400u64,
            collateral: Coin::new(200u128, "ujuno"),
        };

        OPEN_INTEREST
            .save(deps.as_mut().storage, &Some(entry.clone()))
            .expect("save succeeds");
        let loaded = OPEN_INTEREST
            .load(deps.as_ref().storage)
            .expect("load succeeds");

        assert_eq!(loaded, Some(entry));

        let fresh_deps = mock_dependencies();
        let missing = OPEN_INTEREST
            .may_load(fresh_deps.as_ref().storage)
            .expect("may_load succeeds");

        assert!(missing.is_none());
    }

    #[test]
    fn open_interest_expiry_defaults_to_none() {
        let mut deps = mock_dependencies();
        let stored = OPEN_INTEREST_EXPIRY
            .may_load(deps.as_ref().storage)
            .expect("may_load succeeds");

        assert!(stored.is_none());

        let expiry = Timestamp::from_seconds(100);
        OPEN_INTEREST_EXPIRY
            .save(deps.as_mut().storage, &Some(expiry))
            .expect("save succeeds");

        let loaded = OPEN_INTEREST_EXPIRY
            .load(deps.as_ref().storage)
            .expect("load succeeds");
        assert_eq!(loaded, Some(expiry));

        OPEN_INTEREST_EXPIRY
            .save(deps.as_mut().storage, &None)
            .expect("cleared");
        let cleared = OPEN_INTEREST_EXPIRY
            .load(deps.as_ref().storage)
            .expect("load succeeds");
        assert!(cleared.is_none());
    }

    #[test]
    fn counter_offer_map_handles_unique_proposers() {
        let mut deps = mock_dependencies();
        let proposer_a = Addr::unchecked("lender-a");
        let proposer_b = Addr::unchecked("lender-b");
        let entry_a = OpenInterest {
            liquidity_coin: Coin::new(100u128, "uusd"),
            interest_coin: Coin::new(5u128, "uusd"),
            expiry_duration: 86_400u64,
            collateral: Coin::new(200u128, "ujuno"),
        };
        let entry_b = OpenInterest {
            liquidity_coin: Coin::new(250u128, "uusd"),
            interest_coin: Coin::new(15u128, "uusd"),
            expiry_duration: 120_000u64,
            collateral: Coin::new(225u128, "ujuno"),
        };

        COUNTER_OFFERS
            .save(deps.as_mut().storage, &proposer_a, &entry_a)
            .expect("save succeeds");
        COUNTER_OFFERS
            .save(deps.as_mut().storage, &proposer_b, &entry_b)
            .expect("save succeeds");

        let loaded_a = COUNTER_OFFERS
            .load(deps.as_ref().storage, &proposer_a)
            .expect("load succeeds");
        assert_eq!(loaded_a, entry_a);

        let all: Vec<_> = COUNTER_OFFERS
            .range(deps.as_ref().storage, None, None, Order::Ascending)
            .map(|entry| entry.expect("range succeeds"))
            .collect();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn max_counter_offers_matches_u8_capacity() {
        assert_eq!(MAX_COUNTER_OFFERS, u8::MAX);
        assert_eq!(MAX_COUNTER_OFFERS as usize, 255usize);
    }
}

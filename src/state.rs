use crate::types::OpenInterest;
use cosmwasm_std::Addr;
use cw_storage_plus::Item;

pub const OWNER: Item<Addr> = Item::new("owner");
pub const LENDER: Item<Option<Addr>> = Item::new("lender");
pub const OUTSTANDING_DEBT: Item<u128> = Item::new("outstanding_debt");
pub const OPEN_INTEREST: Item<Option<OpenInterest>> = Item::new("open_interest");

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::{testing::mock_dependencies, Coin};

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

        assert!(missing.is_none());
    }

    #[test]
    fn outstanding_debt_item_handles_amount() {
        let mut deps = mock_dependencies();
        let amount = 50u128;

        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &amount)
            .expect("save succeeds");

        let loaded = OUTSTANDING_DEBT
            .load(deps.as_ref().storage)
            .expect("query succeeds");

        assert_eq!(loaded, amount);
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
}

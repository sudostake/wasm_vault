use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::Item;

pub const OWNER: Item<Addr> = Item::new("owner");
pub const OUTSTANDING_DEBT: Item<Uint128> = Item::new("outstanding_debt");

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::{testing::mock_dependencies, Uint128};

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
    fn outstanding_debt_item_handles_amount() {
        let mut deps = mock_dependencies();
        let amount = Uint128::new(50);

        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &amount)
            .expect("save succeeds");

        let loaded = OUTSTANDING_DEBT
            .load(deps.as_ref().storage)
            .expect("query succeeds");

        assert_eq!(loaded, amount);
    }
}

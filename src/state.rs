use cosmwasm_std::Addr;
use cw_storage_plus::Item;

use crate::types::OutstandingDebt;

pub const OWNER: Item<Addr> = Item::new("owner");
pub const OUTSTANDING_DEBT: Item<OutstandingDebt> = Item::new("outstanding_debt");

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
        let debt = OutstandingDebt {
            amount: Uint128::new(50),
        };

        OUTSTANDING_DEBT
            .save(deps.as_mut().storage, &debt)
            .expect("save succeeds");

        let loaded = OUTSTANDING_DEBT
            .may_load(deps.as_ref().storage)
            .expect("query succeeds");

        assert_eq!(loaded, Some(debt));
    }
}

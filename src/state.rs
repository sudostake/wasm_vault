use cosmwasm_std::Addr;
use cw_storage_plus::Item;

pub const OWNER: Item<Addr> = Item::new("owner");

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::mock_dependencies;

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
}

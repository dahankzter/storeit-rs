#![allow(unexpected_cfgs)]
use storeit::{Entity, Fetchable};

#[derive(Entity, Clone, Debug)]
struct User {
    #[fetch(id)]
    id: Option<i64>,
    email: String,
}

#[test]
fn facade_reexports_and_entity_metadata() {
    // Access associated constants via Fetchable (re-exported from storeit_core)
    assert_eq!(User::TABLE, "users");
    assert_eq!(User::SELECT_COLUMNS, &["id", "email"]);

    // Ensure the macro generated a RowAdapter type with the expected name in the consumer crate.
    // We don't construct rows here; just ensure the type exists and is name-resolved.
    let _adapter = UserRowAdapter;
}

use storeit_macros::Entity;
use storeit_core::Fetchable;

#[derive(Entity, Clone, Debug, PartialEq)]
#[entity(table = "people")]
struct Person {
    #[fetch(id)]
    id: i64,
    #[fetch(column = "email_address")]
    email: String,
    #[fetch(column = "full_name")]
    name: String,
}

fn main() {
    assert_eq!(Person::TABLE, "people");
    assert_eq!(Person::SELECT_COLUMNS, &["id", "email_address", "full_name"]);
    let _adapter = PersonRowAdapter;
    let _ = _adapter;
}
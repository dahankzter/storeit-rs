use storeit_macros::Entity;

#[derive(Entity)]
struct BadCol {
    #[fetch(id)]
    id: i64,
    #[fetch(column = "bad name")]
    email: String,
}

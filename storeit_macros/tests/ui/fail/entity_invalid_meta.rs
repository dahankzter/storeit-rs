use storeit_macros::Entity;

#[derive(Entity)]
struct BadAttr {
    #[fetch(column)] // missing = "..." value, should fail to parse
    email: String,
    #[fetch(id)]
    id: i64,
}

fn main() {}

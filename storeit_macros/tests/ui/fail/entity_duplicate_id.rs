use storeit_macros::Entity;

#[derive(Entity)]
struct BadIds {
    #[fetch(id)]
    id1: i64,
    #[fetch(id)]
    id2: i64,
    email: String,
}

fn main() {}

use storeit_macros::{Entity, repository};

#[derive(Entity)]
struct User { #[fetch(id)] id: i64, email: String }

#[repository(entity = User, backend = UnknownDb)]
mod users_repo {}

fn main() {}

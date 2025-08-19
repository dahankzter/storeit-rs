use storeit_macros::{Entity, repository};

#[derive(Entity)]
struct User { #[fetch(id)] id: i64, email: String }

// Using a bare `entity` path (no value) should trigger "Unsupported attribute format"
#[repository(entity)]
mod users_repo {}

fn main() {}

use storeit_macros::{Entity, repository};

#[derive(Entity)]
struct User { #[fetch(id)] id: i64, email: String }

// Missing type after finder name should cause a parsing error inside the finders list
#[repository(entity = User, backend = Libsql, finders(find_by_email))]
mod users_repo {}

fn main() {}

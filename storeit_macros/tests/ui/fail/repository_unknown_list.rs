use storeit_macros::{Entity, repository};

#[derive(Entity)]
struct User { #[fetch(id)] id: i64, email: String }

// Unknown attribute list name should trigger the parser's "Unknown attribute list" error
#[repository(entity = User, backend = Libsql, wronglist(find_by_email: String))]
mod users_repo {}

fn main() {}

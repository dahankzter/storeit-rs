use storeit_macros::{Entity, repository};

#[derive(Entity)]
struct User { #[fetch(id)] id: i64, email: String }

// Misspelled NameValue key (bekkend) should trigger the parser's "Unknown attribute" error
#[repository(entity = User, bekkend = Libsql)]
mod users_repo {}

fn main() {}

use storeit_macros::{Entity, repository};

#[derive(Entity)]
struct User { #[fetch(id)] id: i64, email: String }

// u128 is not among the supported finder parameter types
#[repository(entity = User, backend = TokioPostgres, finders(find_by_count: u128))]
mod users_repo {}

fn main() {}

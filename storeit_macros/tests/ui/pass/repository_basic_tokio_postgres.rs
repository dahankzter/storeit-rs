use storeit_macros::{Entity, repository};
use storeit_core::{Fetchable, RowAdapter};

#[derive(Entity, Clone, Debug)]
struct User {
    #[fetch(id)]
    id: Option<i64>,
    email: String,
}

// Dummy adapter to satisfy compile-time bounds; body not executed in this test
struct A;
impl RowAdapter<User> for A {
    type Row = ();
    fn from_row(&self, _row: &Self::Row) -> storeit_core::RepoResult<User> { unreachable!() }
}

#[repository(entity = User, backend = TokioPostgres, finders(find_by_email: String))]
mod users_repo {}

fn main() {
    // Compiles if everything is wired; no instantiation needed.
    let _ = User::TABLE;
}

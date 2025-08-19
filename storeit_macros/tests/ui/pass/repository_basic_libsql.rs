use storeit_macros::{Entity, repository};
use storeit_core::{Fetchable, RowAdapter};

#[derive(Entity, Clone, Debug)]
struct User {
    #[fetch(id)]
    id: Option<i64>,
    email: String,
}

// Provide a dummy adapter satisfying bounds; the body won't be executed in this compile-pass test.
struct A;
impl RowAdapter<User> for A {
    type Row = ();
    fn from_row(&self, _row: &Self::Row) -> storeit_core::RepoResult<User> { unreachable!() }
}

#[repository(entity = User, backend = Libsql, finders(find_by_email: String))]
mod users_repo {}

fn main() {
    // The test passes if this compiles. We don't need to instantiate the repo here.
    let _ = User::TABLE;
}

// Enable with: cargo run -p storeit_libsql --features libsql-backend --example basic

#[cfg(feature = "libsql-backend")]
fn main() -> Result<(), storeit_core::RepoError> {
    use libsql::Database;
    use storeit_core::{Fetchable, RepoError, RowAdapter};
    use storeit_libsql::LibsqlRepository;

    #[derive(storeit_macros::Entity, Clone, Debug)]
    #[entity(table = "users")]
    struct User {
        #[fetch(id)]
        id: Option<i64>,
        email: String,
    }

    struct UserAdapter;
    impl RowAdapter<User> for UserAdapter {
        type Row = libsql::Row;
        fn from_row(&self, row: &Self::Row) -> storeit_core::RepoResult<User> {
            // Column order follows metadata: id, email
            let id: i64 = row.get(0).map_err(storeit_core::RepoError::mapping)?;
            let email: String = row.get(1).map_err(storeit_core::RepoError::mapping)?;
            Ok(User {
                id: Some(id),
                email,
            })
        }
    }

    // In-memory DB just to demonstrate repository construction
    #[allow(deprecated)]
    let db = Database::open(":memory:").map_err(RepoError::backend)?;
    let _repo: LibsqlRepository<User, UserAdapter> =
        LibsqlRepository::new(std::sync::Arc::new(db), UserAdapter);
    println!("Constructed LibsqlRepository for User entity");
    Ok(())
}

#[cfg(not(feature = "libsql-backend"))]
fn main() {
    eprintln!("Enable feature libsql-backend to run this example");
}

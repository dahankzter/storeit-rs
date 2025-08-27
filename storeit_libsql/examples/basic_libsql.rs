// Enable with: cargo run -p storeit_libsql --features libsql-backend --example basic

#[cfg(feature = "libsql-backend")]
fn main() -> Result<(), storeit_core::RepoError> {
    use libsql::Database;
    use storeit_core::{RepoError, RowAdapter};
    use storeit_libsql::LibsqlRepository;

    #[derive(Clone, Debug)]
    struct User {
        id: Option<i64>,
        email: String,
    }

    impl storeit_core::Fetchable for User {
        const TABLE: &'static str = "users";
        const SELECT_COLUMNS: &'static [&'static str] = &["id", "email"];
        const FINDABLE_COLUMNS: &'static [(&'static str, &'static str)] = &[("email", "TEXT")];
    }
    impl storeit_core::Identifiable for User {
        type Key = i64;
        const ID_COLUMN: &'static str = "id";
        fn id(&self) -> Option<Self::Key> {
            self.id
        }
    }
    impl storeit_core::Insertable for User {
        const INSERT_COLUMNS: &'static [&'static str] = &["email"];
        fn insert_values(&self) -> Vec<storeit_core::ParamValue> {
            vec![storeit_core::ParamValue::String(self.email.clone())]
        }
    }
    impl storeit_core::Updatable for User {
        const UPDATE_COLUMNS: &'static [&'static str] = &["email", "id"];
        fn update_values(&self) -> Vec<storeit_core::ParamValue> {
            vec![
                storeit_core::ParamValue::String(self.email.clone()),
                storeit_core::ParamValue::I64(self.id.unwrap_or_default()),
            ]
        }
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

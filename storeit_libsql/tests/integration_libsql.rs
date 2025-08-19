#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

use storeit_core::{Repository, RowAdapter};
use tests_common::{migrations, User};

#[cfg(feature = "libsql-backend")]
struct MyAdapter;

#[cfg(feature = "libsql-backend")]
impl RowAdapter<User> for MyAdapter {
    type Row = libsql::Row;

    fn from_row(&self, row: &Self::Row) -> storeit_core::RepoResult<User> {
        // Column order follows tests_common::User derived metadata: id, email, active
        let id: i64 = row.get(0).map_err(storeit_core::RepoError::mapping)?;
        let email: String = row.get(1).map_err(storeit_core::RepoError::mapping)?;
        let active_i64: i64 = row.get(2).map_err(storeit_core::RepoError::mapping)?; // booleans stored as 0/1
        Ok(User {
            id: Some(id),
            email,
            active: active_i64 != 0,
        })
    }
}

#[cfg(feature = "libsql-backend")]
#[tokio::test]
#[ignore = "Excluded from default runs to keep coverage fast and deterministic; run with -- --ignored to execute"]
async fn libsql_crud_and_find_by_field_in_memory() -> storeit_core::RepoResult<()> {
    use libsql::Database;
    use storeit_libsql::LibsqlRepository;

    // Shared in-memory database across connections
    #[allow(deprecated)]
    let db =
        Database::open("file::memory:?cache=shared").map_err(storeit_core::RepoError::backend)?;
    // Run migrations
    let conn = db.connect().map_err(storeit_core::RepoError::backend)?;
    conn.execute(migrations::LIBSQL_USERS_SQL, ())
        .await
        .expect("apply migrations");

    // Build repository
    let repo: LibsqlRepository<User, MyAdapter> =
        LibsqlRepository::new(std::sync::Arc::new(db), MyAdapter);

    // Insert
    let u = User {
        id: None,
        email: "c@example.com".into(),
        active: true,
    };
    let created = repo.insert(&u).await?;
    assert!(created.id.is_some());

    // Find by id
    let fetched = repo.find_by_id(&created.id.unwrap()).await?;
    assert!(fetched.is_some());
    assert_eq!(fetched.as_ref().unwrap().email, "c@example.com");

    // Update
    let mut to_update = fetched.unwrap();
    to_update.active = false;
    let updated = repo.update(&to_update).await?;
    assert!(!updated.active);

    // find_by_field
    let found = repo
        .find_by_field(
            "email",
            storeit_core::ParamValue::String("c@example.com".into()),
        )
        .await?;
    assert_eq!(found.len(), 1);

    // Delete
    let ok = repo.delete_by_id(&updated.id.unwrap()).await?;
    assert!(ok);

    Ok(())
}

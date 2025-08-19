// Run with:
//   cargo run -p repository --no-default-features --features libsql-backend --example libsql_e2e
// Requires: the libsql feature on the facade, which pulls in storeit_libsql.

#![allow(unexpected_cfgs)]

use storeit::{Repository as _, RowAdapter};

#[derive(storeit::Entity, Clone, Debug, PartialEq)]
pub struct User {
    #[fetch(id)]
    pub id: Option<i64>,
    pub email: String,
    pub active: bool,
}

// Manual RowAdapter for libsql rows
struct UserAdapter;
impl RowAdapter<User> for UserAdapter {
    type Row = libsql::Row;
    fn from_row(&self, row: &Self::Row) -> storeit::RepoResult<User> {
        let id: i64 = row.get(0).map_err(storeit::RepoError::mapping)?;
        let email: String = row.get(1).map_err(storeit::RepoError::mapping)?;
        let active_i64: i64 = row.get(2).map_err(storeit::RepoError::mapping)?; // 0/1 in SQLite
        Ok(User {
            id: Some(id),
            email,
            active: active_i64 != 0,
        })
    }
}

#[storeit::repository(entity = User, backend = Libsql, finders(find_by_email: String))]
pub mod users_repo {}

#[tokio::main]
async fn main() -> Result<(), storeit::RepoError> {
    // Shared in-memory database
    #[allow(deprecated)]
    let db_url = "file::memory:?cache=shared";

    // Create schema (minimal migration)
    {
        #[allow(deprecated)]
        let db = libsql::Database::open(db_url).map_err(storeit::RepoError::backend)?;
        let conn = db.connect().map_err(storeit::RepoError::backend)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS users (\n  id INTEGER PRIMARY KEY AUTOINCREMENT,\n  email TEXT NOT NULL UNIQUE,\n  active INTEGER NOT NULL\n);",
            (),
        )
        .await.map_err(storeit::RepoError::backend)?;
    }

    // Build typed repository using the explicit adapter
    let repo: users_repo::Repository<UserAdapter> =
        users_repo::Repository::from_url_with_adapter(db_url, UserAdapter).await?;

    // Insert
    let created = repo
        .insert(&User {
            id: None,
            email: "e2e@sqlite".into(),
            active: true,
        })
        .await?;
    println!("created = {:?}", created);

    // By id
    let by_id = repo.find_by_id(&created.id.unwrap()).await?;
    println!("by_id = {:?}", by_id);

    // Finder
    let found = repo.find_by_email(&"e2e@sqlite".to_string()).await?;
    println!("found = {:?}", found);

    // Update
    let mut u = found.into_iter().next().unwrap();
    u.active = false;
    let updated = repo.update(&u).await?;
    println!("updated = {:?}", updated);

    // Delete
    let ok = repo.delete_by_id(&updated.id.unwrap()).await?;
    println!("deleted? {}", ok);

    // Show TransactionTemplate shape (backend-agnostic)
    let tpl = storeit::transactions::default_transaction_template();
    let _ = tpl
        .execute(|_ctx| async move { Ok::<_, storeit::RepoError>(()) })
        .await?;

    Ok(())
}

// Run with:
//   export POSTGRES_URL=postgres://postgres:postgres@localhost:5432/postgres
//   cargo run -p repository --no-default-features --features postgres-backend --example postgres_e2e

#![allow(unexpected_cfgs)]

use storeit::{Repository as _, RowAdapter};

#[derive(storeit::Entity, Clone, Debug, PartialEq)]
pub struct User {
    #[fetch(id)]
    pub id: Option<i64>,
    pub email: String,
    pub active: bool,
}

// Manual RowAdapter for tokio_postgres rows
struct UserAdapter;
impl RowAdapter<User> for UserAdapter {
    type Row = tokio_postgres::Row;
    fn from_row(&self, row: &Self::Row) -> storeit::RepoResult<User> {
        let id: i64 = row.get(0);
        let email: String = row.get(1);
        let active: bool = row.get(2);
        Ok(User {
            id: Some(id),
            email,
            active,
        })
    }
}

#[storeit::repository(entity = User, backend = TokioPostgres, finders(find_by_email: String))]
pub mod users_repo {}

#[tokio::main]
async fn main() -> Result<(), storeit::RepoError> {
    let url = std::env::var("POSTGRES_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/postgres".to_string());

    // Create schema (minimal migration)
    {
        let (client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
            .await
            .map_err(storeit::RepoError::backend)?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        client
            .batch_execute(
                "CREATE TABLE IF NOT EXISTS users (\n  id BIGSERIAL PRIMARY KEY,\n  email TEXT NOT NULL UNIQUE,\n  active BOOLEAN NOT NULL\n);",
            )
            .await.map_err(storeit::RepoError::backend)?;
    }

    let repo: users_repo::Repository<UserAdapter> =
        users_repo::Repository::from_url_with_adapter(&url, UserAdapter).await?;

    let created = repo
        .insert(&User {
            id: None,
            email: "e2e@pg".into(),
            active: true,
        })
        .await?;
    println!("created = {:?}", created);

    let by_id = repo.find_by_id(&created.id.unwrap()).await?;
    println!("by_id = {:?}", by_id);

    let found = repo.find_by_email(&"e2e@pg".to_string()).await?;
    println!("found = {:?}", found);

    let mut u = found.into_iter().next().unwrap();
    u.active = false;
    let updated = repo.update(&u).await?;
    println!("updated = {:?}", updated);

    let ok = repo.delete_by_id(&updated.id.unwrap()).await?;
    println!("deleted? {}", ok);

    let tpl = storeit::transactions::default_transaction_template();
    let _ = tpl
        .execute(|_ctx| async move { Ok::<_, storeit::RepoError>(()) })
        .await?;

    Ok(())
}

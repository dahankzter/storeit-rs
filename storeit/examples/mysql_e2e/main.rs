// Run with:
//   export MYSQL_URL=mysql://user:pass@localhost:3306/test
//   cargo run -p repository --no-default-features --features mysql-async --example mysql_e2e

#![allow(unexpected_cfgs)]

use mysql_async::prelude::Queryable;
use storeit::Repository as _;

#[derive(storeit::Entity, Clone, Debug, PartialEq)]
pub struct User {
    #[fetch(id)]
    pub id: Option<i64>,
    pub email: String,
    pub active: bool,
}

#[storeit::repository(entity = User, backend = MysqlAsync, finders(find_by_email: String))]
pub mod users_repo {}

#[tokio::main]
async fn main() -> Result<(), storeit::RepoError> {
    let url = std::env::var("MYSQL_URL")
        .unwrap_or_else(|_| "mysql://root:root@localhost:3306/test".to_string());

    // Create schema (minimal migration)
    {
        let opts = mysql_async::Opts::from_url(&url).map_err(storeit::RepoError::backend)?;
        let pool = mysql_async::Pool::new(opts);
        let mut conn = pool.get_conn().await.map_err(storeit::RepoError::backend)?;
        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS users (\n  id BIGINT PRIMARY KEY AUTO_INCREMENT,\n  email TEXT NOT NULL,\n  active BOOLEAN NOT NULL\n);",
        )
        .await.map_err(storeit::RepoError::backend)?;
    }

    let repo: users_repo::Repository<UserRowAdapter> =
        users_repo::Repository::from_url_with_adapter(&url, UserRowAdapter).await?;

    let created = repo
        .insert(&User {
            id: None,
            email: "e2e@mysql".into(),
            active: true,
        })
        .await?;
    println!("created = {:?}", created);

    let by_id = repo.find_by_id(&created.id.unwrap()).await?;
    println!("by_id = {:?}", by_id);

    let found = repo.find_by_email(&"e2e@mysql".to_string()).await?;
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

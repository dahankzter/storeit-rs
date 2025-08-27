// Run with:
//   cargo run -p repository --no-default-features --features libsql-backend --example libsql_tx
// Demonstrates explicit transactions with the LibSQL backend (commit and rollback).

#![allow(unexpected_cfgs)]

use storeit::Repository as _;

#[derive(storeit::Entity, Clone, Debug, PartialEq)]
pub struct User {
    #[fetch(id)]
    pub id: Option<i64>,
    pub email: String,
    pub active: bool,
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
        .await
        .map_err(storeit::RepoError::backend)?;
    }

    // Build a repository bound to the DB url for non-transactional operations
    let repo: users_repo::Repository<UserRowAdapter> =
        users_repo::Repository::from_url_with_adapter(db_url, UserRowAdapter).await?;

    // 1) A successful transaction (commit)
    {
        use storeit_core::transactions::{Isolation, Propagation, TransactionDefinition};
        use storeit_libsql::LibsqlTransactionManager;
        #[allow(deprecated)]
        let db = std::sync::Arc::new(
            libsql::Database::open(db_url).map_err(storeit::RepoError::backend)?,
        );
        let mgr = LibsqlTransactionManager::from_arc(db);
        let def = TransactionDefinition {
            propagation: Propagation::Required,
            isolation: Isolation::Default,
            read_only: false,
            timeout: None,
        };
        let email1 = "tx_commit@sqlite".to_string();
        mgr.execute(&def, |_ctx| {
            let email1 = email1.clone();
            async move {
                // Build a repo that participates in the active transaction
                let repo_tx: users_repo::Repository<UserRowAdapter> =
                    users_repo::Repository::from_url_with_adapter(db_url, UserRowAdapter).await?;
                let _ = repo_tx
                    .insert(&User {
                        id: None,
                        email: email1.clone(),
                        active: true,
                    })
                    .await?;
                Ok::<_, storeit::RepoError>(())
            }
        })
        .await?;
        // After commit, we should see the user
        let found = repo.find_by_email(&email1).await?;
        assert_eq!(found.len(), 1);
        println!("commit example: created {}", email1);
    }

    // 2) A failing transaction (rollback)
    {
        use storeit_core::transactions::{Isolation, Propagation, TransactionDefinition};
        use storeit_libsql::LibsqlTransactionManager;
        #[allow(deprecated)]
        let db = std::sync::Arc::new(
            libsql::Database::open(db_url).map_err(storeit::RepoError::backend)?,
        );
        let mgr = LibsqlTransactionManager::from_arc(db);
        let def = TransactionDefinition {
            propagation: Propagation::Required,
            isolation: Isolation::Default,
            read_only: false,
            timeout: None,
        };
        let email2 = "tx_rollback@sqlite".to_string();
        let res = mgr
            .execute(&def, |_ctx| {
                let email2 = email2.clone();
                async move {
                    let repo_tx: users_repo::Repository<UserRowAdapter> =
                        users_repo::Repository::from_url_with_adapter(db_url, UserRowAdapter)
                            .await?;
                    let _ = repo_tx
                        .insert(&User {
                            id: None,
                            email: email2.clone(),
                            active: true,
                        })
                        .await?;
                    // Force an error so the tx rolls back
                    Err::<(), _>(storeit::RepoError::backend(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "boom",
                    )))
                }
            })
            .await;
        assert!(res.is_err());
        let found = repo.find_by_email(&email2).await?;
        assert!(found.is_empty());
        println!("rollback example: insertion of {} rolled back", email2);
    }

    Ok(())
}

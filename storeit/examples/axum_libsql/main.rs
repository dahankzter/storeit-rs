// Run with:
//   cargo run -p repository --no-default-features --features libsql-backend --example axum_libsql
// Starts an Axum server backed by an in-memory libsql database.

#![allow(unexpected_cfgs)]

use axum::{
    extract::Path,
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use storeit::Repository as _;
use storeit_core::transactions::TransactionManager;

#[derive(storeit::Entity, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct User {
    #[fetch(id)]
    pub id: Option<i64>,
    pub email: String,
    pub active: bool,
}

#[storeit::repository(entity = User, backend = Libsql, finders(find_by_email: String))]
pub mod users_repo {}

#[derive(Clone)]
struct AppState {
    db: Arc<libsql::Database>,
    db_url: String,
}

#[derive(Deserialize)]
struct CreateUser {
    email: String,
    active: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Shared in-memory database
    let db_url = "file::memory:?cache=shared".to_string();
    #[allow(deprecated)]
    let db = Arc::new(libsql::Database::open(&db_url)?);
    let conn = db.connect()?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS users (\n  id INTEGER PRIMARY KEY AUTOINCREMENT,\n  email TEXT NOT NULL UNIQUE,\n  active INTEGER NOT NULL\n);",
        (),
    )
    .await?;

    let state = AppState {
        db: db.clone(),
        db_url: db_url.clone(),
    };

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/users/:id", get(get_user))
        .route("/users", post(create_user_tx).get(list_user_by_email))
        .with_state(state);

    let addr: SocketAddr = "127.0.0.1:3000".parse()?;
    println!("Axum listening on http://{addr}");
    axum::serve(tokio::net::TcpListener::bind(addr).await?, app).await?;
    Ok(())
}

async fn get_user(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Option<User>>, String> {
    let repo: users_repo::Repository<UserRowAdapter> =
        users_repo::Repository::from_url_with_adapter(&state.db_url, UserRowAdapter)
            .await
            .map_err(|e| format!("repo: {e:#}"))?;
    let user = repo
        .find_by_id(&id)
        .await
        .map_err(|e| format!("find_by_id: {e:#}"))?;
    Ok(Json(user))
}

// GET /users?email=foo@example.com
async fn list_user_by_email(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Vec<User>>, String> {
    let email = params.get("email").cloned().unwrap_or_default();
    let repo: users_repo::Repository<UserRowAdapter> =
        users_repo::Repository::from_url_with_adapter(&state.db_url, UserRowAdapter)
            .await
            .map_err(|e| format!("repo: {e:#}"))?;
    let users = repo
        .find_by_email(&email)
        .await
        .map_err(|e| format!("find_by_email: {e:#}"))?;
    Ok(Json(users))
}

// POST /users (executed inside a transaction using the LibsqlTransactionManager)
async fn create_user_tx(
    State(state): State<AppState>,
    Json(payload): Json<CreateUser>,
) -> Result<Json<User>, String> {
    use storeit_core::transactions::{Isolation, Propagation, TransactionDefinition};
    use storeit_libsql::LibsqlTransactionManager;

    let mgr = LibsqlTransactionManager::from_arc(state.db.clone());
    let def = TransactionDefinition {
        propagation: Propagation::Required,
        isolation: Isolation::Default,
        read_only: false,
        timeout: None,
    };

    let email = payload.email.clone();
    let active = payload.active;

    let created = mgr
        .execute(&def, |_ctx| {
            let db_url = state.db_url.clone();
            async move {
                let repo: users_repo::Repository<UserRowAdapter> =
                    users_repo::Repository::from_url_with_adapter(&db_url, UserRowAdapter).await?;
                let user = repo
                    .insert(&User {
                        id: None,
                        email,
                        active,
                    })
                    .await?;
                Ok::<_, storeit::RepoError>(user)
            }
        })
        .await
        .map_err(|e| format!("tx: {e:#}"))?;

    Ok(Json(created))
}

// Run with:
//   cargo run -p repository --no-default-features --features libsql-backend --example warp_libsql
// Starts a Warp server backed by an in-memory libsql database.

#![allow(unexpected_cfgs)]

use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use storeit::Repository as _;
use storeit_core::transactions::TransactionManager;
use warp::Filter;

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
async fn main() {
    // Shared in-memory database
    let db_url = "file::memory:?cache=shared".to_string();
    #[allow(deprecated)]
    let db = Arc::new(libsql::Database::open(&db_url).expect("open db"));
    let conn = db.connect().expect("conn");
    conn.execute(
        "CREATE TABLE IF NOT EXISTS users (\n  id INTEGER PRIMARY KEY AUTOINCREMENT,\n  email TEXT NOT NULL UNIQUE,\n  active INTEGER NOT NULL\n);",
        (),
    )
    .await
    .expect("schema");

    let state = AppState {
        db: db.clone(),
        db_url: db_url.clone(),
    };

    let state_filter = warp::any().map(move || state.clone());

    let health = warp::path("health").and(warp::get()).map(|| "ok");

    let get_user = warp::path!("users" / i64)
        .and(warp::get())
        .and(state_filter.clone())
        .and_then(|id: i64, state: AppState| async move {
            let repo: users_repo::Repository<UserRowAdapter> =
                users_repo::Repository::from_url_with_adapter(&state.db_url, UserRowAdapter)
                    .await
                    .map_err(|e| warp::reject::custom(StringError(format!("repo: {e:#}"))))?;
            let user = repo
                .find_by_id(&id)
                .await
                .map_err(|e| warp::reject::custom(StringError(format!("find_by_id: {e:#}"))))?;
            Ok::<_, Infallible>(warp::reply::json(&user))
        });

    let list_by_email = warp::path("users")
        .and(warp::get())
        .and(state_filter.clone())
        .and(warp::query::<std::collections::HashMap<String, String>>())
        .and_then(
            |state: AppState, params: std::collections::HashMap<String, String>| async move {
                let email = params.get("email").cloned().unwrap_or_default();
                let repo: users_repo::Repository<UserRowAdapter> =
                    users_repo::Repository::from_url_with_adapter(&state.db_url, UserRowAdapter)
                        .await
                        .map_err(|e| warp::reject::custom(StringError(format!("repo: {e:#}"))))?;
                let users = repo.find_by_email(&email).await.map_err(|e| {
                    warp::reject::custom(StringError(format!("find_by_email: {e:#}")))
                })?;
                Ok::<_, Infallible>(warp::reply::json(&users))
            },
        );

    let create_user = warp::path("users")
        .and(warp::post())
        .and(state_filter.clone())
        .and(warp::body::json())
        .and_then(|state: AppState, payload: CreateUser| async move {
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
                            users_repo::Repository::from_url_with_adapter(&db_url, UserRowAdapter)
                                .await?;
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
                .map_err(|e| warp::reject::custom(StringError(format!("tx: {e:#}"))))?;
            Ok::<_, Infallible>(warp::reply::json(&created))
        });

    let routes = health.or(get_user).or(list_by_email).or(create_user);

    println!("Warp listening on http://127.0.0.1:3030");
    warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
}

// Minimal custom rejection to carry strings
#[derive(Debug)]
struct StringError(String);
impl warp::reject::Reject for StringError {}

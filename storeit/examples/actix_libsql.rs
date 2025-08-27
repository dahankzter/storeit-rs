// Run with:
//   cargo run -p repository --no-default-features --features libsql-backend --example actix_libsql
// Starts an Actix-Web server backed by an in-memory libsql database.

#![allow(unexpected_cfgs)]

use actix_web::{get, post, web, App, HttpResponse, HttpServer, Responder};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use storeit::{Repository as _, RowAdapter};

#[derive(storeit::Entity, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct User {
    #[fetch(id)]
    pub id: Option<i64>,
    pub email: String,
    pub active: bool,
}

// Manual RowAdapter for libsql rows
struct UserAdapter;
impl storeit::RowAdapter<User> for UserAdapter {
    type Row = libsql::Row;
    fn from_row(&self, row: &Self::Row) -> storeit::RepoResult<User> {
        let id: i64 = row.get(0).map_err(storeit::RepoError::mapping)?;
        let email: String = row.get(1).map_err(storeit::RepoError::mapping)?;
        let active_i64: i64 = row.get(2).map_err(storeit::RepoError::mapping)?; // 0/1 in SQLite
        Ok(User { id: Some(id), email, active: active_i64 != 0 })
    }
}

#[storeit::repository(entity = User, backend = Libsql, finders(find_by_email: String))]
pub mod users_repo {}

#[derive(Clone)]
struct AppState { db: Arc<libsql::Database>, db_url: String }

#[derive(Deserialize)]
struct CreateUser { email: String, active: bool }

#[get("/health")]
async fn health() -> impl Responder { HttpResponse::Ok().body("ok") }

#[get("/users/{id}")]
async fn get_user(state: web::Data<AppState>, path: web::Path<i64>) -> actix_web::Result<impl Responder> {
    let id = path.into_inner();
    let repo: users_repo::Repository<UserAdapter> = users_repo::Repository::from_url_with_adapter(&state.db_url, UserAdapter)
        .await
        .map_err(to_http_err)?;
    let user = repo.find_by_id(&id).await.map_err(to_http_err)?;
    Ok(web::Json(user))
}

#[get("/users")]
async fn list_by_email(state: web::Data<AppState>, q: web::Query<std::collections::HashMap<String, String>>) -> actix_web::Result<impl Responder> {
    let email = q.get("email").cloned().unwrap_or_default();
    let repo: users_repo::Repository<UserAdapter> = users_repo::Repository::from_url_with_adapter(&state.db_url, UserAdapter)
        .await
        .map_err(to_http_err)?;
    let users = repo.find_by_email(&email).await.map_err(to_http_err)?;
    Ok(web::Json(users))
}

#[post("/users")]
async fn create_user_tx(state: web::Data<AppState>, payload: web::Json<CreateUser>) -> actix_web::Result<impl Responder> {
    use storeit_core::transactions::{Isolation, Propagation, TransactionDefinition};
    use storeit_libsql::LibsqlTransactionManager;

    let mgr = LibsqlTransactionManager::from_arc(state.db.clone());
    let def = TransactionDefinition { propagation: Propagation::Required, isolation: Isolation::Default, read_only: false, timeout: None };
    let email = payload.email.clone();
    let active = payload.active;
    let created = mgr
        .execute(&def, |_ctx| {
            let db_url = state.db_url.clone();
            async move {
                let repo: users_repo::Repository<UserAdapter> = users_repo::Repository::from_url_with_adapter(&db_url, UserAdapter).await?;
                let user = repo.insert(&User { id: None, email, active }).await?;
                Ok::<_, storeit::RepoError>(user)
            }
        })
        .await
        .map_err(to_http_err)?;
    Ok(web::Json(created))
}

fn to_http_err(e: storeit::RepoError) -> actix_web::Error {
    actix_web::error::ErrorInternalServerError(format!("{e:#}"))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
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

    let state = AppState { db: db.clone(), db_url: db_url.clone() };

    println!("Actix-Web listening on http://127.0.0.1:8080");
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(state.clone()))
            .service(health)
            .service(get_user)
            .service(list_by_email)
            .service(create_user_tx)
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}

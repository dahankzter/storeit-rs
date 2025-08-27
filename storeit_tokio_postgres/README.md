# storeit_tokio_postgres

PostgreSQL backend adapter for `storeit`, built on `tokio-postgres`.

- Feature: `postgres-backend` enables the implementation using tokio-postgres and Tokio runtime.
- Provides `TokioPostgresRepository<T, A>` and `TokioPostgresTransactionManager`.

Quick start:
```rust
use storeit_core::{RowAdapter, Repository};
use storeit_tokio_postgres::TokioPostgresRepository;

#[derive(Clone, Debug)]
struct User { id: Option<i64>, email: String, active: bool }
// implement Fetchable/Identifiable/Insertable/Updatable for User...
struct UserAdapter;
impl RowAdapter<User> for UserAdapter {
    type Row = tokio_postgres::Row;
    fn from_row(&self, row: &Self::Row) -> storeit_core::RepoResult<User> { /* map columns */ }
}
# async fn demo() -> storeit_core::RepoResult<()> {
let repo = TokioPostgresRepository::from_url("postgres://postgres:postgres@localhost/postgres", "id", UserAdapter).await?;
let _ = repo.find_by_id(&1).await?;
# Ok(()) }
```

See integration tests under `storeit_tokio_postgres/tests` for usage with Testcontainers.

MSRV: 1.70
License: MIT OR Apache-2.0

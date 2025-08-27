# storeit_libsql

LibSQL/Turso backend adapter for the `storeit` repository framework.

- Feature: `libsql-backend` enables the implementation using the `libsql` crate and Tokio runtime.
- Implements the async `Repository<T>` for your entities and provides a `LibsqlTransactionManager` for transaction semantics (including nested savepoints).

Quick start:
```rust
use storeit_core::{RowAdapter, Repository};
use storeit_libsql::LibsqlRepository;

#[derive(Clone, Debug)]
struct User { id: Option<i64>, email: String, active: bool }
// implement Fetchable/Identifiable/Insertable/Updatable for User...
struct UserAdapter;
impl RowAdapter<User> for UserAdapter {
    type Row = libsql::Row;
    fn from_row(&self, row: &Self::Row) -> storeit_core::RepoResult<User> { /* map columns */ }
}
# async fn demo() -> storeit_core::RepoResult<()> {
let repo = LibsqlRepository::from_url("file:./db.sqlite3", UserAdapter).await?;
let _ = repo.find_by_id(&1).await?;
# Ok(()) }
```

More runnable examples are available in the workspace under `storeit/examples/`.

MSRV: 1.70
License: MIT OR Apache-2.0

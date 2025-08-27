# storeit_mysql_async

MySQL backend adapter for `storeit`, built on `mysql_async`.

- Feature: `mysql-async` enables the implementation using the mysql_async crate and Tokio runtime.
- Provides `MysqlAsyncRepository<T, A>` and a `MysqlAsyncTransactionManager`.

Quick start:
```rust
use storeit_core::{RowAdapter, Repository};
use storeit_mysql_async::MysqlAsyncRepository;

#[derive(Clone, Debug)]
struct User { id: Option<i64>, email: String, active: bool }
// implement Fetchable/Identifiable/Insertable/Updatable for User...
struct UserAdapter;
impl RowAdapter<User> for UserAdapter {
    type Row = mysql_async::Row;
    fn from_row(&self, row: &Self::Row) -> storeit_core::RepoResult<User> { /* map columns */ }
}
# async fn demo() -> storeit_core::RepoResult<()> {
let repo = MysqlAsyncRepository::from_url("mysql://user:pass@localhost:3306/db", UserAdapter).await?;
let _ = repo.find_by_id(&1).await?;
# Ok(()) }
```

Integration tests (ignored by default) can run against a MariaDB/MySQL Testcontainers image.

MSRV: 1.70
License: MIT OR Apache-2.0

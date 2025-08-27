# storeit

Facade crate for the `storeit` repository framework.

This crate re-exports the core traits and macros so you can get started with a single dependency and feature flags to choose your backend(s).

Key features:
- `#[derive(Entity)]` and `#[repository]` macros (via `storeit_macros`)
- Optional helpers: `query-ext`, `batch-ext`, `stream-ext`, `upsert-ext`
- Backend selection features to pull adapters transitively:
  - `libsql-backend` (via `storeit_libsql`)
  - `postgres-backend` (via `storeit_tokio_postgres`)
  - `mysql-async` (via `storeit_mysql_async`)

Quick taste (see workspace README for complete examples):
```rust
use storeit::{Entity, Repository};

#[derive(Entity, Clone, Debug)]
pub struct User {
    #[fetch(id)]
    pub id: Option<i64>,
    pub email: String,
    pub active: bool,
}
```

Links:
- Workspace guide and examples: https://github.com/dahankzter/storeit-rs#readme

MSRV: 1.70
License: MIT OR Apache-2.0

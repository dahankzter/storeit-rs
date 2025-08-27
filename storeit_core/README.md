# storeit_core

Core traits, errors, and transaction abstractions for the storeit repository framework.

This crate is backend-agnostic. It defines:
- Traits: Fetchable, Identifiable, Insertable, Updatable, Repository, RowAdapter
- Common parameter enum: ParamValue
- Error type: RepoError and alias RepoResult
- Generic transactions API: TransactionManager, TransactionTemplate, Propagation, Isolation

Use this crate directly if you are implementing your own backend adapter or macros, or depend on the top-level `storeit` facade for an easier getting-started experience.

Links:
- Workspace README with full guide: https://github.com/dahankzter/storeit-rs#readme
- Crates in this family: `storeit` (facade), `storeit_macros`, `storeit_sql_builder`, adapters like `storeit_tokio_postgres`, `storeit_mysql_async`, `storeit_libsql`.

Minimum Supported Rust Version (MSRV): 1.70
License: MIT OR Apache-2.0
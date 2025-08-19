# Architecture Overview

This document describes the current architecture of storeit-rs at a high level: the crates/layers, how code is generated, how repositories execute against different SQL backends, and how transactions are handled in a backend-agnostic way.

Last updated: 2025-08-19

- Workspace crates and roles
  - storeit_core: Pure abstractions and small types
    - Traits: Fetchable (compile-time entity metadata), Identifiable (key), Insertable/Updatable (value extraction), RowAdapter<T> (row -> entity), async Repository<T> (CRUD/find), and ParamValue (backend-agnostic parameter values).
    - Transactions module: TransactionDefinition (propagation/isolation/read-only/timeout), TransactionManager, TransactionContext, and a small TransactionTemplate helper (re-exported via the facade crate).
  - storeit_macros: Procedural macros
    - #[derive(Entity)]: Emits metadata and implementations of Identifiable, Insertable, Updatable, and generates a default RowAdapter type per entity.
    - #[repository(entity=..., backend=..., finders(...))]: Generates a typed wrapper module for a chosen backend that forwards to the backend’s generic repository and synthesizes derived find_by_<field> methods.
  - storeit_sql_builder: Minimal SQL string builders
    - Uses Fetchable metadata to emit SELECT/INSERT/UPDATE/DELETE strings.
    - Placeholder style is feature-driven ($1.. for Postgres, ? for others).
    - Helpers include select_by_id, delete_by_id, insert, update_by_id, select_all, select_by_field, select_by_is_null, select_by_is_not_null, select_with_pagination.
  - storeit_libsql / storeit_mysql_async / storeit_tokio_postgres: Backend adapters
    - Each crate implements Repository<T> against its driver, converting ParamValue to driver parameter types and mapping driver rows via RowAdapter.
    - They pre-build and cache common SQL strings per repository instance and cache per-field finder SQL.
    - Feature-gated, so consumers select backends via cargo features.
  - storeit (facade): Re-exports core traits, macros, transactions, and exposes backend types behind feature flags; applications can depend on a single crate and optionally alias it as `repository` in Cargo.toml.

- Data flow (happy path)
  1) You define an entity `T` with #[derive(Entity)]. The macro emits metadata (TABLE, SELECT_COLUMNS, etc.) and a default `TRowAdapter`.
  2) You generate a typed repository module with #[repository(...)] choosing a backend (e.g., Libsql, TokioPostgres, MysqlAsync). This wraps a backend repository implementation and adds derived finders.
  3) At runtime, your typed repository delegates to the backend adapter which:
     - Uses precomputed SQL (from storeit_sql_builder) for common operations and a small cache for field-based finders.
     - Binds values via ParamValue -> driver types and maps result rows to entities via RowAdapter.

- Transactions
  - The backend-agnostic TransactionManager trait (storeit_core) defines an execute(...) API (TransactionTemplate is a convenience wrapper around it).
  - Concrete managers exist for libsql, tokio_postgres, and mysql_async. They manage BEGIN/COMMIT/ROLLBACK and emulate RequiresNew/Nested via SAVEPOINTs as best-effort.
  - A task-local stack holds the active transaction connection/client. Repository methods prefer the active transaction handle when present; otherwise they use their own client/pool/connection.
  - This design lets applications:
    - Create repositories once and reuse them both outside and inside transactions.
    - Or explicitly ask a manager to vend a tx-bound repository via manager.repository(ctx, adapter).
  - Isolation/read-only/timeout are applied best-effort per backend; see docs/transactions.md for exact mappings.

- Backend specifics (concise)
  - libsql (SQLite family):
    - Uses libsql::Database/Connection. Inserts default to last_insert_rowid; optional feature can use INSERT ... RETURNING. Read-only via PRAGMA query_only. Timeout via PRAGMA busy_timeout.
  - tokio_postgres (Postgres):
    - Uses a tokio_postgres::Client with a background connection task. Inserts use INSERT ... RETURNING. Isolation/read-only/statement_timeout are applied via SET statements.
  - mysql_async (MySQL):
    - Uses a Pool to acquire connections. Inserts read last_insert_id. Isolation/read-only/innodb_lock_wait_timeout are applied best-effort where supported.

- Error handling
  - Repository methods return storeit_core::RepoResult<T>. Row-mapping errors should be wrapped with RepoError::mapping(e) and SQL/driver errors with RepoError::backend(e). This unified error type is re-exported via the facade as storeit::RepoError/RepoResult.

- Feature flags overview (selected)
  - storeit crate features: libsql-backend, postgres-backend, mysql-async (re-export backend types).
  - storeit_sql_builder: tokio_postgres (dollar placeholders), libsql_returning (optional returning clause), others default to question-mark placeholders.
  - storeit_libsql: libsql-backend (enable real driver), libsql_returning (enable RETURNING flow in both builder and adapter).

- Testing strategy (high-level)
  - Fast unit tests cover builders, basic conversions, and some error surfacing.
  - Real-database integration tests (ignored by default) use testcontainers for Postgres/MySQL and in-memory DBs for libsql; CI has a workflow to run them on a schedule or on demand.

References
- Transactions details: docs/transactions.md
- Adapter-specific notes and examples: see README (Backend-specific notes section)

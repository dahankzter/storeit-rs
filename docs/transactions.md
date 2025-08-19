# Transactions feature mapping (Aug 2025)

This document summarizes how the current TransactionManager implementations map the backend‑agnostic TransactionDefinition settings to each concrete database. The goal is clarity about what is supported today and what is best‑effort.

Backends covered:
- storeit_libsql (SQLite/libSQL)
- storeit_tokio_postgres (PostgreSQL via tokio_postgres)
- storeit_mysql_async (MySQL via mysql_async)

Legend:
- ✓ implemented
- ~ best‑effort / advisory
- ✗ not implemented

## Defaults and ergonomics

TransactionTemplate and TransactionDefinition defaults are simple and consistent across backends:
- Default TransactionDefinition: Propagation::Required, Isolation::Default, read_only = false, timeout = None.
- Use default_transaction_template() for a ready-to-use template.
- Override per-call with execute_with(&def, ...), or set app-wide defaults via TransactionTemplate::with_defaults(def).

Examples (pseudo-usage, works for any backend manager):

```ignore
#![allow(unexpected_cfgs)]
use repository::transactions::{default_transaction_template, TransactionDefinition, Propagation, Isolation, TransactionContext};

// Default settings: Required, Default isolation, read/write, no timeout
let tpl = default_transaction_template();

tpl.execute(|_ctx: TransactionContext| async move {
    // ... work in a transaction with default semantics
    Ok::<_, repository::RepoError>(())
}).await?;

// Per-call read-only transaction with a 500ms timeout
let ro = TransactionDefinition {
    propagation: Propagation::Required,
    isolation: Isolation::Default,
    read_only: true,
    timeout: Some(std::time::Duration::from_millis(500)),
};

tpl.execute_with(&ro, |_ctx: TransactionContext| async move {
    // ... queries only; writes may error or be blocked best‑effort depending on backend
    Ok::<_, repository::RepoError>(())
}).await?;
```

## Propagation semantics

Supported values for all backends today:
- Required: begin a new transaction if none active; otherwise reuse the current one. ✓
- RequiresNew: when a transaction is active, simulate with a SAVEPOINT and RELEASE/ROLLBACK TO SAVEPOINT. ✓ (best‑effort)
- Nested: same mechanism as RequiresNew using SAVEPOINT. ✓ (best‑effort)
- Supports: run non‑transactionally if no active transaction; otherwise run inside the existing one. ✓
- NotSupported: run non‑transactionally; if a transaction exists, do not join it. ✓
- Never: error if a transaction exists. ✓

Notes:
- All three backends implement propagation via task‑local storage to track active transactions/connections and a depth counter for savepoints.

## Isolation levels

Isolation::Default
- libsql: BEGIN DEFERRED. ✓
- postgres: driver default (no explicit SET). ✓
- mysql: driver default (no explicit SET). ✓

Isolation::ReadCommitted
- libsql: BEGIN DEFERRED (closest SQLite mode). ~
- postgres: SET TRANSACTION ISOLATION LEVEL READ COMMITTED. ✓
- mysql: SET TRANSACTION ISOLATION LEVEL READ COMMITTED. ✓ (best‑effort; availability depends on engine)

Isolation::RepeatableRead
- libsql: BEGIN IMMEDIATE. ~ (maps to locking the database for writes earlier)
- postgres: SET TRANSACTION ISOLATION LEVEL REPEATABLE READ. ✓
- mysql: SET TRANSACTION ISOLATION LEVEL REPEATABLE READ. ✓

Isolation::Serializable
- libsql: BEGIN EXCLUSIVE. ~
- postgres: SET TRANSACTION ISOLATION LEVEL SERIALIZABLE. ✓
- mysql: SET TRANSACTION ISOLATION LEVEL SERIALIZABLE. ✓

Notes:
- SQLite/libSQL does not expose the same isolation taxonomy; mappings use BEGIN modes (DEFERRED/IMMEDIATE/EXCLUSIVE) as the closest equivalents.

## Read‑only transactions

TransactionDefinition.read_only = true
- libsql: PRAGMA query_only = ON while the transaction is active; reverted after completion. ~
- postgres: SET TRANSACTION READ ONLY. ✓
- mysql: SET TRANSACTION READ ONLY where supported; otherwise no‑op. ~

Notes:
- Enforcing read‑only at the driver/database level may be limited by the backend and engine configuration. Errors are best‑effort.

## Timeouts

TransactionDefinition.timeout = Some(Duration)
- libsql: PRAGMA busy_timeout = <millis>. ~ (applies to connection; helps fail fast on locks)
- postgres: SET LOCAL statement_timeout = '<millis>ms'. ~ (applies within the transaction)
- mysql: SET SESSION innodb_lock_wait_timeout = <secs>. ~ (approximate; engine‑specific)

Notes:
- Semantics differ across backends. Timeouts generally apply to lock waits or statement execution, not transaction lifetimes.

## Transaction‑scoped repositories

You can reuse the same repository instance inside and outside transactions across all backends:
- mysql_async: Operations automatically pick up the task‑local transaction connection when active. ✓
- libsql: Repository methods prefer the task‑local transaction connection when active; otherwise they use the repository’s own connection or open a new one. ✓ (Aug 2025)
- tokio_postgres: Repository methods prefer the task‑local transaction client when active; otherwise they use the repository’s own client. ✓ (Aug 2025)

Managers also expose helpers to vend explicitly tx‑bound repositories if you prefer that style:
- libsql: LibsqlTransactionManager::repository(ctx, adapter) -> LibsqlRepository<...> bound to the active libsql::Connection if present. ✓
- postgres: TokioPostgresTransactionManager::repository(ctx, adapter) -> TokioPostgresRepository<...> bound to the active tokio_postgres::Client if present. ✓
- mysql: MysqlAsyncTransactionManager::repository(ctx, adapter) -> MysqlAsyncRepository<...>; operations pick up the task‑local transaction connection automatically. ✓

Usage pattern:
- Create a single TransactionTemplate<M> for your chosen backend’s manager.
- Inside tpl.execute(...), you can call methods on your prebuilt repositories directly and they will participate in the active transaction. Alternatively, you can call manager.repository(ctx, adapter) to obtain an explicitly tx‑bound repository for that scope.

## Caveats and known limitations

- SAVEPOINT behavior may vary across databases; Nested/RequiresNew are implemented via savepoints and are best‑effort.
- Read‑only and timeout settings are applied on a best‑effort basis and may not be strictly enforced by all engines.
- Passing ParamValue::Null to find_by_field results in WHERE field = NULL, which returns no rows per SQL three‑valued logic. When you need to match NULLs, use explicit helpers:
  - In SQL builder: use select_by_is_null::<E>("field") or select_by_is_not_null::<E>("field"). These generate the correct WHERE ... IS NULL/IS NOT NULL clause.
  - Facade/backends: today, Repository::find_by_field cannot express IS NULL; prefer a dedicated finder or a small custom method that uses the SQL builder to construct the query string and bind no parameter for the null case.

Example (non-compiled snippet using the builder):
```rust
use storeit_sql_builder::{select_by_is_null, select_by_is_not_null};

// Build a query to fetch rows where email IS NULL for entity E
let sql = select_by_is_null::<E>("email");
// Execute sql using your backend repository/driver, then map rows via your RowAdapter.
```

## References

- Design background: docs/transactions-design.md
- Cross‑backend analysis: docs/backends-analysis.md
